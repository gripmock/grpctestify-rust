//! Helper utilities for the test runner.
//!
//! Contains pure functions and static helpers used by the test runner
//! that don't require `self` access: variable substitution, TLS defaults,
//! JSON formatting, and metadata conversion.

use crate::polyfill::runtime;
use crate::utils::file::FileUtils;
use serde_json::Value;
use std::collections::HashMap;
use std::path::Path;

/// Buffer size for the request message channel.
/// Controls back-pressure for client streaming: larger values allow more
/// buffered requests but consume more memory.
pub const REQUEST_CHANNEL_BUFFER: usize = 100;

/// Default TLS configuration from environment variables.
pub fn tls_env_defaults() -> HashMap<String, String> {
    let mut defaults = HashMap::new();

    if let Ok(value) = std::env::var(crate::config::ENV_GRPCTESTIFY_TLS_CA_FILE)
        && !value.trim().is_empty()
    {
        defaults.insert("ca_cert".to_string(), value);
    }
    if let Ok(value) = std::env::var(crate::config::ENV_GRPCTESTIFY_TLS_CERT_FILE)
        && !value.trim().is_empty()
    {
        defaults.insert("client_cert".to_string(), value);
    }
    if let Ok(value) = std::env::var(crate::config::ENV_GRPCTESTIFY_TLS_KEY_FILE)
        && !value.trim().is_empty()
    {
        defaults.insert("client_key".to_string(), value);
    }
    if let Ok(value) = std::env::var(crate::config::ENV_GRPCTESTIFY_TLS_SERVER_NAME)
        && !value.trim().is_empty()
    {
        defaults.insert("server_name".to_string(), value);
    }

    defaults
}

/// Resolve a TLS file path relative to document or CWD.
pub fn resolve_tls_path(value: &str, from_env: bool, document_path: &Path) -> String {
    let path = Path::new(value);
    if path.is_absolute() {
        return path.to_string_lossy().to_string();
    }

    if from_env {
        if runtime::supports(runtime::Capability::IsolatedFsIo)
            && let Ok(cwd) = std::env::current_dir()
        {
            return cwd.join(path).to_string_lossy().to_string();
        }
        return path.to_string_lossy().to_string();
    }

    FileUtils::resolve_relative_path(document_path, value)
        .to_string_lossy()
        .to_string()
}

/// Build full service name from package and service.
pub fn full_service_name(package: &str, service: &str) -> String {
    if package.is_empty() {
        service.to_string()
    } else {
        format!("{}.{}", package, service)
    }
}

/// Format JSON value for display.
pub fn format_json_pretty(value: &Value) -> String {
    serde_json::to_string_pretty(value).unwrap_or_else(|_| value.to_string())
}

/// Interpolate variables in a string template.
/// Replaces `{{var}}` patterns with values from the variables map.
/// Returns `None` if no substitutions were made.
pub fn interpolate_variables(template: &str, variables: &HashMap<String, Value>) -> Option<String> {
    let mut out = String::with_capacity(template.len());
    let mut cursor = 0usize;
    let mut changed = false;

    while let Some(open_rel) = template[cursor..].find("{{") {
        let open = cursor + open_rel;
        out.push_str(&template[cursor..open]);

        let after_open = open + 2;
        if let Some(close_rel) = template[after_open..].find("}}") {
            let close = after_open + close_rel;
            let var_name = template[after_open..close].trim();

            if let Some(var_value) = variables.get(var_name) {
                if let Value::String(s) = var_value {
                    out.push_str(s);
                } else {
                    out.push_str(&var_value.to_string());
                }
                changed = true;
            } else {
                out.push_str(&template[open..close + 2]);
            }
            cursor = close + 2;
        } else {
            out.push_str(&template[cursor..]);
            break;
        }
    }

    if cursor < template.len() {
        out.push_str(&template[cursor..]);
    }

    if changed { Some(out) } else { None }
}

/// Recursively substitute variables in a JSON value.
/// If a string is exactly `{{var}}`, it's replaced with the actual Value type.
/// Otherwise, string interpolation is performed.
pub fn substitute_variables(value: &mut Value, variables: &HashMap<String, Value>) {
    match value {
        Value::String(s) => {
            let original = s.clone();
            if s.starts_with("{{") && s.ends_with("}}") {
                let inner = s[2..s.len() - 2].trim();
                if !inner.contains("{{")
                    && let Some(val) = variables.get(inner)
                {
                    *value = val.clone();
                    return;
                }
            }
            if let Some(replaced) = interpolate_variables(s, variables) {
                *s = replaced;
            }
            // If nothing changed, restore original (type-preserving)
            if *s == original {
                // No change
            }
        }
        Value::Array(items) => {
            for item in items {
                substitute_variables(item, variables);
            }
        }
        Value::Object(map) => {
            for (_, val) in map.iter_mut() {
                substitute_variables(val, variables);
            }
        }
        _ => {}
    }
}

/// Convert tonic metadata map to HashMap.
pub fn metadata_map_to_hashmap(metadata: &tonic::metadata::MetadataMap) -> HashMap<String, String> {
    let mut out = HashMap::new();
    for kv in metadata.iter() {
        if let tonic::metadata::KeyAndValueRef::Ascii(key, value) = kv {
            if let Ok(v) = value.to_str() {
                out.insert(key.to_string(), v.to_string());
            }
        } else if let tonic::metadata::KeyAndValueRef::Binary(key, value) = kv {
            out.insert(key.to_string(), format!("{:?}", value));
        }
    }
    out
}
