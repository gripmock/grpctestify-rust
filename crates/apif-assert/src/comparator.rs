use super::engine::AssertionResult;
use apif_ast::ast::InlineOptions;
use serde_json::Value;

pub struct JsonComparator;

impl JsonComparator {
    pub fn compare(
        actual: &Value,
        expected: &Value,
        options: &InlineOptions,
    ) -> Vec<AssertionResult> {
        let mut results = Vec::new();

        if options.redact.is_empty() {
            Self::compare_recursive(actual, expected, "$", options, &mut results);
        } else {
            let mut actual_redacted = actual.clone();
            Self::redact_value(&mut actual_redacted, &options.redact);
            Self::compare_recursive(&actual_redacted, expected, "$", options, &mut results);
        }

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
        if let Value::String(s) = expected
            && s == "*"
        {
            return;
        }

        match (actual, expected) {
            (Value::Object(act_map), Value::Object(exp_map)) => {
                for (k, exp_val) in exp_map {
                    let new_path = format!("{}.{}", path, k);

                    if let Some(act_val) = act_map.get(k) {
                        Self::compare_recursive(act_val, exp_val, &new_path, options, results);
                    } else {
                        if !is_protojson_default_value(exp_val) {
                            results.push(AssertionResult::fail(format!(
                                "Key '{}' missing in actual response",
                                new_path
                            )));
                        }
                    }
                }

                if !options.partial {
                    for k in act_map.keys() {
                        if !exp_map.contains_key(k) {
                            results.push(AssertionResult::fail(format!(
                                "Unexpected key '{}.{}' in actual response",
                                path, k
                            )));
                        }
                    }
                }
            }
            (Value::Array(act_arr), Value::Array(exp_arr)) => {
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

                if options.unordered_arrays {
                    let mut matched_actual_indices = std::collections::HashSet::new();
                    let mut hash_to_indices: std::collections::HashMap<u64, Vec<usize>> =
                        std::collections::HashMap::new();

                    for (i, act_item) in act_arr.iter().enumerate() {
                        let hash = Self::hash_value(act_item);
                        hash_to_indices.entry(hash).or_default().push(i);
                    }

                    for exp_item in exp_arr {
                        let exp_hash = Self::hash_value(exp_item);
                        let mut found = false;

                        if let Some(indices) = hash_to_indices.get(&exp_hash) {
                            for &idx in indices.iter() {
                                if matched_actual_indices.contains(&idx) {
                                    continue;
                                }

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

                        if !found {
                            for (idx, act_item) in act_arr.iter().enumerate() {
                                if matched_actual_indices.contains(&idx) {
                                    continue;
                                }

                                let mut temp_results = Vec::new();
                                Self::compare_recursive(
                                    act_item,
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

                        if !found {
                            results.push(AssertionResult::fail(format!(
                                "Missing expected item in unordered array at '{}': {:?}",
                                path, exp_item
                            )));
                        }
                    }

                    if !options.partial && matched_actual_indices.len() < act_arr.len() {
                        results.push(AssertionResult::fail(format!(
                            "Unordered array at '{}' has {} extra items",
                            path,
                            act_arr.len() - matched_actual_indices.len()
                        )));
                    }

                    return;
                }

                let len = std::cmp::min(act_arr.len(), exp_arr.len());
                for i in 0..len {
                    let new_path = format!("{}[{}]", path, i);
                    Self::compare_recursive(&act_arr[i], &exp_arr[i], &new_path, options, results);
                }

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

                let a_is_int = a.is_i64() || a.is_u64();
                let e_is_int = e.is_i64() || e.is_u64();
                if a_is_int && e_is_int {
                    let equal = if let (Some(ai), Some(ei)) = (a.as_i64(), e.as_i64()) {
                        ai == ei
                    } else if let (Some(au), Some(eu)) = (a.as_u64(), e.as_u64()) {
                        au == eu
                    } else {
                        false
                    };
                    if equal {
                        return;
                    }
                } else if let (Some(af), Some(ef)) = (a.as_f64(), e.as_f64()) {
                    let scale = af.abs().max(ef.abs());
                    if af == ef || (af - ef).abs() <= 1e-9 * scale {
                        return;
                    }
                }

                results.push(AssertionResult::fail_with_diff(
                    format!("Value mismatch at '{}': expected {}, got {}", path, e, a),
                    format!("{}", e),
                    format!("{}", a),
                ));
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
    fn compare_exact_match() {
        let actual = json!({"foo": "bar", "num": 1});
        let expected = json!({"foo": "bar", "num": 1});
        let options = InlineOptions::default();

        let results = JsonComparator::compare(&actual, &expected, &options);
        assert!(results.is_empty());
    }

    #[test]
    fn compare_numeric_representation_match() {
        let actual = json!({"result": 60.0});
        let expected = json!({"result": 60});
        let options = InlineOptions::default();

        let results = JsonComparator::compare(&actual, &expected, &options);
        assert!(results.is_empty());
    }

    #[test]
    fn compare_mismatch() {
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
    fn compare_partial_object() {
        let actual = json!({"foo": "bar", "extra": "field"});
        let expected = json!({"foo": "bar"});

        let options = InlineOptions::default();
        let results = JsonComparator::compare(&actual, &expected, &options);
        assert_eq!(results.len(), 1);

        let options = InlineOptions {
            partial: true,
            ..Default::default()
        };
        let results = JsonComparator::compare(&actual, &expected, &options);
        assert!(results.is_empty());
    }

    #[test]
    fn wildcard() {
        let actual = json!({"id": 12345, "name": "test"});
        let expected = json!({"id": "*", "name": "test"});
        let options = InlineOptions::default();

        let results = JsonComparator::compare(&actual, &expected, &options);
        assert!(results.is_empty());
    }

    #[test]
    fn redact() {
        let actual = json!({"id": 12345, "secret": "hidden", "name": "test"});
        let expected = json!({"id": 12345, "name": "test"});

        let options = InlineOptions {
            redact: vec!["secret".to_string()],
            ..Default::default()
        };

        let results = JsonComparator::compare(&actual, &expected, &options);
        assert!(results.is_empty());
    }

    #[test]
    fn tolerance() {
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
    fn compare_empty_objects() {
        let actual = json!({});
        let expected = json!({});
        let options = InlineOptions::default();

        let results = JsonComparator::compare(&actual, &expected, &options);
        assert!(results.is_empty());
    }

    #[test]
    fn compare_empty_arrays() {
        let actual = json!([]);
        let expected = json!([]);
        let options = InlineOptions::default();

        let results = JsonComparator::compare(&actual, &expected, &options);
        assert!(results.is_empty());
    }

    #[test]
    fn compare_null_values() {
        let actual = json!({"val": null});
        let expected = json!({"val": null});
        let options = InlineOptions::default();

        let results = JsonComparator::compare(&actual, &expected, &options);
        assert!(results.is_empty());
    }

    #[test]
    fn compare_null_mismatch() {
        let actual = json!({"val": "not null"});
        let expected = json!({"val": null});
        let options = InlineOptions::default();

        let results = JsonComparator::compare(&actual, &expected, &options);
        assert_eq!(results.len(), 1);
    }

    #[test]
    fn compare_boolean_values() {
        let actual = json!({"active": true, "deleted": false});
        let expected = json!({"active": true, "deleted": false});
        let options = InlineOptions::default();

        let results = JsonComparator::compare(&actual, &expected, &options);
        assert!(results.is_empty());
    }

    #[test]
    fn compare_boolean_mismatch() {
        let actual = json!({"active": true});
        let expected = json!({"active": false});
        let options = InlineOptions::default();

        let results = JsonComparator::compare(&actual, &expected, &options);
        assert_eq!(results.len(), 1);
    }

    #[test]
    fn compare_nested_objects() {
        let actual = json!({"user": {"name": "test", "age": 25}});
        let expected = json!({"user": {"name": "test", "age": 25}});
        let options = InlineOptions::default();

        let results = JsonComparator::compare(&actual, &expected, &options);
        assert!(results.is_empty());
    }

    #[test]
    fn compare_nested_mismatch() {
        let actual = json!({"user": {"name": "test"}});
        let expected = json!({"user": {"name": "other"}});
        let options = InlineOptions::default();

        let results = JsonComparator::compare(&actual, &expected, &options);
        assert_eq!(results.len(), 1);
    }

    #[test]
    fn compare_arrays_different_lengths() {
        let actual = json!([1, 2, 3]);
        let expected = json!([1, 2]);
        let options = InlineOptions::default();

        let results = JsonComparator::compare(&actual, &expected, &options);
        assert!(!results.is_empty());
    }

    #[test]
    fn compare_arrays_with_objects() {
        let actual = json!([{"id": 1}, {"id": 2}]);
        let expected = json!([{"id": 1}, {"id": 2}]);
        let options = InlineOptions::default();

        let results = JsonComparator::compare(&actual, &expected, &options);
        assert!(results.is_empty());
    }

    #[test]
    fn compare_partial_nested_object() {
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
    fn redact_nested() {
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
    fn unordered_arrays_optimized() {
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
    fn unordered_arrays_with_objects() {
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
    fn unordered_arrays_missing_item() {
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
    fn unordered_arrays_extra_item() {
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
    fn unordered_arrays_partial() {
        let actual = json!([1, 2, 3, 4]);
        let expected = json!([1, 3]);
        let options = InlineOptions {
            unordered_arrays: true,
            partial: true,
            ..Default::default()
        };

        let results = JsonComparator::compare(&actual, &expected, &options);
        assert!(results.is_empty());

        let expected = json!([1, 999]);
        let results = JsonComparator::compare(&actual, &expected, &options);
        assert!(
            !results.is_empty(),
            "missing expected item must fail even in partial mode"
        );
    }

    #[test]
    fn unordered_arrays_partial_missing_item_fails() {
        let actual = json!([1, 2, 3]);
        let expected = json!([999]);
        let options = InlineOptions {
            unordered_arrays: true,
            partial: true,
            ..Default::default()
        };

        let results = JsonComparator::compare(&actual, &expected, &options);
        assert_eq!(results.len(), 1);
        if let AssertionResult::Fail { message, .. } = &results[0] {
            assert!(message.contains("Missing expected item"));
        } else {
            panic!("Expected Fail");
        }
    }

    #[test]
    fn unordered_arrays_numeric_representation() {
        let actual = json!([60.0]);
        let expected = json!([60]);
        let options = InlineOptions {
            unordered_arrays: true,
            ..Default::default()
        };

        let results = JsonComparator::compare(&actual, &expected, &options);
        assert!(results.is_empty(), "got: {:?}", results);
    }

    #[test]
    fn unordered_arrays_wildcard() {
        let actual = json!(["abc", 1]);
        let expected = json!([1, "*"]);
        let options = InlineOptions {
            unordered_arrays: true,
            ..Default::default()
        };

        let results = JsonComparator::compare(&actual, &expected, &options);
        assert!(results.is_empty(), "got: {:?}", results);
    }

    #[test]
    fn unordered_arrays_tolerance() {
        let actual = json!([10.005, 20.0]);
        let expected = json!([20.0, 10.0]);
        let options = InlineOptions {
            unordered_arrays: true,
            tolerance: Some(0.01),
            ..Default::default()
        };

        let results = JsonComparator::compare(&actual, &expected, &options);
        assert!(results.is_empty(), "got: {:?}", results);
    }

    #[test]
    fn unordered_arrays_partial_objects() {
        let actual = json!([{"id": 2, "extra": "y"}, {"id": 1, "extra": "x"}]);
        let expected = json!([{"id": 1}, {"id": 2}]);
        let options = InlineOptions {
            unordered_arrays: true,
            partial: true,
            ..Default::default()
        };

        let results = JsonComparator::compare(&actual, &expected, &options);
        assert!(results.is_empty(), "got: {:?}", results);
    }

    #[test]
    fn compare_large_integers_exact() {
        let actual = json!({"id": 9223372036854775807i64});
        let expected = json!({"id": 9223372036854775806i64});
        let options = InlineOptions::default();

        let results = JsonComparator::compare(&actual, &expected, &options);
        assert_eq!(results.len(), 1);

        let expected = json!({"id": 9223372036854775807i64});
        let results = JsonComparator::compare(&actual, &expected, &options);
        assert!(results.is_empty());
    }

    #[test]
    fn compare_small_floats_not_equal() {
        let actual = json!({"val": 0.0000001});
        let expected = json!({"val": 0.0000009});
        let options = InlineOptions::default();

        let results = JsonComparator::compare(&actual, &expected, &options);
        assert_eq!(results.len(), 1);
    }

    #[test]
    fn compare_float_rounding_noise_equal() {
        let actual = json!({"val": 0.1 + 0.2});
        let expected = json!({"val": 0.3});
        let options = InlineOptions::default();

        let results = JsonComparator::compare(&actual, &expected, &options);
        assert!(results.is_empty(), "got: {:?}", results);
    }

    #[test]
    fn compare_mixed_sign_large_integers() {
        let actual = json!({"val": -1i64});
        let expected = json!({"val": 18446744073709551615u64});
        let options = InlineOptions::default();

        let results = JsonComparator::compare(&actual, &expected, &options);
        assert_eq!(results.len(), 1);
    }

    #[test]
    fn hash_value_consistency() {
        let value1 = json!({"id": 1, "name": "test"});
        let value2 = json!({"id": 1, "name": "test"});
        let value3 = json!({"name": "test", "id": 1});

        let hash1 = JsonComparator::hash_value(&value1);
        let hash2 = JsonComparator::hash_value(&value2);
        let hash3 = JsonComparator::hash_value(&value3);

        assert_eq!(hash1, hash2);
        assert_eq!(hash2, hash3);
    }
}
