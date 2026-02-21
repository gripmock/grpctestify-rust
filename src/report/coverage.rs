use prost_reflect::{DescriptorPool, MessageDescriptor};
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::sync::{Arc, Mutex};

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

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CoverageStats {
    pub covered: usize,
    pub total: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MessageFieldCoverage {
    pub message_type: String,
    pub covered_fields: Vec<String>,
    pub total_fields: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CoverageReport {
    pub files: Vec<CoverageFile>,
    pub messages: Vec<MessageFieldCoverage>,
    pub summary: CoverageStats,
    pub field_summary: CoverageStats,
}

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
        let mut calls = self.calls.lock().unwrap();
        let service_calls = calls.entry(service.to_string()).or_default();
        *service_calls.entry(method.to_string()).or_insert(0) += 1;
    }

    pub fn record_fields_from_json(&self, message_type: &str, json: &serde_json::Value) {
        let mut fields = self.fields_covered.lock().unwrap();
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
        let mut pool = self.pool.lock().unwrap();
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

    fn count_fields_recursive(msg: &MessageDescriptor) -> usize {
        msg.fields().count()
    }

    pub fn generate_json_report(&self) -> CoverageReport {
        let calls = self.calls.lock().unwrap();
        let pool = self.pool.lock().unwrap();
        let fields_covered = self.fields_covered.lock().unwrap();

        let mut files = Vec::new();
        let mut messages = Vec::new();
        let mut total_covered = 0;
        let mut total_methods = 0;
        let mut total_fields_covered = 0;
        let mut total_fields = 0;

        // Method coverage
        let mut services: Vec<_> = pool.services().collect();
        services.sort_by(|a, b| a.name().cmp(b.name()));

        for service in services {
            let service_name = service.name();
            if service_name.contains("reflection") {
                continue;
            }

            let methods: Vec<_> = service.methods().collect();
            let called_methods = calls.get(service_name).cloned().unwrap_or_default();

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
        let calls = self.calls.lock().unwrap();
        let pool = self.pool.lock().unwrap();
        let fields_covered = self.fields_covered.lock().unwrap();

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

            let called_methods = calls.get(service_name).cloned().unwrap_or_default();

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
