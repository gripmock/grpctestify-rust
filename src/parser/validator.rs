// GCTF document validator - validates parsed AST
// Checks for required sections, conflicts, and data integrity

use super::ast::*;
use crate::bench::schema::{
    BENCH_ASSERT_MODE_VALUES, BENCH_CACHE_VALUES, BENCH_DURATION_KEYS, BENCH_DURATION_STOP_VALUES,
    BENCH_LOAD_SCHEDULE_VALUES, BENCH_MODE_VALUES, BENCH_NUMERIC_KEYS, allowed_values_message,
    canonical_bench_key, is_allowed_value, suggest_bench_key, supported_bench_keys,
};
use anyhow::{Result, bail};
use serde::Serialize;

/// Validation error
#[derive(Debug, Clone, Serialize)]
pub struct ValidationError {
    pub message: String,
    pub line: Option<usize>,
    pub severity: ErrorSeverity,
}

/// Error severity
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum ErrorSeverity {
    Error,
    Warning,
    Info,
}

/// Validate a parsed GCTF document (returns all errors/warnings without bailing)
pub fn validate_document_diagnostics(document: &GctfDocument) -> Vec<ValidationError> {
    let mut errors = Vec::new();

    // Check for required sections
    validate_required_sections(document, &mut errors);

    // Check for conflicts
    validate_conflicts(document, &mut errors);

    // Validate content
    validate_content(document, &mut errors);

    // Validate structure
    validate_structure(document, &mut errors);

    errors
}

/// Validate a parsed GCTF document (legacy wrapper that bails on error)
pub fn validate_document(document: &GctfDocument) -> Result<Vec<ValidationError>> {
    let errors = validate_document_diagnostics(document);
    let has_errors = errors.iter().any(|e| e.severity == ErrorSeverity::Error);

    if has_errors {
        let error_messages: Vec<String> = errors
            .iter()
            .filter(|e| e.severity == ErrorSeverity::Error)
            .map(|e| format!("Line {}: {}", e.line.unwrap_or(0), e.message))
            .collect();

        bail!("Validation failed:\n{}", error_messages.join("\n"));
    }

    Ok(errors)
}

/// Validate required sections
fn validate_required_sections(document: &GctfDocument, errors: &mut Vec<ValidationError>) {
    // ENDPOINT is required
    if document.get_endpoint().is_none() {
        errors.push(ValidationError {
            message: "ENDPOINT section is required".to_string(),
            line: None,
            severity: ErrorSeverity::Error,
        });
    }

    // ADDRESS or environment variable is required
    // The address might also be provided via CLI args or Config file, which the validator
    // doesn't have access to here. We should probably relax this check or make it a warning.
    // Ideally validation happens with full context, but for now let's check env var.
    // If neither section nor env var is present, we warn instead of error,
    // because it might be supplied at runtime.
    let env_addr = std::env::var(crate::config::ENV_GRPCTESTIFY_ADDRESS).ok();
    if document.get_address(env_addr.as_deref()).is_none() {
        // Downgrade to warning because address can come from CLI/Config
        errors.push(ValidationError {
            message: format!(
                "ADDRESS section missing (ensure {} is set or passed via --address)",
                crate::config::ENV_GRPCTESTIFY_ADDRESS
            ),
            line: None,
            severity: ErrorSeverity::Warning,
        });
    }

    // At least RESPONSE, ERROR or ASSERTS should be present for verification
    let has_response = document.first_section(SectionType::Response).is_some();
    let has_error = document.first_section(SectionType::Error).is_some();
    let has_asserts = document.first_section(SectionType::Asserts).is_some();

    if !has_response && !has_error && !has_asserts {
        errors.push(ValidationError {
            message: "At least one verification section (RESPONSE, ERROR, or ASSERTS) is required"
                .to_string(),
            line: None,
            severity: ErrorSeverity::Error,
        });
    }
}

/// Validate conflicts
fn validate_conflicts(document: &GctfDocument, errors: &mut Vec<ValidationError>) {
    // RESPONSE and ERROR cannot both be present
    if document.has_response_error_conflict() {
        errors.push(ValidationError {
            message: "Cannot have both RESPONSE and ERROR sections".to_string(),
            line: None,
            severity: ErrorSeverity::Error,
        });
    }
}

/// Validate content
fn validate_content(document: &GctfDocument, errors: &mut Vec<ValidationError>) {
    // Validate endpoint format
    if let Some(endpoint) = document.get_endpoint()
        && !endpoint.contains('/')
    {
        errors.push(ValidationError {
            message: format!(
                "Invalid endpoint format: {}. Expected format: package.Service/Method",
                endpoint
            ),
            line: document
                .first_section(SectionType::Endpoint)
                .map(|s| s.start_line),
            severity: ErrorSeverity::Error,
        });
    }

    // Validate address format
    if let Some(address) = document.get_address(None)
        && !address.contains(':')
    {
        errors.push(ValidationError {
            message: format!(
                "Invalid address format: {}. Expected format: host:port",
                address
            ),
            line: document
                .first_section(SectionType::Address)
                .map(|s| s.start_line),
            severity: ErrorSeverity::Error,
        });
    }

    // Validate JSON sections
    for section_type in [
        SectionType::Request,
        SectionType::Response,
        SectionType::Error,
    ] {
        for section in document.sections_by_type(section_type) {
            match &section.content {
                SectionContent::Json(json) => {
                    // Check if JSON is valid object or array
                    // For ERROR, we also allow strings
                    let is_valid = if section_type == SectionType::Error {
                        json.is_object() || json.is_array() || json.is_string()
                    } else {
                        json.is_object() || json.is_array()
                    };

                    if !is_valid {
                        errors.push(ValidationError {
                            message: format!(
                                "{:?} section must contain valid JSON object or array{}",
                                section_type,
                                if section_type == SectionType::Error {
                                    " or string"
                                } else {
                                    ""
                                }
                            ),
                            line: Some(section.start_line),
                            severity: ErrorSeverity::Error,
                        });
                    }

                    if section_type == SectionType::Error
                        && let Some(details) = json.get("details")
                    {
                        if !details.is_array() {
                            errors.push(ValidationError {
                                message: "ERROR section field 'details' must be an array"
                                    .to_string(),
                                line: Some(section.start_line),
                                severity: ErrorSeverity::Error,
                            });
                        } else if let Some(detail_items) = details.as_array() {
                            for detail in detail_items {
                                if !detail.is_object() {
                                    errors.push(ValidationError {
                                        message: "ERROR section 'details' items must be objects"
                                            .to_string(),
                                        line: Some(section.start_line),
                                        severity: ErrorSeverity::Error,
                                    });
                                    break;
                                }

                                if let Some(type_value) = detail.get("@type")
                                    && !type_value.is_string()
                                {
                                    errors.push(ValidationError {
                                        message:
                                            "ERROR.details item field '@type' must be a string"
                                                .to_string(),
                                        line: Some(section.start_line),
                                        severity: ErrorSeverity::Error,
                                    });
                                }
                            }
                        }
                    }
                }
                SectionContent::JsonLines(values) => {
                    if section_type != SectionType::Response {
                        errors.push(ValidationError {
                            message: format!(
                                "{:?} section does not support newline-delimited JSON messages",
                                section_type
                            ),
                            line: Some(section.start_line),
                            severity: ErrorSeverity::Error,
                        });
                    } else if values.is_empty() {
                        errors.push(ValidationError {
                            message: "RESPONSE section contains no JSON messages".to_string(),
                            line: Some(section.start_line),
                            severity: ErrorSeverity::Error,
                        });
                    }
                }
                _ => {}
            }
        }
    }

    // Validate key-value sections
    for section_type in [
        SectionType::RequestHeaders,
        SectionType::Tls,
        SectionType::Proto,
        SectionType::Options,
        SectionType::Bench,
    ] {
        for section in document.sections_by_type(section_type) {
            if let SectionContent::KeyValues(kv) = &section.content {
                // Check for empty keys or values
                for key in kv.keys() {
                    if key.is_empty() {
                        errors.push(ValidationError {
                            message: format!("Empty key in {:?} section", section_type),
                            line: Some(section.start_line),
                            severity: ErrorSeverity::Error,
                        });
                    }
                }

                if section_type == SectionType::Options {
                    let mut parsed_no_retry: Option<bool> = None;
                    let mut parsed_retry: Option<u32> = None;
                    for (key, value) in kv {
                        match key.as_str() {
                            "timeout" => {
                                if value.trim().parse::<u64>().ok().is_none_or(|v| v == 0) {
                                    errors.push(ValidationError {
                                        message: format!(
                                            "OPTIONS.timeout must be a positive integer, got '{}'",
                                            value
                                        ),
                                        line: Some(section.start_line),
                                        severity: ErrorSeverity::Error,
                                    });
                                }
                            }
                            "no_retry" | "no-retry" => {
                                let normalized = value.trim().to_ascii_lowercase();
                                let is_bool = matches!(
                                    normalized.as_str(),
                                    "true" | "1" | "yes" | "on" | "false" | "0" | "no" | "off"
                                );
                                if !is_bool {
                                    errors.push(ValidationError {
                                        message: format!(
                                            "OPTIONS.{} must be a boolean, got '{}'",
                                            key, value
                                        ),
                                        line: Some(section.start_line),
                                        severity: ErrorSeverity::Error,
                                    });
                                } else {
                                    parsed_no_retry = Some(matches!(
                                        normalized.as_str(),
                                        "true" | "1" | "yes" | "on"
                                    ));
                                }

                                if key == "no-retry" {
                                    errors.push(ValidationError {
                                        message:
                                            "OPTIONS.no-retry is deprecated; prefer OPTIONS.no_retry"
                                                .to_string(),
                                        line: Some(section.start_line),
                                        severity: ErrorSeverity::Warning,
                                    });
                                }
                            }
                            "retry" => {
                                if value.trim().parse::<u32>().is_err() {
                                    errors.push(ValidationError {
                                        message: format!(
                                            "OPTIONS.retry must be a non-negative integer, got '{}'",
                                            value
                                        ),
                                        line: Some(section.start_line),
                                        severity: ErrorSeverity::Error,
                                    });
                                } else {
                                    parsed_retry = value.trim().parse::<u32>().ok();
                                }
                            }
                            "retry_delay" | "retry-delay" => {
                                if value.trim().parse::<f64>().ok().is_none_or(|v| v < 0.0) {
                                    errors.push(ValidationError {
                                        message: format!(
                                            "OPTIONS.retry_delay must be a non-negative number, got '{}'",
                                            value
                                        ),
                                        line: Some(section.start_line),
                                        severity: ErrorSeverity::Error,
                                    });
                                }

                                if key == "retry-delay" {
                                    errors.push(ValidationError {
                                        message:
                                            "OPTIONS.retry-delay is deprecated; prefer OPTIONS.retry_delay"
                                                .to_string(),
                                        line: Some(section.start_line),
                                        severity: ErrorSeverity::Warning,
                                    });
                                }
                            }
                            "compression" => {
                                let normalized = value.trim().to_ascii_lowercase();
                                if !matches!(normalized.as_str(), "none" | "gzip") {
                                    errors.push(ValidationError {
                                        message: format!(
                                            "OPTIONS.compression must be one of: none, gzip (got '{}')",
                                            value
                                        ),
                                        line: Some(section.start_line),
                                        severity: ErrorSeverity::Error,
                                    });
                                }
                            }
                            _ => {
                                errors.push(ValidationError {
                                    message: format!(
                                        "Unknown OPTIONS key '{}'. Supported keys: timeout, retry, retry_delay, no_retry, compression",
                                        key
                                    ),
                                    line: Some(section.start_line),
                                    severity: ErrorSeverity::Warning,
                                });
                            }
                        }
                    }

                    if parsed_no_retry == Some(true) && parsed_retry.is_some_and(|r| r > 0) {
                        errors.push(ValidationError {
                            message:
                                "OPTIONS.no_retry=true conflicts with OPTIONS.retry>0; retry value will be ignored"
                                    .to_string(),
                            line: Some(section.start_line),
                            severity: ErrorSeverity::Warning,
                        });
                    }
                } else if section_type == SectionType::Bench {
                    validate_bench_key_values(kv, section.start_line, errors);
                }
            }
        }
    }

    for section in &document.sections {
        for attr in &section.attributes {
            match attr.name.as_str() {
                "skip" => {
                    if attr.parse_bool().is_none() {
                        errors.push(ValidationError {
                            message: format!(
                                "Attribute #[skip] must be boolean-compatible, got '{}'",
                                attr.value
                            ),
                            line: Some(section.start_line),
                            severity: ErrorSeverity::Error,
                        });
                    }
                }
                "timeout" => {
                    if attr.parse_u64().is_none_or(|v| v == 0) {
                        errors.push(ValidationError {
                            message: format!(
                                "Attribute #[timeout] must be a positive integer, got '{}'",
                                attr.value
                            ),
                            line: Some(section.start_line),
                            severity: ErrorSeverity::Error,
                        });
                    }
                }
                "retry" => {
                    if attr.parse_u32().is_none() {
                        errors.push(ValidationError {
                            message: format!(
                                "Attribute #[retry] must be a non-negative integer, got '{}'",
                                attr.value
                            ),
                            line: Some(section.start_line),
                            severity: ErrorSeverity::Error,
                        });
                    }
                }
                "retry_delay" | "retry-delay" => {
                    if attr.parse_f64().is_none_or(|v| v < 0.0) {
                        errors.push(ValidationError {
                            message: format!(
                                "Attribute #[{}] must be a non-negative number, got '{}'",
                                attr.name, attr.value
                            ),
                            line: Some(section.start_line),
                            severity: ErrorSeverity::Error,
                        });
                    }

                    if attr.name == "retry-delay" {
                        errors.push(ValidationError {
                            message:
                                "Attribute #[retry-delay] is deprecated; prefer #[retry_delay]"
                                    .to_string(),
                            line: Some(section.start_line),
                            severity: ErrorSeverity::Warning,
                        });
                    }
                }
                "no_retry" | "no-retry" => {
                    if attr.parse_bool().is_none() {
                        errors.push(ValidationError {
                            message: format!(
                                "Attribute #[{}] must be boolean-compatible, got '{}'",
                                attr.name, attr.value
                            ),
                            line: Some(section.start_line),
                            severity: ErrorSeverity::Error,
                        });
                    }

                    if attr.name == "no-retry" {
                        errors.push(ValidationError {
                            message: "Attribute #[no-retry] is deprecated; prefer #[no_retry]"
                                .to_string(),
                            line: Some(section.start_line),
                            severity: ErrorSeverity::Warning,
                        });
                    }
                }
                "name" | "tag" | "owner" | "summary" => {}
                _ => {
                    errors.push(ValidationError {
                        message: format!(
                            "Unknown attribute '#[{}]'. Supported attributes: skip, timeout, retry, retry_delay, no_retry, name, tag, owner, summary",
                            attr.name
                        ),
                        line: Some(section.start_line),
                        severity: ErrorSeverity::Warning,
                    });
                }
            }
        }

        let no_retry_attr = section
            .attributes
            .iter()
            .find(|a| a.name == "no_retry" || a.name == "no-retry")
            .and_then(|a| a.parse_bool());
        let retry_attr = section
            .attributes
            .iter()
            .find(|a| a.name == "retry")
            .and_then(|a| a.parse_u32());
        if no_retry_attr == Some(true) && retry_attr.is_some_and(|r| r > 0) {
            errors.push(ValidationError {
                message:
                    "Attribute conflict: #[no_retry] with #[retry(N>0)] on same section; retry value will be ignored"
                        .to_string(),
                line: Some(section.start_line),
                severity: ErrorSeverity::Warning,
            });
        }
    }

    // Validate assertions
    for section in document.sections_by_type(SectionType::Asserts) {
        if let SectionContent::Assertions(assertions) = &section.content {
            for assertion in assertions {
                if assertion.is_empty() {
                    errors.push(ValidationError {
                        message: "Empty assertion found".to_string(),
                        line: Some(section.start_line),
                        severity: ErrorSeverity::Warning,
                    });
                }
            }
        }
    }
}

fn validate_bench_key_values(
    kv: &std::collections::HashMap<String, String>,
    start_line: usize,
    errors: &mut Vec<ValidationError>,
) {
    let supported_keys_message = bench_supported_keys_message();
    for (key, value) in kv {
        let key_norm = canonical_bench_key(key.as_str()).unwrap_or(key.as_str());
        match key_norm {
            "mode" => {
                if !is_allowed_value(value, BENCH_MODE_VALUES) {
                    errors.push(ValidationError {
                        message: format!(
                            "BENCH.mode must be one of: {} (got '{}')",
                            allowed_values_message(BENCH_MODE_VALUES),
                            value
                        ),
                        line: Some(start_line),
                        severity: ErrorSeverity::Error,
                    });
                }
            }
            "load_schedule" => {
                if !is_allowed_value(value, BENCH_LOAD_SCHEDULE_VALUES) {
                    errors.push(ValidationError {
                        message: format!(
                            "BENCH.load_schedule must be one of: {} (got '{}')",
                            allowed_values_message(BENCH_LOAD_SCHEDULE_VALUES),
                            value
                        ),
                        line: Some(start_line),
                        severity: ErrorSeverity::Error,
                    });
                }
            }
            k if BENCH_NUMERIC_KEYS.contains(&k) => {
                if value.trim().parse::<u64>().is_err() {
                    errors.push(ValidationError {
                        message: format!(
                            "BENCH.{} must be a non-negative integer, got '{}'",
                            key, value
                        ),
                        line: Some(start_line),
                        severity: ErrorSeverity::Error,
                    });
                }
            }
            k if BENCH_DURATION_KEYS.contains(&k) => {
                validate_bench_duration(key, value, start_line, errors);
            }
            "no_assert" | "count_errors_in_latency" => {
                let normalized = value.trim().to_ascii_lowercase();
                if !matches!(normalized.as_str(), "true" | "false" | "1" | "0") {
                    errors.push(ValidationError {
                        message: format!(
                            "BENCH.{} must be a boolean (true/false/1/0), got '{}'",
                            key, value
                        ),
                        line: Some(start_line),
                        severity: ErrorSeverity::Error,
                    });
                }
            }
            "duration_stop" => {
                if !is_allowed_value(value, BENCH_DURATION_STOP_VALUES) {
                    errors.push(ValidationError {
                        message: format!(
                            "BENCH.duration_stop must be one of: {} (got '{}')",
                            allowed_values_message(BENCH_DURATION_STOP_VALUES),
                            value
                        ),
                        line: Some(start_line),
                        severity: ErrorSeverity::Error,
                    });
                }
            }
            "latency_percentiles" => {
                validate_latency_percentiles(value, start_line, errors);
            }
            "sample_rate" => {
                if value
                    .trim()
                    .parse::<f64>()
                    .ok()
                    .is_none_or(|v| !(0.0..=1.0).contains(&v))
                {
                    errors.push(ValidationError {
                        message: format!("BENCH.sample_rate must be in [0,1], got '{}'", value),
                        line: Some(start_line),
                        severity: ErrorSeverity::Error,
                    });
                }
            }
            "assert_mode" => {
                if !is_allowed_value(value, BENCH_ASSERT_MODE_VALUES) {
                    errors.push(ValidationError {
                        message: format!(
                            "BENCH.assert_mode must be one of: {} (got '{}')",
                            allowed_values_message(BENCH_ASSERT_MODE_VALUES),
                            value
                        ),
                        line: Some(start_line),
                        severity: ErrorSeverity::Error,
                    });
                }
            }
            "cache" => {
                if !is_allowed_value(value, BENCH_CACHE_VALUES) {
                    errors.push(ValidationError {
                        message: format!(
                            "BENCH.cache must be one of: {} (got '{}')",
                            allowed_values_message(BENCH_CACHE_VALUES),
                            value
                        ),
                        line: Some(start_line),
                        severity: ErrorSeverity::Error,
                    });
                }
            }
            _ => {
                if key == "thresholds" || key.starts_with("thresholds.") {
                    validate_bench_threshold_key(key, value, start_line, errors);
                } else {
                    let hint = canonical_bench_key(key)
                        .filter(|canonical| *canonical != key)
                        .map(|canonical| format!(" Hint: use canonical key '{}'.", canonical))
                        .or_else(|| {
                            suggest_bench_key(key)
                                .map(|suggested| format!(" Hint: did you mean '{}' ?", suggested))
                        })
                        .unwrap_or_default();
                    errors.push(ValidationError {
                        message: format!(
                            "Unknown BENCH key '{}'. Supported keys: {}{}",
                            key, supported_keys_message, hint
                        ),
                        line: Some(start_line),
                        severity: ErrorSeverity::Warning,
                    });
                }
            }
        }
    }
}

fn bench_supported_keys_message() -> String {
    supported_bench_keys().join(", ")
}

fn validate_bench_duration(
    key: &str,
    value: &str,
    start_line: usize,
    errors: &mut Vec<ValidationError>,
) {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        errors.push(ValidationError {
            message: format!("BENCH.{} must not be empty", key),
            line: Some(start_line),
            severity: ErrorSeverity::Error,
        });
        return;
    }
    let unit = if trimmed.ends_with("ms") {
        &trimmed[..trimmed.len() - 2]
    } else if trimmed.ends_with('s') {
        &trimmed[..trimmed.len() - 1]
    } else if trimmed.ends_with('m') {
        &trimmed[..trimmed.len() - 1]
    } else if trimmed.ends_with('h') {
        &trimmed[..trimmed.len() - 1]
    } else {
        trimmed
    };
    if unit.parse::<f64>().is_err() {
        errors.push(ValidationError {
            message: format!(
                "BENCH.{} has invalid duration format '{}'; expected e.g. 30s, 5m, 1h, 500ms",
                key, value
            ),
            line: Some(start_line),
            severity: ErrorSeverity::Error,
        });
    }
}

fn validate_latency_percentiles(value: &str, start_line: usize, errors: &mut Vec<ValidationError>) {
    for token in value.split(',') {
        let t = token.trim();
        if t.is_empty() {
            continue;
        }
        if !t.starts_with('p') {
            errors.push(ValidationError {
                message: format!(
                    "Invalid percentile '{}' in latency_percentiles; expected p50, p90, p95, p99, p99.9, etc.",
                    t
                ),
                line: Some(start_line),
                severity: ErrorSeverity::Error,
            });
            continue;
        }
        let num_str = t[1..].trim();
        if num_str.parse::<f64>().is_err() {
            errors.push(ValidationError {
                message: format!(
                    "Invalid percentile value in '{}'; expected number after 'p'",
                    t
                ),
                line: Some(start_line),
                severity: ErrorSeverity::Error,
            });
        }
    }
}

fn validate_bench_threshold_key(
    key: &str,
    value: &str,
    start_line: usize,
    errors: &mut Vec<ValidationError>,
) {
    if !is_valid_threshold_expr(value) {
        errors.push(ValidationError {
            message: format!(
                "BENCH threshold '{}' has invalid expression '{}'; expected one of: <N, <=N, >N, >=N",
                key, value
            ),
            line: Some(start_line),
            severity: ErrorSeverity::Error,
        });
    }

    if let Some(inner) = key.strip_prefix("thresholds.") {
        validate_percentile_metric_key(inner, start_line, errors);
    }
}

fn validate_percentile_metric_key(key: &str, start_line: usize, errors: &mut Vec<ValidationError>) {
    let p_metric = if key.starts_with("latency_ms.p(") {
        key.strip_prefix("latency_ms.p(")
    } else if key.starts_with("p(") {
        key.strip_prefix("p(")
    } else {
        None
    };

    let Some(rest) = p_metric else {
        return;
    };

    let Some(percentile_str) = rest.strip_suffix(')') else {
        errors.push(ValidationError {
            message: format!(
                "Invalid percentile key '{}'; expected syntax p(<value>) or latency_ms.p(<value>)",
                key
            ),
            line: Some(start_line),
            severity: ErrorSeverity::Error,
        });
        return;
    };

    let Ok(percentile) = percentile_str.parse::<f64>() else {
        errors.push(ValidationError {
            message: format!(
                "Invalid percentile value in key '{}'; expected numeric value",
                key
            ),
            line: Some(start_line),
            severity: ErrorSeverity::Error,
        });
        return;
    };

    if !(percentile > 0.0 && percentile < 100.0) {
        errors.push(ValidationError {
            message: format!("Percentile in key '{}' must be in range (0,100)", key),
            line: Some(start_line),
            severity: ErrorSeverity::Error,
        });
    }
}

fn is_valid_threshold_expr(raw: &str) -> bool {
    let value = raw.trim();
    let (op, rhs) = if let Some(rest) = value.strip_prefix("<=") {
        ("<=", rest)
    } else if let Some(rest) = value.strip_prefix(">=") {
        (">=", rest)
    } else if let Some(rest) = value.strip_prefix('<') {
        ("<", rest)
    } else if let Some(rest) = value.strip_prefix('>') {
        (">", rest)
    } else {
        return false;
    };

    let _ = op;
    rhs.trim().parse::<f64>().is_ok()
}

/// Validate structure
fn validate_structure(document: &GctfDocument, errors: &mut Vec<ValidationError>) {
    // Check for duplicate non-multiple sections
    let mut seen_sections = std::collections::HashSet::new();
    let mut meta_count = 0;
    let mut meta_first_line = None;
    let mut bench_count = 0;
    let mut bench_first_line = None;

    for section in &document.sections {
        if section.section_type == SectionType::Meta {
            meta_count += 1;
            if meta_first_line.is_none() {
                meta_first_line = Some(section.start_line);
            }
        }
        if section.section_type == SectionType::Bench {
            bench_count += 1;
            if bench_first_line.is_none() {
                bench_first_line = Some(section.start_line);
            }
        }

        if !section.section_type.is_multiple_allowed() {
            if seen_sections.contains(&section.section_type) {
                errors.push(ValidationError {
                    message: format!("Duplicate {:?} section found", section.section_type),
                    line: Some(section.start_line),
                    severity: ErrorSeverity::Error,
                });
            }
            seen_sections.insert(section.section_type);
        }
    }

    // Validate META section: only 0 or 1 per file, must be first if present
    if meta_count > 1 {
        errors.push(ValidationError {
            message: "Only one META section is allowed per file".to_string(),
            line: meta_first_line,
            severity: ErrorSeverity::Error,
        });
    }

    // Check META is first section (if present)
    if meta_count == 1
        && let Some(first_section) = document.sections.first()
        && first_section.section_type != SectionType::Meta
    {
        errors.push(ValidationError {
            message: "META section must be the first section in the file".to_string(),
            line: meta_first_line,
            severity: ErrorSeverity::Error,
        });
    }

    // Validate BENCH section: only 0 or 1 per file, should be file-level near top
    if bench_count > 1 {
        errors.push(ValidationError {
            message: "Only one BENCH section is allowed per file".to_string(),
            line: bench_first_line,
            severity: ErrorSeverity::Error,
        });
    }

    if bench_count == 1
        && let Some(bench_idx) = document
            .sections
            .iter()
            .position(|s| s.section_type == SectionType::Bench)
    {
        let bench_is_valid_position = match bench_idx {
            0 => true,
            1 => document
                .sections
                .first()
                .is_some_and(|s| s.section_type == SectionType::Meta),
            _ => false,
        };

        if !bench_is_valid_position {
            errors.push(ValidationError {
                message: "BENCH section must be first, or immediately after META".to_string(),
                line: bench_first_line,
                severity: ErrorSeverity::Warning,
            });
        }
    }

    // Validate section order (optional, but good for readability)
    // Not enforcing strict order, just checking for obvious issues
    // TODO: Add optional strict ordering validation

    // Validate inline options are only on supported sections
    for section in &document.sections {
        let has_any_inline_options = section.inline_options.with_asserts
            || section.inline_options.partial
            || section.inline_options.tolerance.is_some()
            || !section.inline_options.redact.is_empty()
            || section.inline_options.unordered_arrays;

        if !has_any_inline_options {
            continue;
        }

        match section.section_type {
            SectionType::Response => {
                // All known options are supported for RESPONSE section
            }
            SectionType::Error => {
                if section.inline_options.tolerance.is_some()
                    || !section.inline_options.redact.is_empty()
                    || section.inline_options.unordered_arrays
                {
                    errors.push(ValidationError {
                        message:
                            "ERROR section only supports partial and with_asserts inline options"
                                .to_string(),
                        line: Some(section.start_line),
                        severity: ErrorSeverity::Warning,
                    });
                }
            }
            _ => {
                errors.push(ValidationError {
                    message: format!(
                        "Inline options are not supported for {:?} section",
                        section.section_type
                    ),
                    line: Some(section.start_line),
                    severity: ErrorSeverity::Warning,
                });
            }
        }
    }

    // Warn about redundant empty ERROR with with_asserts
    for (i, section) in document.sections.iter().enumerate() {
        if section.section_type == SectionType::Error
            && section.inline_options.with_asserts
            && matches!(section.content, SectionContent::Empty)
            && document
                .sections
                .get(i + 1)
                .is_some_and(|next| next.section_type == SectionType::Asserts)
        {
            errors.push(ValidationError {
                message:
                    "Empty ERROR with with_asserts is redundant; remove ERROR and keep ASSERTS"
                        .to_string(),
                line: Some(section.start_line),
                severity: ErrorSeverity::Warning,
            });
        }
    }
}

/// Check if validation passed (no errors)
pub fn validation_passed(errors: &[ValidationError]) -> bool {
    !errors.iter().any(|e| e.severity == ErrorSeverity::Error)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn create_test_document() -> GctfDocument {
        let mut doc = GctfDocument::new("test.gctf".to_string());

        doc.sections = vec![
            Section {
                section_type: SectionType::Address,
                content: SectionContent::Single("localhost:4770".to_string()),
                inline_options: InlineOptions::default(),
                raw_content: "localhost:4770".to_string(),
                start_line: 1,
                end_line: 1,
                attributes: Vec::new(),
            },
            Section {
                section_type: SectionType::Endpoint,
                content: SectionContent::Single("my.Service/Method".to_string()),
                inline_options: InlineOptions::default(),
                raw_content: "my.Service/Method".to_string(),
                start_line: 3,
                end_line: 3,
                attributes: Vec::new(),
            },
        ];

        doc
    }

    #[test]
    fn test_validate_required_sections_pass() {
        let doc = create_test_document();
        let result = validate_document(&doc);
        // Should fail because no REQUEST or ASSERTS
        assert!(result.is_err());
    }

    #[test]
    fn test_validate_endpoint_format() {
        let mut doc = create_test_document();
        doc.sections[1].content = SectionContent::Single("invalid_endpoint".to_string());

        let result = validate_document(&doc);
        assert!(result.is_err());
    }

    #[test]
    fn test_validate_address_format() {
        let mut doc = create_test_document();
        doc.sections[0].content = SectionContent::Single("invalid_address".to_string());

        let result = validate_document(&doc);
        assert!(result.is_err());
    }

    #[test]
    fn test_validation_passed() {
        let errors = vec![
            ValidationError {
                message: "Warning".to_string(),
                line: Some(1),
                severity: ErrorSeverity::Warning,
            },
            ValidationError {
                message: "Info".to_string(),
                line: Some(2),
                severity: ErrorSeverity::Info,
            },
        ];

        assert!(validation_passed(&errors));
    }

    #[test]
    fn test_validation_failed() {
        let errors = vec![
            ValidationError {
                message: "Warning".to_string(),
                line: Some(1),
                severity: ErrorSeverity::Warning,
            },
            ValidationError {
                message: "Error".to_string(),
                line: Some(2),
                severity: ErrorSeverity::Error,
            },
        ];

        assert!(!validation_passed(&errors));
    }

    #[test]
    fn test_validate_document_diagnostics() {
        let doc = create_test_document();
        let errors = validate_document_diagnostics(&doc);
        // Should have some errors or warnings
        assert!(!errors.is_empty());
    }

    #[test]
    fn test_validate_document_with_response() {
        let mut doc = create_test_document();
        doc.sections.push(Section {
            section_type: SectionType::Response,
            content: SectionContent::Json(serde_json::json!({"result": "ok"})),
            inline_options: InlineOptions::default(),
            raw_content: "{\"result\": \"ok\"}".to_string(),
            start_line: 5,
            end_line: 6,
            attributes: Vec::new(),
        });

        let result = validate_document(&doc);
        // Should pass with ADDRESS, ENDPOINT, and RESPONSE
        assert!(result.is_ok());
    }

    #[test]
    fn test_validate_document_with_error_section() {
        let mut doc = create_test_document();
        doc.sections.push(Section {
            section_type: SectionType::Error,
            content: SectionContent::Json(serde_json::json!({"code": 5})),
            inline_options: InlineOptions::default(),
            raw_content: "{\"code\": 5}".to_string(),
            start_line: 5,
            end_line: 6,
            attributes: Vec::new(),
        });

        let result = validate_document(&doc);
        // Should pass with ADDRESS, ENDPOINT, and ERROR
        assert!(result.is_ok());
    }

    #[test]
    fn test_validate_document_error_partial_option_allowed() {
        let mut doc = create_test_document();
        doc.sections.push(Section {
            section_type: SectionType::Error,
            content: SectionContent::Json(serde_json::json!({"code": 5})),
            inline_options: InlineOptions {
                partial: true,
                ..InlineOptions::default()
            },
            raw_content: "{\"code\": 5}".to_string(),
            start_line: 5,
            end_line: 6,
            attributes: Vec::new(),
        });

        let errors = validate_document_diagnostics(&doc);
        assert!(!errors.iter().any(|e| {
            e.message
                .contains("ERROR section only supports partial and with_asserts")
        }));
    }

    #[test]
    fn test_validate_document_error_tolerance_still_warns() {
        let mut doc = create_test_document();
        doc.sections.push(Section {
            section_type: SectionType::Error,
            content: SectionContent::Json(serde_json::json!({"code": 5})),
            inline_options: InlineOptions {
                tolerance: Some(0.1),
                ..InlineOptions::default()
            },
            raw_content: "{\"code\": 5}".to_string(),
            start_line: 5,
            end_line: 6,
            attributes: Vec::new(),
        });

        let errors = validate_document_diagnostics(&doc);
        assert!(errors.iter().any(|e| {
            e.message
                .contains("ERROR section only supports partial and with_asserts")
        }));
    }

    #[test]
    fn test_validate_document_warns_on_empty_error_with_asserts() {
        let mut doc = create_test_document();
        doc.sections.push(Section {
            section_type: SectionType::Error,
            content: SectionContent::Empty,
            inline_options: InlineOptions {
                with_asserts: true,
                ..InlineOptions::default()
            },
            raw_content: "".to_string(),
            start_line: 5,
            end_line: 5,
            attributes: Vec::new(),
        });
        doc.sections.push(Section {
            section_type: SectionType::Asserts,
            content: SectionContent::Assertions(vec![".code == 5".to_string()]),
            inline_options: InlineOptions::default(),
            raw_content: ".code == 5".to_string(),
            start_line: 6,
            end_line: 6,
            attributes: Vec::new(),
        });

        let errors = validate_document_diagnostics(&doc);
        assert!(errors.iter().any(|e| {
            e.message
                .contains("Empty ERROR with with_asserts is redundant")
                && e.severity == ErrorSeverity::Warning
        }));
    }

    #[test]
    fn test_validate_document_no_warning_for_non_empty_error_with_asserts() {
        let mut doc = create_test_document();
        doc.sections.push(Section {
            section_type: SectionType::Error,
            content: SectionContent::Json(serde_json::json!({"code": 5})),
            inline_options: InlineOptions {
                with_asserts: true,
                ..InlineOptions::default()
            },
            raw_content: "{\"code\": 5}".to_string(),
            start_line: 5,
            end_line: 6,
            attributes: Vec::new(),
        });
        doc.sections.push(Section {
            section_type: SectionType::Asserts,
            content: SectionContent::Assertions(vec![".code == 5".to_string()]),
            inline_options: InlineOptions::default(),
            raw_content: ".code == 5".to_string(),
            start_line: 7,
            end_line: 7,
            attributes: Vec::new(),
        });

        let errors = validate_document_diagnostics(&doc);
        assert!(!errors.iter().any(|e| {
            e.message
                .contains("Empty ERROR with with_asserts is redundant")
        }));
    }

    #[test]
    fn test_validate_document_no_warning_for_empty_error_without_with_asserts() {
        let mut doc = create_test_document();
        doc.sections.push(Section {
            section_type: SectionType::Error,
            content: SectionContent::Empty,
            inline_options: InlineOptions::default(),
            raw_content: "".to_string(),
            start_line: 5,
            end_line: 5,
            attributes: Vec::new(),
        });
        doc.sections.push(Section {
            section_type: SectionType::Asserts,
            content: SectionContent::Assertions(vec![".code == 5".to_string()]),
            inline_options: InlineOptions::default(),
            raw_content: ".code == 5".to_string(),
            start_line: 6,
            end_line: 6,
            attributes: Vec::new(),
        });

        let errors = validate_document_diagnostics(&doc);
        assert!(!errors.iter().any(|e| {
            e.message
                .contains("Empty ERROR with with_asserts is redundant")
        }));
    }

    #[test]
    fn test_validate_document_no_warning_for_empty_error_with_non_adjacent_asserts() {
        let mut doc = create_test_document();
        doc.sections.push(Section {
            section_type: SectionType::Error,
            content: SectionContent::Empty,
            inline_options: InlineOptions {
                with_asserts: true,
                ..InlineOptions::default()
            },
            raw_content: "".to_string(),
            start_line: 5,
            end_line: 5,
            attributes: Vec::new(),
        });
        doc.sections.push(Section {
            section_type: SectionType::Request,
            content: SectionContent::Json(serde_json::json!({"id": 1})),
            inline_options: InlineOptions::default(),
            raw_content: "{\"id\": 1}".to_string(),
            start_line: 6,
            end_line: 7,
            attributes: Vec::new(),
        });
        doc.sections.push(Section {
            section_type: SectionType::Asserts,
            content: SectionContent::Assertions(vec![".code == 5".to_string()]),
            inline_options: InlineOptions::default(),
            raw_content: ".code == 5".to_string(),
            start_line: 8,
            end_line: 8,
            attributes: Vec::new(),
        });

        let errors = validate_document_diagnostics(&doc);
        assert!(!errors.iter().any(|e| {
            e.message
                .contains("Empty ERROR with with_asserts is redundant")
        }));
    }

    #[test]
    fn test_validate_document_with_asserts() {
        let mut doc = create_test_document();
        doc.sections.push(Section {
            section_type: SectionType::Asserts,
            content: SectionContent::Assertions(vec![".id == 1".to_string()]),
            inline_options: InlineOptions::default(),
            raw_content: ".id == 1".to_string(),
            start_line: 5,
            end_line: 5,
            attributes: Vec::new(),
        });

        let result = validate_document(&doc);
        // Should pass with ADDRESS, ENDPOINT, and ASSERTS
        assert!(result.is_ok());
    }

    #[test]
    fn test_validate_document_missing_endpoint() {
        let mut doc = create_test_document();
        doc.sections.remove(1); // Remove ENDPOINT

        let errors = validate_document_diagnostics(&doc);
        let has_endpoint_error = errors.iter().any(|e| e.message.contains("ENDPOINT"));
        assert!(has_endpoint_error);
    }

    #[test]
    fn test_validate_document_response_error_conflict() {
        let mut doc = create_test_document();
        doc.sections.push(Section {
            section_type: SectionType::Response,
            content: SectionContent::Json(serde_json::json!({"result": "ok"})),
            inline_options: InlineOptions::default(),
            raw_content: "{\"result\": \"ok\"}".to_string(),
            start_line: 5,
            end_line: 6,
            attributes: Vec::new(),
        });
        doc.sections.push(Section {
            section_type: SectionType::Error,
            content: SectionContent::Json(serde_json::json!({"code": 5})),
            inline_options: InlineOptions::default(),
            raw_content: "{\"code\": 5}".to_string(),
            start_line: 7,
            end_line: 8,
            attributes: Vec::new(),
        });

        let errors = validate_document_diagnostics(&doc);
        let has_conflict_error = errors
            .iter()
            .any(|e| e.message.contains("RESPONSE") && e.message.contains("ERROR"));
        assert!(has_conflict_error);
    }

    #[test]
    fn test_validate_document_empty_requests() {
        let mut doc = create_test_document();
        doc.sections.push(Section {
            section_type: SectionType::Request,
            content: SectionContent::Empty,
            inline_options: InlineOptions::default(),
            raw_content: "".to_string(),
            start_line: 5,
            end_line: 5,
            attributes: Vec::new(),
        });
        doc.sections.push(Section {
            section_type: SectionType::Response,
            content: SectionContent::Json(serde_json::json!({"result": "ok"})),
            inline_options: InlineOptions::default(),
            raw_content: "{\"result\": \"ok\"}".to_string(),
            start_line: 6,
            end_line: 7,
            attributes: Vec::new(),
        });

        let result = validate_document(&doc);
        // Empty REQUEST is allowed
        assert!(result.is_ok());
    }

    #[test]
    fn test_validate_document_invalid_request_json() {
        let mut doc = create_test_document();
        doc.sections.push(Section {
            section_type: SectionType::Request,
            content: SectionContent::Json(serde_json::json!({"key": "value"})),
            inline_options: InlineOptions::default(),
            raw_content: "{\"key\": \"value\"}".to_string(),
            start_line: 5,
            end_line: 6,
            attributes: Vec::new(),
        });
        doc.sections.push(Section {
            section_type: SectionType::Response,
            content: SectionContent::Json(serde_json::json!({"result": "ok"})),
            inline_options: InlineOptions::default(),
            raw_content: "{\"result\": \"ok\"}".to_string(),
            start_line: 7,
            end_line: 8,
            attributes: Vec::new(),
        });

        let result = validate_document(&doc);
        // Valid JSON should pass
        assert!(result.is_ok());
    }

    #[test]
    fn test_validate_document_invalid_response_json() {
        let mut doc = create_test_document();
        doc.sections.push(Section {
            section_type: SectionType::Request,
            content: SectionContent::Json(serde_json::json!({"key": "value"})),
            inline_options: InlineOptions::default(),
            raw_content: "{\"key\": \"value\"}".to_string(),
            start_line: 5,
            end_line: 6,
            attributes: Vec::new(),
        });
        doc.sections.push(Section {
            section_type: SectionType::Response,
            content: SectionContent::Json(serde_json::json!({"result": "ok"})),
            inline_options: InlineOptions::default(),
            raw_content: "{\"result\": \"ok\"}".to_string(),
            start_line: 7,
            end_line: 8,
            attributes: Vec::new(),
        });

        let errors = validate_document_diagnostics(&doc);
        // Valid JSON should have no errors
        let has_json_errors = errors.iter().any(|e| e.message.contains("JSON"));
        assert!(!has_json_errors);
    }

    #[test]
    fn test_validate_error_details_must_be_array() {
        let mut doc = create_test_document();
        doc.sections.push(Section {
            section_type: SectionType::Error,
            content: SectionContent::Json(serde_json::json!({
                "code": 3,
                "details": {"@type": "type.googleapis.com/google.rpc.ErrorInfo"}
            })),
            inline_options: InlineOptions::default(),
            raw_content: "".to_string(),
            start_line: 5,
            end_line: 8,
            attributes: Vec::new(),
        });

        let errors = validate_document_diagnostics(&doc);
        assert!(
            errors
                .iter()
                .any(|e| e.message.contains("field 'details' must be an array"))
        );
    }

    #[test]
    fn test_validate_error_details_items_must_be_objects() {
        let mut doc = create_test_document();
        doc.sections.push(Section {
            section_type: SectionType::Error,
            content: SectionContent::Json(serde_json::json!({
                "code": 3,
                "details": ["not-an-object"]
            })),
            inline_options: InlineOptions::default(),
            raw_content: "".to_string(),
            start_line: 5,
            end_line: 8,
            attributes: Vec::new(),
        });

        let errors = validate_document_diagnostics(&doc);
        assert!(
            errors
                .iter()
                .any(|e| e.message.contains("'details' items must be objects"))
        );
    }

    #[test]
    fn test_validate_document_address_from_env() {
        // Set env var
        unsafe {
            std::env::set_var(crate::config::ENV_GRPCTESTIFY_ADDRESS, "env:5000");
        }

        let mut doc = GctfDocument::new("test.gctf".to_string());
        doc.sections.push(Section {
            section_type: SectionType::Endpoint,
            content: SectionContent::Single("Service/Method".to_string()),
            inline_options: InlineOptions::default(),
            raw_content: "Service/Method".to_string(),
            start_line: 1,
            end_line: 1,
            attributes: Vec::new(),
        });
        doc.sections.push(Section {
            section_type: SectionType::Response,
            content: SectionContent::Json(serde_json::json!({"result": "ok"})),
            inline_options: InlineOptions::default(),
            raw_content: "{\"result\": \"ok\"}".to_string(),
            start_line: 2,
            end_line: 3,
            attributes: Vec::new(),
        });

        let result = validate_document(&doc);
        // Should pass because address comes from env
        assert!(result.is_ok());

        // Clean up
        unsafe {
            std::env::remove_var(crate::config::ENV_GRPCTESTIFY_ADDRESS);
        }
    }

    #[test]
    fn test_validate_options_unknown_key_warning() {
        let mut doc = create_test_document();
        let mut options = std::collections::HashMap::new();
        options.insert("unknown".to_string(), "value".to_string());
        doc.sections.push(Section {
            section_type: SectionType::Options,
            content: SectionContent::KeyValues(options),
            inline_options: InlineOptions::default(),
            raw_content: "unknown: value".to_string(),
            start_line: 5,
            end_line: 6,
            attributes: Vec::new(),
        });
        doc.sections.push(Section {
            section_type: SectionType::Response,
            content: SectionContent::Json(serde_json::json!({"result": "ok"})),
            inline_options: InlineOptions::default(),
            raw_content: "{\"result\": \"ok\"}".to_string(),
            start_line: 7,
            end_line: 8,
            attributes: Vec::new(),
        });

        let diagnostics = validate_document_diagnostics(&doc);
        assert!(diagnostics.iter().any(|d| {
            d.severity == ErrorSeverity::Warning && d.message.contains("Unknown OPTIONS key")
        }));
    }

    #[test]
    fn test_validate_options_dry_run_is_unknown_key_warning() {
        let mut doc = create_test_document();
        let mut options = std::collections::HashMap::new();
        options.insert("dry_run".to_string(), "true".to_string());
        doc.sections.push(Section {
            section_type: SectionType::Options,
            content: SectionContent::KeyValues(options),
            inline_options: InlineOptions::default(),
            raw_content: "dry_run: true".to_string(),
            start_line: 5,
            end_line: 6,
            attributes: Vec::new(),
        });
        doc.sections.push(Section {
            section_type: SectionType::Response,
            content: SectionContent::Json(serde_json::json!({"result": "ok"})),
            inline_options: InlineOptions::default(),
            raw_content: "{\"result\": \"ok\"}".to_string(),
            start_line: 7,
            end_line: 8,
            attributes: Vec::new(),
        });

        let diagnostics = validate_document_diagnostics(&doc);
        assert!(diagnostics.iter().any(|d| {
            d.severity == ErrorSeverity::Warning
                && d.message
                    .contains("Unknown OPTIONS key 'dry_run'. Supported keys: timeout, retry, retry_delay, no_retry, compression")
        }));
    }

    #[test]
    fn test_validate_options_timeout_invalid_error() {
        let mut doc = create_test_document();
        let mut options = std::collections::HashMap::new();
        options.insert("timeout".to_string(), "0".to_string());
        doc.sections.push(Section {
            section_type: SectionType::Options,
            content: SectionContent::KeyValues(options),
            inline_options: InlineOptions::default(),
            raw_content: "timeout: 0".to_string(),
            start_line: 5,
            end_line: 6,
            attributes: Vec::new(),
        });
        doc.sections.push(Section {
            section_type: SectionType::Response,
            content: SectionContent::Json(serde_json::json!({"result": "ok"})),
            inline_options: InlineOptions::default(),
            raw_content: "{\"result\": \"ok\"}".to_string(),
            start_line: 7,
            end_line: 8,
            attributes: Vec::new(),
        });

        let diagnostics = validate_document_diagnostics(&doc);
        assert!(diagnostics.iter().any(|d| {
            d.severity == ErrorSeverity::Error
                && d.message
                    .contains("OPTIONS.timeout must be a positive integer")
        }));
    }

    #[test]
    fn test_validate_options_snake_case_keys_are_supported() {
        let mut doc = create_test_document();
        let mut options = std::collections::HashMap::new();
        options.insert("timeout".to_string(), "5".to_string());
        options.insert("retry".to_string(), "2".to_string());
        options.insert("retry_delay".to_string(), "0.5".to_string());
        options.insert("no_retry".to_string(), "false".to_string());
        options.insert("compression".to_string(), "gzip".to_string());
        doc.sections.push(Section {
            section_type: SectionType::Options,
            content: SectionContent::KeyValues(options),
            inline_options: InlineOptions::default(),
            raw_content: "".to_string(),
            start_line: 5,
            end_line: 8,
            attributes: Vec::new(),
        });
        doc.sections.push(Section {
            section_type: SectionType::Response,
            content: SectionContent::Json(serde_json::json!({"result": "ok"})),
            inline_options: InlineOptions::default(),
            raw_content: "{\"result\": \"ok\"}".to_string(),
            start_line: 9,
            end_line: 10,
            attributes: Vec::new(),
        });

        let diagnostics = validate_document_diagnostics(&doc);
        assert!(
            !diagnostics
                .iter()
                .any(|d| d.message.contains("Unknown OPTIONS key"))
        );
        assert!(
            !diagnostics
                .iter()
                .any(|d| d.severity == ErrorSeverity::Error)
        );
    }

    #[test]
    fn test_validate_options_compression_invalid_error() {
        let mut doc = create_test_document();
        let mut options = std::collections::HashMap::new();
        options.insert("compression".to_string(), "brotli".to_string());
        doc.sections.push(Section {
            section_type: SectionType::Options,
            content: SectionContent::KeyValues(options),
            inline_options: InlineOptions::default(),
            raw_content: "compression: brotli".to_string(),
            start_line: 5,
            end_line: 6,
            attributes: Vec::new(),
        });
        doc.sections.push(Section {
            section_type: SectionType::Response,
            content: SectionContent::Json(serde_json::json!({"result": "ok"})),
            inline_options: InlineOptions::default(),
            raw_content: "{\"result\": \"ok\"}".to_string(),
            start_line: 7,
            end_line: 8,
            attributes: Vec::new(),
        });

        let diagnostics = validate_document_diagnostics(&doc);
        assert!(diagnostics.iter().any(|d| {
            d.severity == ErrorSeverity::Error
                && d.message
                    .contains("OPTIONS.compression must be one of: none, gzip")
        }));
    }

    #[test]
    fn test_validate_options_kebab_case_keys_deprecated_warning() {
        let mut doc = create_test_document();
        let mut options = std::collections::HashMap::new();
        options.insert("retry-delay".to_string(), "0.3".to_string());
        options.insert("no-retry".to_string(), "false".to_string());
        doc.sections.push(Section {
            section_type: SectionType::Options,
            content: SectionContent::KeyValues(options),
            inline_options: InlineOptions::default(),
            raw_content: "retry-delay: 0.3\nno-retry: false".to_string(),
            start_line: 5,
            end_line: 7,
            attributes: Vec::new(),
        });
        doc.sections.push(Section {
            section_type: SectionType::Response,
            content: SectionContent::Json(serde_json::json!({"result": "ok"})),
            inline_options: InlineOptions::default(),
            raw_content: "{\"result\": \"ok\"}".to_string(),
            start_line: 8,
            end_line: 9,
            attributes: Vec::new(),
        });

        let diagnostics = validate_document_diagnostics(&doc);
        assert!(diagnostics.iter().any(|d| {
            d.severity == ErrorSeverity::Warning
                && d.message.contains("OPTIONS.retry-delay is deprecated")
        }));
        assert!(diagnostics.iter().any(|d| {
            d.severity == ErrorSeverity::Warning
                && d.message.contains("OPTIONS.no-retry is deprecated")
        }));
    }

    #[test]
    fn test_validate_options_no_retry_retry_conflict_warning() {
        let mut doc = create_test_document();
        let mut options = std::collections::HashMap::new();
        options.insert("retry".to_string(), "3".to_string());
        options.insert("no_retry".to_string(), "true".to_string());
        doc.sections.push(Section {
            section_type: SectionType::Options,
            content: SectionContent::KeyValues(options),
            inline_options: InlineOptions::default(),
            raw_content: "retry: 3\nno_retry: true".to_string(),
            start_line: 5,
            end_line: 7,
            attributes: Vec::new(),
        });
        doc.sections.push(Section {
            section_type: SectionType::Response,
            content: SectionContent::Json(serde_json::json!({"result": "ok"})),
            inline_options: InlineOptions::default(),
            raw_content: "{\"result\": \"ok\"}".to_string(),
            start_line: 8,
            end_line: 9,
            attributes: Vec::new(),
        });

        let diagnostics = validate_document_diagnostics(&doc);
        assert!(diagnostics.iter().any(|d| {
            d.severity == ErrorSeverity::Warning
                && d.message
                    .contains("OPTIONS.no_retry=true conflicts with OPTIONS.retry>0")
        }));
    }

    #[test]
    fn test_validate_attribute_retry_delay_kebab_case_deprecated_warning() {
        let mut doc = create_test_document();
        doc.sections.push(Section {
            section_type: SectionType::Request,
            content: SectionContent::Json(serde_json::json!({"id": 1})),
            inline_options: InlineOptions::default(),
            raw_content: "{\"id\":1}".to_string(),
            start_line: 5,
            end_line: 6,
            attributes: vec![GctfAttribute::new("retry-delay", "0.2")],
        });
        doc.sections.push(Section {
            section_type: SectionType::Response,
            content: SectionContent::Json(serde_json::json!({"result": "ok"})),
            inline_options: InlineOptions::default(),
            raw_content: "{\"result\": \"ok\"}".to_string(),
            start_line: 7,
            end_line: 8,
            attributes: Vec::new(),
        });

        let diagnostics = validate_document_diagnostics(&doc);
        assert!(diagnostics.iter().any(|d| {
            d.severity == ErrorSeverity::Warning
                && d.message.contains("Attribute #[retry-delay] is deprecated")
        }));
    }

    #[test]
    fn test_validate_bench_dynamic_percentile_key_ok() {
        let mut doc = create_test_document();
        let mut bench = std::collections::HashMap::new();
        bench.insert(
            "thresholds.latency_ms.p(99.9)".to_string(),
            "<300".to_string(),
        );
        bench.insert("thresholds.p(95)".to_string(), "<120".to_string());
        doc.sections.insert(
            0,
            Section {
                section_type: SectionType::Bench,
                content: SectionContent::KeyValues(bench),
                inline_options: InlineOptions::default(),
                raw_content: String::new(),
                start_line: 0,
                end_line: 2,
                attributes: Vec::new(),
            },
        );

        let diagnostics = validate_document_diagnostics(&doc);
        assert!(!diagnostics.iter().any(
            |d| d.message.contains("Invalid percentile") || d.message.contains("range (0,100)")
        ));
    }

    #[test]
    fn test_validate_bench_dynamic_percentile_key_invalid_range() {
        let mut doc = create_test_document();
        let mut bench = std::collections::HashMap::new();
        bench.insert("thresholds.p(120)".to_string(), "<300".to_string());
        doc.sections.insert(
            0,
            Section {
                section_type: SectionType::Bench,
                content: SectionContent::KeyValues(bench),
                inline_options: InlineOptions::default(),
                raw_content: String::new(),
                start_line: 0,
                end_line: 2,
                attributes: Vec::new(),
            },
        );

        let diagnostics = validate_document_diagnostics(&doc);
        assert!(
            diagnostics
                .iter()
                .any(|d| d.message.contains("must be in range (0,100)"))
        );
    }

    #[test]
    fn test_validate_bench_threshold_expression_invalid() {
        let mut doc = create_test_document();
        let mut bench = std::collections::HashMap::new();
        bench.insert("thresholds.p(95)".to_string(), "~120".to_string());
        doc.sections.insert(
            0,
            Section {
                section_type: SectionType::Bench,
                content: SectionContent::KeyValues(bench),
                inline_options: InlineOptions::default(),
                raw_content: String::new(),
                start_line: 0,
                end_line: 2,
                attributes: Vec::new(),
            },
        );

        let diagnostics = validate_document_diagnostics(&doc);
        assert!(
            diagnostics
                .iter()
                .any(|d| d.message.contains("invalid expression"))
        );
    }

    #[test]
    fn test_validate_bench_load_schedule_and_progress_keys() {
        let mut doc = create_test_document();
        let mut bench = std::collections::HashMap::new();
        bench.insert("load_schedule".to_string(), "step".to_string());
        bench.insert("load_start".to_string(), "10".to_string());
        bench.insert("load_step".to_string(), "5".to_string());
        bench.insert("load_end".to_string(), "40".to_string());
        bench.insert("load_step_duration".to_string(), "3s".to_string());
        bench.insert("load_max_duration".to_string(), "30s".to_string());
        bench.insert("progress_interval".to_string(), "2s".to_string());
        doc.sections.insert(
            0,
            Section {
                section_type: SectionType::Bench,
                content: SectionContent::KeyValues(bench),
                inline_options: InlineOptions::default(),
                raw_content: String::new(),
                start_line: 0,
                end_line: 2,
                attributes: Vec::new(),
            },
        );

        let diagnostics = validate_document_diagnostics(&doc);
        assert!(!diagnostics.iter().any(|d| {
            d.message.contains("Unknown BENCH key")
                || d.message.contains("BENCH.load_schedule must be one of")
        }));
    }

    #[test]
    fn test_validate_bench_hyphenated_keys_are_unknown() {
        let mut doc = create_test_document();
        let mut bench = std::collections::HashMap::new();
        bench.insert("load-schedule".to_string(), "line".to_string());
        bench.insert("load-step-duration".to_string(), "2s".to_string());
        bench.insert("progress-interval".to_string(), "1s".to_string());
        bench.insert("assert-mode".to_string(), "sampled".to_string());
        bench.insert("duration-stop".to_string(), "wait".to_string());
        doc.sections.insert(
            0,
            Section {
                section_type: SectionType::Bench,
                content: SectionContent::KeyValues(bench),
                inline_options: InlineOptions::default(),
                raw_content: String::new(),
                start_line: 0,
                end_line: 2,
                attributes: Vec::new(),
            },
        );

        let diagnostics = validate_document_diagnostics(&doc);
        assert!(diagnostics.iter().any(|d| {
            d.message.contains("Unknown BENCH key 'load-schedule'")
                && d.message.contains("did you mean 'load_schedule'")
        }));
    }

    #[test]
    fn test_validate_bench_snake_case_keys_no_deprecation_warning() {
        let mut doc = create_test_document();
        let mut bench = std::collections::HashMap::new();
        bench.insert("load_schedule".to_string(), "line".to_string());
        bench.insert("progress_interval".to_string(), "1s".to_string());
        doc.sections.insert(
            0,
            Section {
                section_type: SectionType::Bench,
                content: SectionContent::KeyValues(bench),
                inline_options: InlineOptions::default(),
                raw_content: String::new(),
                start_line: 0,
                end_line: 2,
                attributes: Vec::new(),
            },
        );

        let diagnostics = validate_document_diagnostics(&doc);
        assert!(
            !diagnostics
                .iter()
                .any(|d| d.message.contains("is deprecated"))
        );
    }

    #[test]
    fn test_validate_bench_unknown_key_typo_suggestion() {
        let mut doc = create_test_document();
        let mut bench = std::collections::HashMap::new();
        bench.insert("load_shedule".to_string(), "step".to_string());
        doc.sections.insert(
            0,
            Section {
                section_type: SectionType::Bench,
                content: SectionContent::KeyValues(bench),
                inline_options: InlineOptions::default(),
                raw_content: String::new(),
                start_line: 0,
                end_line: 2,
                attributes: Vec::new(),
            },
        );

        let diagnostics = validate_document_diagnostics(&doc);
        assert!(diagnostics.iter().any(|d| {
            d.message.contains("Unknown BENCH key 'load_shedule'")
                && d.message.contains("did you mean 'load_schedule'")
        }));
    }

    #[test]
    fn test_validation_error_debug() {
        let error = ValidationError {
            message: "test error".to_string(),
            line: Some(10),
            severity: ErrorSeverity::Error,
        };
        let debug_str = format!("{:?}", error);
        assert!(debug_str.contains("ValidationError"));
        assert!(debug_str.contains("test error"));
    }

    #[test]
    fn test_error_severity_serialize() {
        let error = ErrorSeverity::Error;
        let json = serde_json::to_string(&error).unwrap();
        assert_eq!(json, "\"error\"");

        let warning = ErrorSeverity::Warning;
        let json = serde_json::to_string(&warning).unwrap();
        assert_eq!(json, "\"warning\"");
    }
}
