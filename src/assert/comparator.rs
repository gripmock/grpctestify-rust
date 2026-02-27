use super::engine::AssertionResult;
use crate::parser::ast::InlineOptions;
use serde_json::Value;

pub struct JsonComparator;

impl JsonComparator {
    pub fn compare(
        actual: &Value,
        expected: &Value,
        options: &InlineOptions,
    ) -> Vec<AssertionResult> {
        let mut results = Vec::new();

        // 1. Redact fields if needed
        let mut actual_redacted = actual.clone();
        if !options.redact.is_empty() {
            Self::redact_value(&mut actual_redacted, &options.redact);
        }

        // 2. Compare
        Self::compare_recursive(&actual_redacted, expected, "$", options, &mut results);

        results
    }

    fn redact_value(value: &mut Value, fields: &[String]) {
        match value {
            Value::Object(map) => {
                for field in fields {
                    map.remove(field);
                }
                for (_, v) in map.iter_mut() {
                    Self::redact_value(v, fields);
                }
            }
            Value::Array(arr) => {
                for v in arr.iter_mut() {
                    Self::redact_value(v, fields);
                }
            }
            _ => {}
        }
    }

    fn compare_recursive(
        actual: &Value,
        expected: &Value,
        path: &str,
        options: &InlineOptions,
        results: &mut Vec<AssertionResult>,
    ) {
        // Handle Wildcard "*"
        if let Value::String(s) = expected
            && s == "*"
        {
            return; // Matches anything
        }

        // Type mismatch check
        // Numbers can be float/int, so strictly checking discriminants might be too harsh if serde parses differently.
        // But generally types should match.
        // Exception: expected "*" string matches any actual type (handled above).

        match (actual, expected) {
            (Value::Object(act_map), Value::Object(exp_map)) => {
                let resolve_actual_key = |expected_key: &str| -> Option<String> {
                    if act_map.contains_key(expected_key) {
                        return Some(expected_key.to_string());
                    }

                    let camel = snake_to_camel(expected_key);
                    if act_map.contains_key(&camel) {
                        return Some(camel);
                    }

                    let snake = camel_to_snake(expected_key);
                    if act_map.contains_key(&snake) {
                        return Some(snake);
                    }

                    None
                };

                // For objects, iterate over EXPECTED keys
                for (k, exp_val) in exp_map {
                    let new_path = format!("{}.{}", path, k);

                    if let Some(resolved_key) = resolve_actual_key(k) {
                        let act_val = act_map.get(&resolved_key).expect("resolved key must exist");
                        Self::compare_recursive(act_val, exp_val, &new_path, options, results);
                    } else {
                        // Proto JSON may omit fields with default values.
                        // If expected value is a default, treat missing key as acceptable.
                        if !is_protojson_default_value(exp_val) {
                            results.push(AssertionResult::fail(format!(
                                "Key '{}' missing in actual response",
                                new_path
                            )));
                        }
                    }
                }

                // If NOT partial, check that actual doesn't have extra keys
                if !options.partial {
                    for k in act_map.keys() {
                        let snake = camel_to_snake(k);
                        let camel = snake_to_camel(k);
                        if !exp_map.contains_key(k)
                            && !exp_map.contains_key(&snake)
                            && !exp_map.contains_key(&camel)
                        {
                            results.push(AssertionResult::fail(format!(
                                "Unexpected key '{}.{}' in actual response",
                                path, k
                            )));
                        }
                    }
                }
            }
            (Value::Array(act_arr), Value::Array(exp_arr)) => {
                // Array length check
                if !options.partial && act_arr.len() != exp_arr.len() {
                    results.push(AssertionResult::fail_with_diff(
                        format!(
                            "Array length mismatch at '{}': expected {}, got {}",
                            path,
                            exp_arr.len(),
                            act_arr.len()
                        ),
                        format!("length: {}", exp_arr.len()),
                        format!("length: {}", act_arr.len()),
                    ));
                }

                // If unordered_arrays is set, we need special handling
                if options.unordered_arrays {
                    // OPTIMIZED: Hash-based O(n) comparison instead of O(n²)
                    // Strategy: Hash each item and compare hash sets
                    // For items with same hash, do deep comparison

                    let mut matched_actual_indices = std::collections::HashSet::new();
                    let mut hash_to_indices: std::collections::HashMap<u64, Vec<usize>> =
                        std::collections::HashMap::new();

                    // Build hash map for actual items
                    for (i, act_item) in act_arr.iter().enumerate() {
                        let hash = Self::hash_value(act_item);
                        hash_to_indices.entry(hash).or_default().push(i);
                    }

                    // Match expected items against actual items using hash map
                    for exp_item in exp_arr.iter() {
                        let exp_hash = Self::hash_value(exp_item);
                        let mut found = false;

                        if let Some(indices) = hash_to_indices.get_mut(&exp_hash) {
                            for &idx in indices.iter() {
                                if matched_actual_indices.contains(&idx) {
                                    continue;
                                }

                                // Verify with deep comparison
                                let mut temp_results = Vec::new();
                                Self::compare_recursive(
                                    &act_arr[idx],
                                    exp_item,
                                    &format!("{}[{}]", path, idx),
                                    options,
                                    &mut temp_results,
                                );

                                if temp_results.is_empty() {
                                    matched_actual_indices.insert(idx);
                                    found = true;
                                    break;
                                }
                            }
                        }

                        if !found && !options.partial {
                            results.push(AssertionResult::fail(format!(
                                "Missing expected item in unordered array at '{}': {:?}",
                                path, exp_item
                            )));
                        }
                    }

                    // Check for extra items in actual (if not partial)
                    if !options.partial && matched_actual_indices.len() < act_arr.len() {
                        results.push(AssertionResult::fail(format!(
                            "Unordered array at '{}' has {} extra items",
                            path,
                            act_arr.len() - matched_actual_indices.len()
                        )));
                    }

                    return;
                }

                // Iterate (up to min length)
                // If partial is true, we usually still expect the items we defined to match the *first* N items
                // OR we strictly match what we have.
                // Let's implement strict index matching for the common case.

                let len = std::cmp::min(act_arr.len(), exp_arr.len());
                for i in 0..len {
                    let new_path = format!("{}[{}]", path, i);
                    Self::compare_recursive(&act_arr[i], &exp_arr[i], &new_path, options, results);
                }

                // If expected is longer than actual, that's always a fail (missing items)
                if exp_arr.len() > act_arr.len() {
                    for i in act_arr.len()..exp_arr.len() {
                        results.push(AssertionResult::fail(format!(
                            "Missing array item at '{}[{}]'",
                            path, i
                        )));
                    }
                }
            }
            (Value::String(a), Value::String(e)) => {
                if a != e {
                    results.push(AssertionResult::fail_with_diff(
                        format!(
                            "Value mismatch at '{}': expected \"{}\", got \"{}\"",
                            path, e, a
                        ),
                        e,
                        a,
                    ));
                }
            }
            (Value::Number(a), Value::Number(e)) => {
                // Handle tolerance if provided
                if let Some(tol) = options.tolerance
                    && let (Some(af), Some(ef)) = (a.as_f64(), e.as_f64())
                {
                    if (af - ef).abs() > tol {
                        results.push(AssertionResult::fail_with_diff(
                            format!(
                                "Value mismatch at '{}': expected {} (tolerance {}), got {}",
                                path, ef, tol, af
                            ),
                            format!("{} (±{})", ef, tol),
                            format!("{}", af),
                        ));
                    }
                    return;
                }

                // Treat numerically-equal values as equal, even if JSON representation differs
                // (e.g. 60 vs 60.0).
                if let (Some(af), Some(ef)) = (a.as_f64(), e.as_f64())
                    && (af == ef || (af - ef).abs() <= 1e-6)
                {
                    return;
                }

                if a != e {
                    results.push(AssertionResult::fail_with_diff(
                        format!("Value mismatch at '{}': expected {}, got {}", path, e, a),
                        format!("{}", e),
                        format!("{}", a),
                    ));
                }
            }
            (Value::Bool(a), Value::Bool(e)) => {
                if a != e {
                    results.push(AssertionResult::fail_with_diff(
                        format!("Value mismatch at '{}': expected {}, got {}", path, e, a),
                        format!("{}", e),
                        format!("{}", a),
                    ));
                }
            }
            (Value::Null, Value::Null) => {}
            _ => {
                // Type mismatch
                results.push(AssertionResult::fail_with_diff(
                    format!(
                        "Type mismatch at '{}': expected {:?}, got {:?}",
                        path, expected, actual
                    ),
                    format!("{:?}", expected),
                    format!("{:?}", actual),
                ));
            }
        }
    }

    /// Hash a JSON value for fast comparison
    /// Uses a simple hash combining approach for efficiency
    fn hash_value(value: &Value) -> u64 {
        use std::collections::hash_map::DefaultHasher;
        use std::hash::{Hash, Hasher};

        let mut hasher = DefaultHasher::new();

        match value {
            Value::Null => 0u8.hash(&mut hasher),
            Value::Bool(b) => (1u8, b).hash(&mut hasher),
            Value::Number(n) => {
                (2u8, n.as_i64(), n.as_u64(), n.as_f64().map(|f| f.to_bits())).hash(&mut hasher)
            }
            Value::String(s) => (3u8, s).hash(&mut hasher),
            Value::Array(arr) => {
                (4u8, arr.len()).hash(&mut hasher);
                for item in arr {
                    Self::hash_value(item).hash(&mut hasher);
                }
            }
            Value::Object(obj) => {
                (5u8, obj.len()).hash(&mut hasher);
                // Sort keys for consistent hashing
                let mut keys: Vec<_> = obj.keys().collect();
                keys.sort();
                for key in keys {
                    key.hash(&mut hasher);
                    Self::hash_value(&obj[key]).hash(&mut hasher);
                }
            }
        }

        hasher.finish()
    }
}

fn snake_to_camel(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    let mut upper = false;
    for ch in input.chars() {
        if ch == '_' {
            upper = true;
            continue;
        }

        if upper {
            out.push(ch.to_ascii_uppercase());
            upper = false;
        } else {
            out.push(ch);
        }
    }
    out
}

fn camel_to_snake(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    for (i, ch) in input.chars().enumerate() {
        if ch.is_ascii_uppercase() {
            if i > 0 {
                out.push('_');
            }
            out.push(ch.to_ascii_lowercase());
        } else {
            out.push(ch);
        }
    }
    out
}

fn is_protojson_default_value(value: &Value) -> bool {
    match value {
        Value::Null => true,
        Value::Bool(b) => !*b,
        Value::Number(n) => {
            if let Some(i) = n.as_i64() {
                i == 0
            } else if let Some(u) = n.as_u64() {
                u == 0
            } else if let Some(f) = n.as_f64() {
                f == 0.0
            } else {
                false
            }
        }
        Value::String(s) => s.is_empty(),
        Value::Array(arr) => arr.is_empty(),
        Value::Object(map) => map.is_empty(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn test_compare_exact_match() {
        let actual = json!({"foo": "bar", "num": 1});
        let expected = json!({"foo": "bar", "num": 1});
        let options = InlineOptions::default();

        let results = JsonComparator::compare(&actual, &expected, &options);
        assert!(results.is_empty());
    }

    #[test]
    fn test_compare_numeric_representation_match() {
        let actual = json!({"result": 60.0});
        let expected = json!({"result": 60});
        let options = InlineOptions::default();

        let results = JsonComparator::compare(&actual, &expected, &options);
        assert!(results.is_empty());
    }

    #[test]
    fn test_compare_mismatch() {
        let actual = json!({"foo": "bar"});
        let expected = json!({"foo": "baz"});
        let options = InlineOptions::default();

        let results = JsonComparator::compare(&actual, &expected, &options);
        assert_eq!(results.len(), 1);
        if let AssertionResult::Fail { message: msg, .. } = &results[0] {
            assert!(msg.contains("Value mismatch"));
        } else {
            panic!("Expected Fail");
        }
    }

    #[test]
    fn test_compare_partial_object() {
        let actual = json!({"foo": "bar", "extra": "field"});
        let expected = json!({"foo": "bar"});

        // Without partial, this should fail (unexpected key)
        let options = InlineOptions::default();
        let results = JsonComparator::compare(&actual, &expected, &options);
        assert_eq!(results.len(), 1);

        // With partial, this should pass
        let options = InlineOptions {
            partial: true,
            ..Default::default()
        };
        let results = JsonComparator::compare(&actual, &expected, &options);
        assert!(results.is_empty());
    }

    #[test]
    fn test_wildcard() {
        let actual = json!({"id": 12345, "name": "test"});
        let expected = json!({"id": "*", "name": "test"});
        let options = InlineOptions::default();

        let results = JsonComparator::compare(&actual, &expected, &options);
        assert!(results.is_empty());
    }

    #[test]
    fn test_redact() {
        let actual = json!({"id": 12345, "secret": "hidden", "name": "test"});
        // If we redact "secret", it's removed from actual.
        // If expected doesn't have it, strict match should pass.
        let expected = json!({"id": 12345, "name": "test"});

        let options = InlineOptions {
            redact: vec!["secret".to_string()],
            ..Default::default()
        };

        let results = JsonComparator::compare(&actual, &expected, &options);
        assert!(results.is_empty());
    }

    #[test]
    fn test_tolerance() {
        let actual = json!({"val": 10.005});
        let expected = json!({"val": 10.0});

        let mut options = InlineOptions {
            tolerance: Some(0.01),
            ..Default::default()
        };

        let results = JsonComparator::compare(&actual, &expected, &options);
        assert!(results.is_empty());

        options.tolerance = Some(0.001);
        let results = JsonComparator::compare(&actual, &expected, &options);
        assert_eq!(results.len(), 1);
    }

    #[test]
    fn test_compare_empty_objects() {
        let actual = json!({});
        let expected = json!({});
        let options = InlineOptions::default();

        let results = JsonComparator::compare(&actual, &expected, &options);
        assert!(results.is_empty());
    }

    #[test]
    fn test_compare_empty_arrays() {
        let actual = json!([]);
        let expected = json!([]);
        let options = InlineOptions::default();

        let results = JsonComparator::compare(&actual, &expected, &options);
        assert!(results.is_empty());
    }

    #[test]
    fn test_compare_null_values() {
        let actual = json!({"val": null});
        let expected = json!({"val": null});
        let options = InlineOptions::default();

        let results = JsonComparator::compare(&actual, &expected, &options);
        assert!(results.is_empty());
    }

    #[test]
    fn test_compare_null_mismatch() {
        let actual = json!({"val": "not null"});
        let expected = json!({"val": null});
        let options = InlineOptions::default();

        let results = JsonComparator::compare(&actual, &expected, &options);
        assert_eq!(results.len(), 1);
    }

    #[test]
    fn test_compare_boolean_values() {
        let actual = json!({"active": true, "deleted": false});
        let expected = json!({"active": true, "deleted": false});
        let options = InlineOptions::default();

        let results = JsonComparator::compare(&actual, &expected, &options);
        assert!(results.is_empty());
    }

    #[test]
    fn test_compare_boolean_mismatch() {
        let actual = json!({"active": true});
        let expected = json!({"active": false});
        let options = InlineOptions::default();

        let results = JsonComparator::compare(&actual, &expected, &options);
        assert_eq!(results.len(), 1);
    }

    #[test]
    fn test_compare_nested_objects() {
        let actual = json!({"user": {"name": "test", "age": 25}});
        let expected = json!({"user": {"name": "test", "age": 25}});
        let options = InlineOptions::default();

        let results = JsonComparator::compare(&actual, &expected, &options);
        assert!(results.is_empty());
    }

    #[test]
    fn test_compare_nested_mismatch() {
        let actual = json!({"user": {"name": "test"}});
        let expected = json!({"user": {"name": "other"}});
        let options = InlineOptions::default();

        let results = JsonComparator::compare(&actual, &expected, &options);
        assert_eq!(results.len(), 1);
    }

    #[test]
    fn test_compare_arrays_different_lengths() {
        let actual = json!([1, 2, 3]);
        let expected = json!([1, 2]);
        let options = InlineOptions::default();

        let results = JsonComparator::compare(&actual, &expected, &options);
        assert!(results.len() > 0);
    }

    #[test]
    fn test_compare_arrays_with_objects() {
        let actual = json!([{"id": 1}, {"id": 2}]);
        let expected = json!([{"id": 1}, {"id": 2}]);
        let options = InlineOptions::default();

        let results = JsonComparator::compare(&actual, &expected, &options);
        assert!(results.is_empty());
    }

    #[test]
    fn test_compare_partial_nested_object() {
        let actual = json!({"user": {"name": "test", "age": 25, "extra": "field"}});
        let expected = json!({"user": {"name": "test"}});
        let options = InlineOptions {
            partial: true,
            ..Default::default()
        };

        let results = JsonComparator::compare(&actual, &expected, &options);
        assert!(results.is_empty());
    }

    #[test]
    fn test_redact_nested() {
        let actual = json!({"user": {"password": "secret", "name": "test"}});
        let expected = json!({"user": {"name": "test"}});

        let options = InlineOptions {
            redact: vec!["password".to_string()],
            ..Default::default()
        };

        let results = JsonComparator::compare(&actual, &expected, &options);
        assert!(results.is_empty());
    }

    #[test]
    fn test_snake_to_camel() {
        assert_eq!(snake_to_camel("user_name"), "userName");
        assert_eq!(snake_to_camel("id"), "id");
        assert_eq!(snake_to_camel("alreadyCamel"), "alreadyCamel");
    }

    #[test]
    fn test_camel_to_snake() {
        assert_eq!(camel_to_snake("userName"), "user_name");
        assert_eq!(camel_to_snake("id"), "id");
        assert_eq!(camel_to_snake("already_snake"), "already_snake");
    }

    #[test]
    fn test_is_protojson_default_value() {
        assert!(is_protojson_default_value(&Value::String("".to_string())));
        assert!(is_protojson_default_value(&Value::Number(0.into())));
        assert!(is_protojson_default_value(&Value::Bool(false)));
        assert!(is_protojson_default_value(&Value::Array(vec![])));
        assert!(is_protojson_default_value(&Value::Object(
            serde_json::Map::new()
        )));
        assert!(!is_protojson_default_value(&Value::String(
            "not empty".to_string()
        )));
        assert!(!is_protojson_default_value(&Value::Number(1.into())));
        assert!(!is_protojson_default_value(&Value::Bool(true)));
    }

    #[test]
    fn test_unordered_arrays_optimized() {
        // Test that unordered arrays work correctly with hash-based optimization
        let actual = json!([3, 1, 2]);
        let expected = json!([1, 2, 3]);
        let options = InlineOptions {
            unordered_arrays: true,
            ..Default::default()
        };

        let results = JsonComparator::compare(&actual, &expected, &options);
        assert!(results.is_empty());
    }

    #[test]
    fn test_unordered_arrays_with_objects() {
        // Test unordered arrays with complex objects
        let actual = json!([
            {"id": 3, "name": "c"},
            {"id": 1, "name": "a"},
            {"id": 2, "name": "b"}
        ]);
        let expected = json!([
            {"id": 1, "name": "a"},
            {"id": 2, "name": "b"},
            {"id": 3, "name": "c"}
        ]);
        let options = InlineOptions {
            unordered_arrays: true,
            ..Default::default()
        };

        let results = JsonComparator::compare(&actual, &expected, &options);
        assert!(results.is_empty());
    }

    #[test]
    fn test_unordered_arrays_missing_item() {
        // Test that missing items are detected
        let actual = json!([1, 2]);
        let expected = json!([1, 2, 3]);
        let options = InlineOptions {
            unordered_arrays: true,
            ..Default::default()
        };

        let results = JsonComparator::compare(&actual, &expected, &options);
        assert!(!results.is_empty());
    }

    #[test]
    fn test_unordered_arrays_extra_item() {
        // Test that extra items are detected
        let actual = json!([1, 2, 3, 4]);
        let expected = json!([1, 2, 3]);
        let options = InlineOptions {
            unordered_arrays: true,
            ..Default::default()
        };

        let results = JsonComparator::compare(&actual, &expected, &options);
        assert!(!results.is_empty());
    }

    #[test]
    fn test_unordered_arrays_partial() {
        // Test that partial matching works with unordered arrays
        let actual = json!([1, 2, 3, 4]);
        let expected = json!([1, 3]);
        let options = InlineOptions {
            unordered_arrays: true,
            partial: true,
            ..Default::default()
        };

        let results = JsonComparator::compare(&actual, &expected, &options);
        assert!(results.is_empty());
    }

    #[test]
    fn test_hash_value_consistency() {
        // Test that hash_value produces consistent results
        let value1 = json!({"id": 1, "name": "test"});
        let value2 = json!({"id": 1, "name": "test"});
        let value3 = json!({"name": "test", "id": 1}); // Different key order

        // Hash should be the same for equal values (including different key order)
        let hash1 = JsonComparator::hash_value(&value1);
        let hash2 = JsonComparator::hash_value(&value2);
        let hash3 = JsonComparator::hash_value(&value3);

        assert_eq!(hash1, hash2);
        assert_eq!(hash2, hash3);
    }
}
