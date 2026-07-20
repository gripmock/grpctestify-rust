//! Helper utilities for the test runner.
//!
//! Contains pure functions and static helpers used by the test runner
//! that don't require `self` access: variable substitution, TLS defaults,
//! JSON formatting, and metadata conversion.

use apif_ast::GctfDocument;
use apif_cfg_runtime as runtime;
use apif_grpc_transport::{
    CompressionMode, ProtoConfig, TlsConfig, WireProtocol, default_address_for,
};
use apif_utils::FileUtils;
use serde::{Deserialize, Serialize};
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

    if let Ok(value) = std::env::var("GRPCTESTIFY_TLS_CA_FILE")
        && !value.trim().is_empty()
    {
        defaults.insert("ca_cert".to_string(), value);
    }
    if let Ok(value) = std::env::var("GRPCTESTIFY_TLS_CERT_FILE")
        && !value.trim().is_empty()
    {
        defaults.insert("client_cert".to_string(), value);
    }
    if let Ok(value) = std::env::var("GRPCTESTIFY_TLS_KEY_FILE")
        && !value.trim().is_empty()
    {
        defaults.insert("client_key".to_string(), value);
    }
    if let Ok(value) = std::env::var("GRPCTESTIFY_TLS_SERVER_NAME")
        && !value.trim().is_empty()
    {
        defaults.insert("server_name".to_string(), value);
    }

    defaults
}

/// Parse truthy values from config-style strings.
pub fn parse_bool_flag(value: &str) -> bool {
    matches!(
        value.trim().to_ascii_lowercase().as_str(),
        "true" | "1" | "yes" | "on"
    )
}

/// Resolve effective address using ADDRESS section, env var, then protocol-dependent default.
/// `protocol_override` takes precedence over OPTIONS.protocol from the document.
pub fn effective_address(
    document: &GctfDocument,
    protocol_override: Option<WireProtocol>,
) -> String {
    document
        .get_address(std::env::var("GRPCTESTIFY_ADDRESS").ok().as_deref())
        .unwrap_or_else(|| {
            let proto = protocol_override.unwrap_or_else(|| {
                document
                    .get_options()
                    .and_then(|o| o.get("protocol").cloned())
                    .map(|s| s.parse().unwrap_or_default())
                    .unwrap_or_default()
            });
            default_address_for(proto).to_string()
        })
}

/// Resolve compression setting from OPTIONS section with env fallback.
pub fn parse_compression_option(options: &HashMap<String, String>) -> Option<CompressionMode> {
    options
        .get("compression")
        .map(|v| v.trim().to_ascii_lowercase())
        .and_then(|v| match v.as_str() {
            "gzip" => Some(CompressionMode::Gzip),
            "none" | "" => Some(CompressionMode::None),
            _ => None,
        })
}

/// Resolve the effective compression mode honoring the canonical precedence
/// `section attribute > OPTIONS > env default`. An explicit-but-unknown value
/// at either level is a configuration error (never a silent fall-back).
pub fn resolve_compression(
    document: &GctfDocument,
    options: &HashMap<String, String>,
    env_default: CompressionMode,
) -> Result<CompressionMode, String> {
    // `get_compression` only yields validated "gzip"/"none" (an invalid attribute
    // value is filtered to None and falls through to OPTIONS/env, matching
    // `resolve_effective_runtime_options`).
    if let Some(attr) = document
        .sections
        .iter()
        .filter_map(|s| s.get_compression())
        .next()
    {
        return Ok(if attr == "gzip" {
            CompressionMode::Gzip
        } else {
            CompressionMode::None
        });
    }

    if let Some(raw) = options.get("compression") {
        return parse_compression_option(options).ok_or_else(|| {
            format!(
                "OPTIONS.compression must be 'gzip' or 'none', got '{}'",
                raw
            )
        });
    }

    Ok(env_default)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RuntimeOptionSource {
    SectionAttribute,
    FileOptions,
    CliDefaults,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RuntimeOptionWithSource<T> {
    pub value: T,
    pub source: RuntimeOptionSource,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EffectiveRuntimeOptions {
    pub timeout_seconds: RuntimeOptionWithSource<u64>,
    pub retry: RuntimeOptionWithSource<u32>,
    pub retry_delay_seconds: RuntimeOptionWithSource<f64>,
    pub no_retry: RuntimeOptionWithSource<bool>,
    pub compression: RuntimeOptionWithSource<String>,
}

#[derive(Debug, Clone, Copy)]
pub struct CliRuntimeDefaults {
    pub timeout_seconds: u64,
    pub retry: u32,
    pub retry_delay_seconds: f64,
    pub no_retry: bool,
}

pub fn resolve_effective_runtime_options(
    document: &GctfDocument,
    cli: CliRuntimeDefaults,
) -> Result<EffectiveRuntimeOptions, String> {
    let options = document.get_options().unwrap_or_default();

    let timeout_from_attr = document
        .sections
        .iter()
        .filter_map(|s| s.get_timeout())
        .find(|&v| v > 0);
    let timeout_seconds = if let Some(v) = timeout_from_attr {
        RuntimeOptionWithSource {
            value: v,
            source: RuntimeOptionSource::SectionAttribute,
        }
    } else if let Some(value) = options.get("timeout") {
        match value.trim().parse::<u64>() {
            Ok(v) if v > 0 => RuntimeOptionWithSource {
                value: v,
                source: RuntimeOptionSource::FileOptions,
            },
            _ => {
                return Err(format!(
                    "OPTIONS.timeout must be a positive integer, got '{}'",
                    value
                ));
            }
        }
    } else {
        RuntimeOptionWithSource {
            value: cli.timeout_seconds,
            source: RuntimeOptionSource::CliDefaults,
        }
    };

    let no_retry_from_attr = document
        .sections
        .iter()
        .filter_map(|s| {
            s.get_attribute("no_retry")
                .or_else(|| s.get_attribute("no-retry"))
        })
        .filter_map(|a| a.parse_bool())
        .next();
    let no_retry = if let Some(v) = no_retry_from_attr {
        RuntimeOptionWithSource {
            value: v,
            source: RuntimeOptionSource::SectionAttribute,
        }
    } else if let Some(value) = options.get("no_retry").or_else(|| options.get("no-retry")) {
        let parsed = match value.trim().to_ascii_lowercase().as_str() {
            "true" | "1" | "yes" | "on" => Some(true),
            "false" | "0" | "no" | "off" => Some(false),
            _ => None,
        };
        match parsed {
            Some(v) => RuntimeOptionWithSource {
                value: v,
                source: RuntimeOptionSource::FileOptions,
            },
            None => {
                return Err(format!(
                    "OPTIONS.no_retry must be a boolean, got '{}'",
                    value
                ));
            }
        }
    } else {
        RuntimeOptionWithSource {
            value: cli.no_retry,
            source: RuntimeOptionSource::CliDefaults,
        }
    };

    let retry_from_attr = document
        .sections
        .iter()
        .filter_map(|s| s.get_retry())
        .next();
    let retry = if let Some(v) = retry_from_attr {
        RuntimeOptionWithSource {
            value: v,
            source: RuntimeOptionSource::SectionAttribute,
        }
    } else if let Some(value) = options.get("retry") {
        match value.trim().parse::<u32>() {
            Ok(v) => RuntimeOptionWithSource {
                value: v,
                source: RuntimeOptionSource::FileOptions,
            },
            Err(_) => {
                return Err(format!(
                    "OPTIONS.retry must be a non-negative integer, got '{}'",
                    value
                ));
            }
        }
    } else {
        RuntimeOptionWithSource {
            value: cli.retry,
            source: RuntimeOptionSource::CliDefaults,
        }
    };

    let retry_delay_from_attr = document
        .sections
        .iter()
        .filter_map(|s| {
            s.get_attribute("retry_delay")
                .or_else(|| s.get_attribute("retry-delay"))
        })
        .filter_map(|a| a.parse_f64())
        .find(|v| *v >= 0.0);
    let retry_delay_seconds = if let Some(v) = retry_delay_from_attr {
        RuntimeOptionWithSource {
            value: v,
            source: RuntimeOptionSource::SectionAttribute,
        }
    } else if let Some(value) = options
        .get("retry_delay")
        .or_else(|| options.get("retry-delay"))
    {
        match value.trim().parse::<f64>() {
            Ok(v) if v >= 0.0 => RuntimeOptionWithSource {
                value: v,
                source: RuntimeOptionSource::FileOptions,
            },
            _ => {
                return Err(format!(
                    "OPTIONS.retry_delay must be a non-negative number, got '{}'",
                    value
                ));
            }
        }
    } else {
        RuntimeOptionWithSource {
            value: cli.retry_delay_seconds,
            source: RuntimeOptionSource::CliDefaults,
        }
    };

    // Canonical compression precedence: section attribute > OPTIONS > env/CLI default.
    // `execution::runner` resolves the effective mode via `resolve_compression`
    // (same precedence), so this reporting path and the runner agree; both error
    // on an explicit-but-invalid value rather than silently falling back.
    let compression_from_attr = document
        .sections
        .iter()
        .filter_map(|s| s.get_compression())
        .next();
    let compression = if let Some(v) = compression_from_attr {
        RuntimeOptionWithSource {
            value: v,
            source: RuntimeOptionSource::SectionAttribute,
        }
    } else if let Some(raw) = options.get("compression") {
        // An explicit-but-unknown OPTIONS.compression is a configuration error,
        // not a silent fall-back to `none`.
        let mode = parse_compression_option(&options).ok_or_else(|| {
            format!(
                "OPTIONS.compression must be 'gzip' or 'none', got '{}'",
                raw
            )
        })?;
        RuntimeOptionWithSource {
            value: match mode {
                CompressionMode::Gzip => "gzip".to_string(),
                CompressionMode::None => "none".to_string(),
            },
            source: RuntimeOptionSource::FileOptions,
        }
    } else {
        RuntimeOptionWithSource {
            value: match CompressionMode::None {
                CompressionMode::Gzip => "gzip".to_string(),
                CompressionMode::None => "none".to_string(),
            },
            source: RuntimeOptionSource::CliDefaults,
        }
    };

    Ok(EffectiveRuntimeOptions {
        timeout_seconds,
        retry,
        retry_delay_seconds,
        no_retry,
        compression,
    })
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

/// Build TLS config using TLS section and env defaults, matching run behavior.
pub fn build_tls_config(document: &GctfDocument, document_path: &Path) -> Option<TlsConfig> {
    let tls_defaults = tls_env_defaults();
    let tls_section = document.get_tls_config();

    let pick_tls_value = |keys: &[&str]| -> Option<(String, bool)> {
        if let Some(section_map) = tls_section.as_ref() {
            for key in keys {
                if let Some(value) = section_map.get(*key) {
                    return Some((value.clone(), false));
                }
            }
        }

        for key in keys {
            if let Some(value) = tls_defaults.get(*key) {
                return Some((value.clone(), true));
            }
        }

        None
    };

    let ca_cert_path = pick_tls_value(&["ca_cert", "ca_file"])
        .map(|(v, from_env)| resolve_tls_path(&v, from_env, document_path));
    let client_cert_path = pick_tls_value(&["client_cert", "cert", "cert_file"])
        .map(|(v, from_env)| resolve_tls_path(&v, from_env, document_path));
    let client_key_path = pick_tls_value(&["client_key", "key", "key_file"])
        .map(|(v, from_env)| resolve_tls_path(&v, from_env, document_path));
    let server_name = pick_tls_value(&["server_name"]).map(|(v, _)| v);
    let insecure_skip_verify = tls_section
        .as_ref()
        .and_then(|m| m.get("insecure"))
        .is_some_and(|s| parse_bool_flag(s));

    if ca_cert_path.is_some()
        || client_cert_path.is_some()
        || client_key_path.is_some()
        || server_name.is_some()
        || insecure_skip_verify
    {
        Some(TlsConfig {
            ca_cert_path,
            client_cert_path,
            client_key_path,
            server_name,
            insecure_skip_verify,
        })
    } else {
        None
    }
}

/// Build proto config with document-relative path resolution, matching run behavior.
pub fn build_proto_config(document: &GctfDocument, document_path: &Path) -> Option<ProtoConfig> {
    document.get_proto_config().map(|proto_map| {
        let files = proto_map
            .get("files")
            .map(|s| {
                s.split(',')
                    .map(|p| {
                        FileUtils::resolve_relative_path(document_path, p.trim())
                            .to_string_lossy()
                            .to_string()
                    })
                    .collect()
            })
            .unwrap_or_default();

        let import_paths = proto_map
            .get("import_paths")
            .map(|s| {
                s.split(',')
                    .map(|p| {
                        FileUtils::resolve_relative_path(document_path, p.trim())
                            .to_string_lossy()
                            .to_string()
                    })
                    .collect()
            })
            .unwrap_or_default();

        let descriptor = proto_map.get("descriptor").map(|p| {
            FileUtils::resolve_relative_path(document_path, p)
                .to_string_lossy()
                .to_string()
        });

        ProtoConfig {
            files,
            import_paths,
            descriptor,
        }
    })
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

/// True when `body` is a single well-formed variable identifier, i.e. the kind
/// of placeholder `interpolate_variables`/`substitute_variables` treat as a
/// variable reference. This deliberately rejects anything with spaces or
/// punctuation so ordinary strings that merely contain `{{` (JSON fragments,
/// free text) are never mistaken for an unresolved placeholder.
fn is_variable_placeholder(body: &str) -> bool {
    let mut chars = body.chars();
    matches!(chars.next(), Some(c) if c.is_ascii_alphabetic() || c == '_')
        && chars.all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '.')
}

/// Append the names of unresolved `{{ identifier }}` placeholders found in
/// `text` to `out`. A placeholder is unresolved when its body is a well-formed
/// variable identifier that is absent from `variables` (i.e. the substitutor
/// would have left it verbatim). Names are de-duplicated, order preserved.
pub fn find_unresolved_placeholders(
    text: &str,
    variables: &HashMap<String, Value>,
    out: &mut Vec<String>,
) {
    let mut cursor = 0usize;
    while let Some(open_rel) = text[cursor..].find("{{") {
        let after_open = cursor + open_rel + 2;
        let Some(close_rel) = text[after_open..].find("}}") else {
            break;
        };
        let close = after_open + close_rel;
        let name = text[after_open..close].trim();
        if is_variable_placeholder(name)
            && !variables.contains_key(name)
            && !out.iter().any(|n| n == name)
        {
            out.push(name.to_string());
        }
        cursor = close + 2;
    }
}

/// Recursively collect unresolved `{{ identifier }}` placeholders from all
/// string values in a JSON `value` (used to guard outgoing request bodies).
pub fn collect_unresolved_placeholders(
    value: &Value,
    variables: &HashMap<String, Value>,
    out: &mut Vec<String>,
) {
    match value {
        Value::String(s) => find_unresolved_placeholders(s, variables, out),
        Value::Array(items) => {
            for item in items {
                collect_unresolved_placeholders(item, variables, out);
            }
        }
        Value::Object(map) => {
            for val in map.values() {
                collect_unresolved_placeholders(val, variables, out);
            }
        }
        _ => {}
    }
}

/// Format unresolved variable names back as `{{a}}, {{b}}` for error messages.
pub fn format_unresolved_placeholders(names: &[String]) -> String {
    names
        .iter()
        .map(|n| format!("{{{{{n}}}}}"))
        .collect::<Vec<_>>()
        .join(", ")
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

#[cfg(test)]
mod tests {
    use super::*;
    use apif_ast::{GctfAttribute, GctfDocument, Section, SectionContent, SectionType};

    fn make_doc(sections: Vec<Section>) -> GctfDocument {
        GctfDocument {
            file_path: "test.gctf".to_string(),
            sections,
            metadata: Default::default(),
            next_document: None,
        }
    }

    fn make_section(section_type: SectionType, content: SectionContent) -> Section {
        Section {
            section_type,
            content,
            inline_options: Default::default(),
            raw_content: String::new(),
            start_line: 0,
            end_line: 0,
            attributes: Vec::new(),
        }
    }

    fn make_section_with_attrs(
        section_type: SectionType,
        content: SectionContent,
        attrs: Vec<GctfAttribute>,
    ) -> Section {
        let mut s = make_section(section_type, content);
        s.attributes = attrs;
        s
    }

    fn cli_defaults() -> CliRuntimeDefaults {
        CliRuntimeDefaults {
            timeout_seconds: 30,
            retry: 0,
            retry_delay_seconds: 1.0,
            no_retry: false,
        }
    }

    fn kv(map: &[(&str, &str)]) -> SectionContent {
        SectionContent::KeyValues(
            map.iter()
                .map(|(k, v)| (k.to_string(), v.to_string()))
                .collect(),
        )
    }

    #[test]
    fn test_resolve_defaults_only() {
        let doc = make_doc(vec![
            make_section(
                SectionType::Endpoint,
                SectionContent::Single("svc/Method".into()),
            ),
            make_section(SectionType::Request, SectionContent::Empty),
        ]);
        let result = resolve_effective_runtime_options(&doc, cli_defaults()).unwrap();

        assert_eq!(result.timeout_seconds.value, 30);
        assert_eq!(
            result.timeout_seconds.source,
            RuntimeOptionSource::CliDefaults
        );
        assert_eq!(result.retry.value, 0);
        assert_eq!(result.retry.source, RuntimeOptionSource::CliDefaults);
        assert_eq!(result.retry_delay_seconds.value, 1.0);
        assert_eq!(
            result.retry_delay_seconds.source,
            RuntimeOptionSource::CliDefaults
        );
        assert!(!result.no_retry.value);
        assert_eq!(result.no_retry.source, RuntimeOptionSource::CliDefaults);
    }

    #[test]
    fn test_resolve_file_options_override_defaults() {
        let doc = make_doc(vec![
            make_section(
                SectionType::Options,
                kv(&[
                    ("timeout", "10"),
                    ("retry", "3"),
                    ("retry_delay", "0.5"),
                    ("no_retry", "true"),
                ]),
            ),
            make_section(
                SectionType::Endpoint,
                SectionContent::Single("svc/Method".into()),
            ),
            make_section(SectionType::Request, SectionContent::Empty),
        ]);
        let result = resolve_effective_runtime_options(&doc, cli_defaults()).unwrap();

        assert_eq!(result.timeout_seconds.value, 10);
        assert_eq!(
            result.timeout_seconds.source,
            RuntimeOptionSource::FileOptions
        );
        assert_eq!(result.retry.value, 3);
        assert_eq!(result.retry.source, RuntimeOptionSource::FileOptions);
        assert_eq!(result.retry_delay_seconds.value, 0.5);
        assert_eq!(
            result.retry_delay_seconds.source,
            RuntimeOptionSource::FileOptions
        );
        assert!(result.no_retry.value);
        assert_eq!(result.no_retry.source, RuntimeOptionSource::FileOptions);
    }

    #[test]
    fn test_resolve_section_attribute_overrides_file_options() {
        let doc = make_doc(vec![
            make_section(
                SectionType::Options,
                kv(&[("timeout", "10"), ("retry", "3"), ("retry_delay", "0.5")]),
            ),
            make_section_with_attrs(
                SectionType::Request,
                SectionContent::Empty,
                vec![
                    GctfAttribute::new("timeout", "5"),
                    GctfAttribute::new("retry", "7"),
                    GctfAttribute::new("retry_delay", "2.0"),
                    GctfAttribute::flag("no_retry"),
                ],
            ),
        ]);
        let result = resolve_effective_runtime_options(&doc, cli_defaults()).unwrap();

        assert_eq!(result.timeout_seconds.value, 5);
        assert_eq!(
            result.timeout_seconds.source,
            RuntimeOptionSource::SectionAttribute
        );
        assert_eq!(result.retry.value, 7);
        assert_eq!(result.retry.source, RuntimeOptionSource::SectionAttribute);
        assert_eq!(result.retry_delay_seconds.value, 2.0);
        assert_eq!(
            result.retry_delay_seconds.source,
            RuntimeOptionSource::SectionAttribute
        );
        assert!(result.no_retry.value);
        assert_eq!(
            result.no_retry.source,
            RuntimeOptionSource::SectionAttribute
        );
    }

    #[test]
    fn test_resolve_attribute_overrides_options_only_for_present_fields() {
        let doc = make_doc(vec![
            make_section(
                SectionType::Options,
                kv(&[("timeout", "10"), ("retry", "3")]),
            ),
            make_section_with_attrs(
                SectionType::Request,
                SectionContent::Empty,
                vec![GctfAttribute::new("retry", "5")],
            ),
        ]);
        let result = resolve_effective_runtime_options(&doc, cli_defaults()).unwrap();

        assert_eq!(result.timeout_seconds.value, 10);
        assert_eq!(
            result.timeout_seconds.source,
            RuntimeOptionSource::FileOptions
        );
        assert_eq!(result.retry.value, 5);
        assert_eq!(result.retry.source, RuntimeOptionSource::SectionAttribute);
        assert_eq!(result.retry_delay_seconds.value, 1.0);
        assert_eq!(
            result.retry_delay_seconds.source,
            RuntimeOptionSource::CliDefaults
        );
    }

    #[test]
    fn test_resolve_kebab_alias_in_options() {
        let doc = make_doc(vec![
            make_section(
                SectionType::Options,
                kv(&[("retry-delay", "0.2"), ("no-retry", "true")]),
            ),
            make_section(
                SectionType::Endpoint,
                SectionContent::Single("svc/Method".into()),
            ),
            make_section(SectionType::Request, SectionContent::Empty),
        ]);
        let result = resolve_effective_runtime_options(&doc, cli_defaults()).unwrap();

        assert_eq!(result.retry_delay_seconds.value, 0.2);
        assert_eq!(
            result.retry_delay_seconds.source,
            RuntimeOptionSource::FileOptions
        );
        assert!(result.no_retry.value);
        assert_eq!(result.no_retry.source, RuntimeOptionSource::FileOptions);
    }

    #[test]
    fn test_resolve_kebab_alias_in_attributes() {
        let doc = make_doc(vec![make_section_with_attrs(
            SectionType::Request,
            SectionContent::Empty,
            vec![
                GctfAttribute::new("retry-delay", "0.3"),
                GctfAttribute::new("no-retry", "true"),
            ],
        )]);
        let result = resolve_effective_runtime_options(&doc, cli_defaults()).unwrap();

        assert_eq!(result.retry_delay_seconds.value, 0.3);
        assert_eq!(
            result.retry_delay_seconds.source,
            RuntimeOptionSource::SectionAttribute
        );
        assert!(result.no_retry.value);
        assert_eq!(
            result.no_retry.source,
            RuntimeOptionSource::SectionAttribute
        );
    }

    #[test]
    fn test_resolve_error_invalid_timeout() {
        let doc = make_doc(vec![
            make_section(SectionType::Options, kv(&[("timeout", "abc")])),
            make_section(
                SectionType::Endpoint,
                SectionContent::Single("svc/Method".into()),
            ),
        ]);
        let result = resolve_effective_runtime_options(&doc, cli_defaults());
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("timeout"));
    }

    #[test]
    fn test_resolve_error_zero_timeout() {
        let doc = make_doc(vec![
            make_section(SectionType::Options, kv(&[("timeout", "0")])),
            make_section(
                SectionType::Endpoint,
                SectionContent::Single("svc/Method".into()),
            ),
        ]);
        let result = resolve_effective_runtime_options(&doc, cli_defaults());
        assert!(result.is_err());
    }

    #[test]
    fn test_resolve_error_invalid_retry() {
        let doc = make_doc(vec![
            make_section(SectionType::Options, kv(&[("retry", "abc")])),
            make_section(
                SectionType::Endpoint,
                SectionContent::Single("svc/Method".into()),
            ),
        ]);
        let result = resolve_effective_runtime_options(&doc, cli_defaults());
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("retry"));
    }

    #[test]
    fn test_resolve_error_invalid_retry_delay() {
        let doc = make_doc(vec![
            make_section(SectionType::Options, kv(&[("retry_delay", "abc")])),
            make_section(
                SectionType::Endpoint,
                SectionContent::Single("svc/Method".into()),
            ),
        ]);
        let result = resolve_effective_runtime_options(&doc, cli_defaults());
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("retry_delay"));
    }

    #[test]
    fn test_resolve_error_negative_retry_delay() {
        let doc = make_doc(vec![
            make_section(SectionType::Options, kv(&[("retry_delay", "-1.0")])),
            make_section(
                SectionType::Endpoint,
                SectionContent::Single("svc/Method".into()),
            ),
        ]);
        let result = resolve_effective_runtime_options(&doc, cli_defaults());
        assert!(result.is_err());
    }

    #[test]
    fn test_resolve_error_invalid_no_retry() {
        let doc = make_doc(vec![
            make_section(SectionType::Options, kv(&[("no_retry", "maybe")])),
            make_section(
                SectionType::Endpoint,
                SectionContent::Single("svc/Method".into()),
            ),
        ]);
        let result = resolve_effective_runtime_options(&doc, cli_defaults());
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("no_retry"));
    }

    #[test]
    fn test_resolve_zero_timeout_attribute_ignored() {
        let doc = make_doc(vec![make_section_with_attrs(
            SectionType::Request,
            SectionContent::Empty,
            vec![GctfAttribute::new("timeout", "0")],
        )]);
        let result = resolve_effective_runtime_options(&doc, cli_defaults()).unwrap();

        assert_eq!(result.timeout_seconds.value, 30);
        assert_eq!(
            result.timeout_seconds.source,
            RuntimeOptionSource::CliDefaults
        );
    }

    #[test]
    fn test_resolve_negative_retry_delay_attribute_ignored() {
        let doc = make_doc(vec![make_section_with_attrs(
            SectionType::Request,
            SectionContent::Empty,
            vec![GctfAttribute::new("retry_delay", "-0.5")],
        )]);
        let result = resolve_effective_runtime_options(&doc, cli_defaults()).unwrap();

        assert_eq!(result.retry_delay_seconds.value, 1.0);
        assert_eq!(
            result.retry_delay_seconds.source,
            RuntimeOptionSource::CliDefaults
        );
    }

    #[test]
    fn test_resolve_compression_from_options() {
        let doc = make_doc(vec![
            make_section(SectionType::Options, kv(&[("compression", "gzip")])),
            make_section(
                SectionType::Endpoint,
                SectionContent::Single("svc/Method".into()),
            ),
        ]);
        let result = resolve_effective_runtime_options(&doc, cli_defaults()).unwrap();

        assert_eq!(result.compression.value, "gzip");
        assert_eq!(result.compression.source, RuntimeOptionSource::FileOptions);
    }

    #[test]
    fn test_resolve_compression_defaults() {
        let doc = make_doc(vec![make_section(
            SectionType::Endpoint,
            SectionContent::Single("svc/Method".into()),
        )]);
        let result = resolve_effective_runtime_options(&doc, cli_defaults()).unwrap();

        assert_eq!(result.compression.source, RuntimeOptionSource::CliDefaults);
    }

    #[test]
    fn test_resolve_json_serialization() {
        let doc = make_doc(vec![
            make_section(
                SectionType::Options,
                kv(&[("timeout", "10"), ("retry", "3")]),
            ),
            make_section(
                SectionType::Endpoint,
                SectionContent::Single("svc/Method".into()),
            ),
        ]);
        let result = resolve_effective_runtime_options(&doc, cli_defaults()).unwrap();

        let json = serde_json::to_value(&result).unwrap();
        let obj = json.as_object().unwrap();
        assert!(obj.contains_key("timeout_seconds"));
        assert!(obj.contains_key("retry"));
        assert!(obj.contains_key("retry_delay_seconds"));
        assert!(obj.contains_key("no_retry"));
        assert!(obj.contains_key("compression"));

        let ts = &obj["timeout_seconds"];
        assert_eq!(ts["value"], 10);
        assert_eq!(ts["source"], "file_options");
    }

    #[test]
    fn test_runtime_option_source_serde_roundtrip() {
        let sources = vec![
            RuntimeOptionSource::SectionAttribute,
            RuntimeOptionSource::FileOptions,
            RuntimeOptionSource::CliDefaults,
        ];
        for source in sources {
            let json = serde_json::to_string(&source).unwrap();
            let back: RuntimeOptionSource = serde_json::from_str(&json).unwrap();
            assert_eq!(source, back);
        }
    }

    #[test]
    fn test_effective_runtime_options_clone_roundtrip() {
        let doc = make_doc(vec![
            make_section(SectionType::Options, kv(&[("timeout", "10")])),
            make_section(
                SectionType::Endpoint,
                SectionContent::Single("svc/Method".into()),
            ),
        ]);
        let result = resolve_effective_runtime_options(&doc, cli_defaults()).unwrap();
        let cloned = result.clone();
        assert_eq!(cloned.timeout_seconds.value, 10);
        assert_eq!(
            cloned.timeout_seconds.source,
            RuntimeOptionSource::FileOptions
        );
    }

    #[test]
    fn test_resolve_compression_from_section_attribute() {
        let doc = make_doc(vec![make_section_with_attrs(
            SectionType::Request,
            SectionContent::Empty,
            vec![GctfAttribute::new("compression", "gzip")],
        )]);
        let result = resolve_effective_runtime_options(&doc, cli_defaults()).unwrap();

        assert_eq!(result.compression.value, "gzip");
        assert_eq!(
            result.compression.source,
            RuntimeOptionSource::SectionAttribute
        );
    }

    #[test]
    fn test_resolve_compression_attribute_overrides_file_options() {
        let doc = make_doc(vec![
            make_section(SectionType::Options, kv(&[("compression", "none")])),
            make_section_with_attrs(
                SectionType::Request,
                SectionContent::Empty,
                vec![GctfAttribute::new("compression", "gzip")],
            ),
        ]);
        let result = resolve_effective_runtime_options(&doc, cli_defaults()).unwrap();

        assert_eq!(result.compression.value, "gzip");
        assert_eq!(
            result.compression.source,
            RuntimeOptionSource::SectionAttribute
        );
    }

    #[test]
    fn test_resolve_compression_attribute_none_value() {
        let doc = make_doc(vec![make_section_with_attrs(
            SectionType::Request,
            SectionContent::Empty,
            vec![GctfAttribute::new("compression", "none")],
        )]);
        let result = resolve_effective_runtime_options(&doc, cli_defaults()).unwrap();

        assert_eq!(result.compression.value, "none");
        assert_eq!(
            result.compression.source,
            RuntimeOptionSource::SectionAttribute
        );
    }

    #[test]
    fn test_resolve_compression_invalid_options_value_errors() {
        // An explicit-but-unknown OPTIONS.compression must be a hard error, not
        // a silent fall-back to `none`.
        let doc = make_doc(vec![
            make_section(SectionType::Options, kv(&[("compression", "brotli")])),
            make_section(
                SectionType::Endpoint,
                SectionContent::Single("svc/Method".into()),
            ),
        ]);
        let result = resolve_effective_runtime_options(&doc, cli_defaults());
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("compression"));
    }

    #[test]
    fn test_resolve_compression_invalid_attribute_value_falls_back() {
        let doc = make_doc(vec![
            make_section(SectionType::Options, kv(&[("compression", "gzip")])),
            make_section_with_attrs(
                SectionType::Request,
                SectionContent::Empty,
                vec![GctfAttribute::new("compression", "invalid")],
            ),
        ]);
        let result = resolve_effective_runtime_options(&doc, cli_defaults()).unwrap();

        assert_eq!(result.compression.value, "gzip");
        assert_eq!(result.compression.source, RuntimeOptionSource::FileOptions);
    }

    fn opts(pairs: &[(&str, &str)]) -> HashMap<String, String> {
        pairs
            .iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect()
    }

    #[test]
    fn test_resolve_compression_attribute_beats_options() {
        let doc = make_doc(vec![
            make_section(SectionType::Options, kv(&[("compression", "none")])),
            make_section_with_attrs(
                SectionType::Request,
                SectionContent::Empty,
                vec![GctfAttribute::new("compression", "gzip")],
            ),
        ]);
        assert_eq!(
            resolve_compression(
                &doc,
                &opts(&[("compression", "none")]),
                CompressionMode::None
            ),
            Ok(CompressionMode::Gzip)
        );
    }

    #[test]
    fn test_resolve_compression_options_used_when_no_attribute() {
        let doc = make_doc(vec![make_section(
            SectionType::Endpoint,
            SectionContent::Single("svc/M".into()),
        )]);
        assert_eq!(
            resolve_compression(
                &doc,
                &opts(&[("compression", "gzip")]),
                CompressionMode::None
            ),
            Ok(CompressionMode::Gzip)
        );
    }

    #[test]
    fn test_resolve_compression_invalid_options_is_error_not_fallback() {
        let doc = make_doc(vec![make_section(
            SectionType::Endpoint,
            SectionContent::Single("svc/M".into()),
        )]);
        let result = resolve_compression(
            &doc,
            &opts(&[("compression", "brotli")]),
            CompressionMode::Gzip,
        );
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("compression"));
    }

    #[test]
    fn test_resolve_compression_env_default_when_unset() {
        let doc = make_doc(vec![make_section(
            SectionType::Endpoint,
            SectionContent::Single("svc/M".into()),
        )]);
        assert_eq!(
            resolve_compression(&doc, &HashMap::new(), CompressionMode::Gzip),
            Ok(CompressionMode::Gzip)
        );
    }

    // (1) An undefined variable in a request body must be reported, not sent.
    #[test]
    fn test_collect_unresolved_placeholder_undefined_in_body() {
        let vars = HashMap::new();
        let mut body = serde_json::json!({ "user": "{{missing}}" });
        substitute_variables(&mut body, &vars);
        let mut unresolved = Vec::new();
        collect_unresolved_placeholders(&body, &vars, &mut unresolved);
        assert_eq!(unresolved, vec!["missing".to_string()]);
        assert_eq!(format_unresolved_placeholders(&unresolved), "{{missing}}");
    }

    // (2) A defined variable still substitutes and is not flagged.
    #[test]
    fn test_collect_unresolved_placeholder_bound_variable_ok() {
        let mut vars = HashMap::new();
        vars.insert("user_id".to_string(), Value::from(42));
        let mut body = serde_json::json!({ "id": "{{user_id}}", "note": "u={{user_id}}" });
        substitute_variables(&mut body, &vars);
        assert_eq!(body["id"], Value::from(42));
        assert_eq!(body["note"], Value::from("u=42"));
        let mut unresolved = Vec::new();
        collect_unresolved_placeholders(&body, &vars, &mut unresolved);
        assert!(unresolved.is_empty());
    }

    // (3) An unresolved placeholder in a header value is detected.
    #[test]
    fn test_find_unresolved_placeholder_in_header_value() {
        let vars = HashMap::new();
        let header_value =
            interpolate_variables("Bearer {{token}}", &vars).unwrap_or("Bearer {{token}}".into());
        let mut unresolved = Vec::new();
        find_unresolved_placeholders(&header_value, &vars, &mut unresolved);
        assert_eq!(unresolved, vec!["token".to_string()]);
    }

    // (4) A legitimate literal that merely contains braces is not false-flagged.
    #[test]
    fn test_collect_unresolved_placeholder_ignores_non_placeholder_literals() {
        let vars = HashMap::new();
        let body = serde_json::json!({
            "json_like": "{ \"a\": 1 }",
            "shell": "${HOME}",
            "free_text": "{{ not a var }}",
            "empty": "{{}}"
        });
        let mut unresolved = Vec::new();
        collect_unresolved_placeholders(&body, &vars, &mut unresolved);
        assert!(unresolved.is_empty(), "unexpected: {:?}", unresolved);
    }
}
