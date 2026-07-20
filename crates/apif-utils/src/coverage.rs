//! gRPC method and protobuf message field coverage collector.
//!
//! Tracks which gRPC service/method calls were made during test execution
//! and which protobuf message fields were covered by assertions.

use prost_reflect::{DescriptorPool, MessageDescriptor};
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::sync::{Arc, Mutex};

/// Coverage data for a single file.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CoverageFile {
    pub uri: String,
    pub statements: CoverageStats,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub branches: Option<CoverageStats>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub functions: Option<CoverageStats>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub fields: Option<CoverageStats>,
}

/// Coverage statistics (covered vs total).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CoverageStats {
    pub covered: usize,
    pub total: usize,
}

/// Coverage data for a protobuf message type's fields.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MessageFieldCoverage {
    pub message_type: String,
    pub covered_fields: Vec<String>,
    pub total_fields: usize,
}

/// Full coverage report with file and message-level statistics.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CoverageReport {
    pub files: Vec<CoverageFile>,
    pub messages: Vec<MessageFieldCoverage>,
    pub summary: CoverageStats,
    pub field_summary: CoverageStats,
}

/// Collects gRPC method call and protobuf field coverage during test execution.
#[derive(Debug, Clone)]
pub struct CoverageCollector {
    calls: Arc<Mutex<HashMap<String, HashMap<String, u64>>>>,
    pool: Arc<Mutex<DescriptorPool>>,
    fields_covered: Arc<Mutex<HashMap<String, HashSet<String>>>>,
}

impl CoverageCollector {
    pub fn new() -> Self {
        Self {
            calls: Arc::new(Mutex::new(HashMap::new())),
            pool: Arc::new(Mutex::new(DescriptorPool::new())),
            fields_covered: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    pub fn record_call(&self, service: &str, method: &str) {
        let mut calls = self.calls.lock().unwrap_or_else(|e| e.into_inner());
        let service_calls = calls.entry(service.to_string()).or_default();
        *service_calls.entry(method.to_string()).or_insert(0) += 1;
    }

    pub fn record_fields_from_json(&self, message_type: &str, json: &serde_json::Value) {
        let mut fields = self
            .fields_covered
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        let message_fields = fields.entry(message_type.to_string()).or_default();
        Self::extract_fields_from_json(json, message_fields, "");
    }

    fn extract_fields_from_json(
        json: &serde_json::Value,
        fields: &mut HashSet<String>,
        prefix: &str,
    ) {
        if let serde_json::Value::Object(map) = json {
            for (key, value) in map {
                let field_path = if prefix.is_empty() {
                    key.clone()
                } else {
                    format!("{}.{}", prefix, key)
                };
                fields.insert(field_path.clone());
                Self::extract_fields_from_json(value, fields, &field_path);
            }
        } else if let serde_json::Value::Array(arr) = json {
            for item in arr {
                Self::extract_fields_from_json(item, fields, prefix);
            }
        }
    }

    pub fn register_pool(&self, other: &DescriptorPool) {
        let mut pool = self.pool.lock().unwrap_or_else(|e| e.into_inner());
        for file in other.files() {
            let _ = pool.add_file_descriptor_proto(file.file_descriptor_proto().clone());
        }
    }

    fn count_message_fields(pool: &DescriptorPool, message_type: &str) -> usize {
        if let Some(msg) = pool.get_message_by_name(message_type) {
            Self::count_fields_recursive(&msg)
        } else {
            0
        }
    }

    /// Count fields the way `extract_fields_from_json` records covered ones:
    /// every field contributes its own dotted path, and message-typed fields
    /// additionally contribute the nested paths of their sub-message. Without
    /// this the denominator only counted top-level fields while the numerator
    /// counted nested `parent.child` paths, understating the total.
    fn count_fields_recursive(msg: &MessageDescriptor) -> usize {
        fn count(msg: &MessageDescriptor, visited: &mut HashSet<String>) -> usize {
            // Guard against recursive message types (e.g. a tree node whose
            // field points back at its own type) to avoid unbounded recursion.
            if !visited.insert(msg.full_name().to_string()) {
                return 0;
            }
            let mut total = 0;
            for field in msg.fields() {
                total += 1;
                // Recurse into nested messages, but not map entries: a map's
                // keys are dynamic and can't be enumerated from the schema.
                if !field.is_map()
                    && let prost_reflect::Kind::Message(sub) = field.kind()
                {
                    total += count(&sub, visited);
                }
            }
            visited.remove(msg.full_name());
            total
        }
        count(msg, &mut HashSet::new())
    }

    pub fn generate_json_report(&self) -> CoverageReport {
        let calls = self.calls.lock().unwrap_or_else(|e| e.into_inner());
        let pool = self.pool.lock().unwrap_or_else(|e| e.into_inner());
        let fields_covered = self
            .fields_covered
            .lock()
            .unwrap_or_else(|e| e.into_inner());

        let mut files = Vec::new();
        let mut messages = Vec::new();
        let mut total_covered = 0;
        let mut total_methods = 0;
        let mut total_fields_covered = 0;
        let mut total_fields = 0;

        // Method coverage - deduplicated iteration pattern
        let mut services: Vec<_> = pool.services().collect();
        services.sort_by(|a, b| a.name().cmp(b.name()));

        for service in services {
            let service_name = service.name();
            if service_name.contains("reflection") {
                continue;
            }

            let methods: Vec<_> = service.methods().collect();
            // Calls are recorded under the fully-qualified service name
            // ("package.Service", see runner.rs). Look up with the same
            // FQN so services inside a proto `package` aren't reported 0%.
            let called_methods = calls.get(service.full_name()).cloned().unwrap_or_default();

            let covered = methods
                .iter()
                .filter(|m| called_methods.get(m.name()).unwrap_or(&0) > &0)
                .count();
            let total = methods.len();

            if total > 0 {
                total_covered += covered;
                total_methods += total;

                files.push(CoverageFile {
                    uri: format!("grpc://{}", service_name),
                    statements: CoverageStats { covered, total },
                    branches: None,
                    functions: Some(CoverageStats { covered, total }),
                    fields: None,
                });
            }
        }

        // Message field coverage
        let mut all_message_types: HashSet<String> = HashSet::new();
        for message_type in fields_covered.keys() {
            all_message_types.insert(message_type.clone());
        }

        let mut sorted_messages: Vec<_> = all_message_types.into_iter().collect();
        sorted_messages.sort();

        for message_type in sorted_messages {
            let covered = fields_covered
                .get(&message_type)
                .map(|s| s.len())
                .unwrap_or(0);
            let total = Self::count_message_fields(&pool, &message_type);

            if total > 0 {
                total_fields_covered += covered.min(total);
                total_fields += total;

                let covered_fields: Vec<String> = fields_covered
                    .get(&message_type)
                    .map(|s| {
                        let mut v: Vec<_> = s.iter().cloned().collect();
                        v.sort();
                        v
                    })
                    .unwrap_or_default();

                messages.push(MessageFieldCoverage {
                    message_type,
                    covered_fields,
                    total_fields: total,
                });
            }
        }

        CoverageReport {
            files,
            messages,
            summary: CoverageStats {
                covered: total_covered,
                total: total_methods,
            },
            field_summary: CoverageStats {
                covered: total_fields_covered,
                total: total_fields,
            },
        }
    }

    pub fn generate_text_report(&self) -> String {
        let calls = self.calls.lock().unwrap_or_else(|e| e.into_inner());
        let pool = self.pool.lock().unwrap_or_else(|e| e.into_inner());
        let fields_covered = self
            .fields_covered
            .lock()
            .unwrap_or_else(|e| e.into_inner());

        let mut report = String::new();
        report.push_str("--- gRPC API Coverage Report ---\n\n");

        // Method coverage
        let mut services: Vec<_> = pool.services().collect();
        services.sort_by(|a, b| a.name().cmp(b.name()));

        if services.is_empty() {
            report.push_str("No services found in descriptors.\n");
            return report;
        }

        for service in services {
            let service_name = service.name();
            if service_name == "grpc.reflection.v1alpha.ServerReflection"
                || service_name == "grpc.reflection.v1.ServerReflection"
            {
                continue;
            }

            report.push_str(&format!("Service: {}\n", service_name));

            // Match the fully-qualified name used when recording calls.
            let called_methods = calls.get(service.full_name()).cloned().unwrap_or_default();

            let mut methods: Vec<_> = service.methods().collect();
            methods.sort_by(|a, b| a.name().cmp(b.name()));

            let mut covered_count = 0;
            let total_count = methods.len();

            for method in methods {
                let method_name = method.name();
                let count = called_methods.get(method_name).unwrap_or(&0);

                let status = if *count > 0 {
                    covered_count += 1;
                    format!("✅ ({} calls)", count)
                } else {
                    "❌ (0 calls)".to_string()
                };

                report.push_str(&format!("  - {}: {}\n", method_name, status));
            }

            let coverage_pct = if total_count > 0 {
                (covered_count as f64 / total_count as f64) * 100.0
            } else {
                0.0
            };

            report.push_str(&format!(
                "  Coverage: {:.1}% ({}/{})\n\n",
                coverage_pct, covered_count, total_count
            ));
        }

        // Message field coverage
        if !fields_covered.is_empty() {
            report.push_str("--- Message Field Coverage ---\n\n");

            let mut message_types: Vec<_> = fields_covered.keys().cloned().collect();
            message_types.sort();

            for message_type in message_types {
                let covered = fields_covered
                    .get(&message_type)
                    .map(|s| s.len())
                    .unwrap_or(0);
                let total = Self::count_message_fields(&pool, &message_type);

                if total > 0 {
                    let pct = (covered.min(total) as f64 / total as f64) * 100.0;
                    let status = if pct >= 100.0 {
                        "✅"
                    } else if pct > 0.0 {
                        "⚠️"
                    } else {
                        "❌"
                    };
                    report.push_str(&format!(
                        "{} {} ({}/{})\n",
                        status,
                        message_type,
                        covered.min(total),
                        total
                    ));
                }
            }
        }

        report
    }
}

impl Default for CoverageCollector {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use prost_reflect::prost_types::{
        DescriptorProto, FileDescriptorProto, MethodDescriptorProto, ServiceDescriptorProto,
    };

    /// Build a pool with a service inside a proto `package`, so its
    /// fully-qualified name ("my.pkg.Greeter") differs from its short name.
    fn pool_with_packaged_service() -> DescriptorPool {
        let mut pool = DescriptorPool::new();
        let file = FileDescriptorProto {
            name: Some("test.proto".to_string()),
            package: Some("my.pkg".to_string()),
            message_type: vec![DescriptorProto {
                name: Some("Empty".to_string()),
                ..Default::default()
            }],
            service: vec![ServiceDescriptorProto {
                name: Some("Greeter".to_string()),
                method: vec![MethodDescriptorProto {
                    name: Some("SayHello".to_string()),
                    input_type: Some(".my.pkg.Empty".to_string()),
                    output_type: Some(".my.pkg.Empty".to_string()),
                    ..Default::default()
                }],
                ..Default::default()
            }],
            ..Default::default()
        };
        pool.add_file_descriptor_proto(file).unwrap();
        pool
    }

    // Bug 6: calls are recorded under the fully-qualified service name, so
    // coverage lookup must use the same FQN or packaged services report 0%.
    #[test]
    fn coverage_matches_fully_qualified_service_name() {
        let collector = CoverageCollector::new();
        collector.register_pool(&pool_with_packaged_service());
        // Recorded exactly as runner.rs does: "package.Service".
        collector.record_call("my.pkg.Greeter", "SayHello");

        let report = collector.generate_json_report();
        assert_eq!(report.summary.total, 1, "one method total");
        assert_eq!(
            report.summary.covered, 1,
            "packaged service call should be counted as covered"
        );

        let text = collector.generate_text_report();
        assert!(text.contains("100.0%"), "text report: {text}");
    }

    /// Build a pool with a message that nests two levels of sub-messages:
    /// `Outer { id, inner: Inner }`, `Inner { name, addr: Addr }`,
    /// `Addr { city }`. Recursively that is 5 fields (id, inner, inner.name,
    /// inner.addr, inner.addr.city).
    fn pool_with_nested_message() -> DescriptorPool {
        use prost_reflect::prost_types::FieldDescriptorProto;
        use prost_reflect::prost_types::field_descriptor_proto::{Label, Type};

        let field =
            |name: &str, number: i32, ty: Type, type_name: Option<&str>| FieldDescriptorProto {
                name: Some(name.to_string()),
                number: Some(number),
                label: Some(Label::Optional as i32),
                r#type: Some(ty as i32),
                type_name: type_name.map(|s| s.to_string()),
                ..Default::default()
            };

        let mut pool = DescriptorPool::new();
        let file = FileDescriptorProto {
            name: Some("nested.proto".to_string()),
            message_type: vec![
                DescriptorProto {
                    name: Some("Outer".to_string()),
                    field: vec![
                        field("id", 1, Type::String, None),
                        field("inner", 2, Type::Message, Some(".Inner")),
                    ],
                    ..Default::default()
                },
                DescriptorProto {
                    name: Some("Inner".to_string()),
                    field: vec![
                        field("name", 1, Type::String, None),
                        field("addr", 2, Type::Message, Some(".Addr")),
                    ],
                    ..Default::default()
                },
                DescriptorProto {
                    name: Some("Addr".to_string()),
                    field: vec![field("city", 1, Type::String, None)],
                    ..Default::default()
                },
            ],
            ..Default::default()
        };
        pool.add_file_descriptor_proto(file).unwrap();
        pool
    }

    // Bug 2: the field-count denominator must recurse into nested messages so
    // it matches the nested dotted paths counted as covered.
    #[test]
    fn nested_message_field_count_is_recursive() {
        let pool = pool_with_nested_message();
        let outer = pool.get_message_by_name("Outer").unwrap();
        // Before the fix this returned 2 (only id, inner).
        assert_eq!(CoverageCollector::count_fields_recursive(&outer), 5);
    }

    #[test]
    fn nested_message_full_coverage_is_100_percent() {
        let collector = CoverageCollector::new();
        collector.register_pool(&pool_with_nested_message());
        collector.record_fields_from_json(
            "Outer",
            &serde_json::json!({
                "id": "x",
                "inner": { "name": "y", "addr": { "city": "z" } }
            }),
        );

        let report = collector.generate_json_report();
        assert_eq!(report.field_summary.total, 5, "recursive field total");
        assert_eq!(report.field_summary.covered, 5, "all nested fields covered");
        let msg = report
            .messages
            .iter()
            .find(|m| m.message_type == "Outer")
            .unwrap();
        assert_eq!(msg.total_fields, 5);
        assert_eq!(msg.covered_fields.len(), 5);
    }

    #[test]
    fn nested_message_partial_coverage_uses_recursive_total() {
        let collector = CoverageCollector::new();
        collector.register_pool(&pool_with_nested_message());
        // Only 3 of the 5 recursive paths are exercised (id, inner, inner.name).
        collector.record_fields_from_json(
            "Outer",
            &serde_json::json!({ "id": "x", "inner": { "name": "y" } }),
        );

        let report = collector.generate_json_report();
        assert_eq!(report.field_summary.total, 5);
        assert_eq!(report.field_summary.covered, 3);
    }
}
