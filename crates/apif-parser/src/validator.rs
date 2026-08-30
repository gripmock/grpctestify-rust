use anyhow::{Result, bail};
use apif_ast::*;
use serde::Serialize;

#[derive(Debug, Clone, Serialize)]
pub struct ValidationError {
    pub message: String,
    pub line: Option<usize>,
    pub severity: ErrorSeverity,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum ErrorSeverity {
    Error,
    Warning,
    Info,
}

pub const BENCH_NUMERIC_KEYS: &[&str] = &[
    "concurrency",
    "requests",
    "max_rps",
    "connections",
    "cpus",
    "skip_first",
    "load_start",
    "load_step",
    "load_end",
    "load_midpoint",
    "load_amplitude",
    "load_frequency",
    "load_spike_target",
    "load_spike_after",
    "load_spike_duration",
    "concurrency_start",
    "concurrency_end",
    "concurrency_step",
];

pub const BENCH_DURATION_KEYS: &[&str] = &[
    "max_duration",
    "request_timeout",
    "connect_timeout",
    "keepalive",
    "ramp_up",
    "warmup",
    "duration",
    "cache_ttl",
    "cool_down",
    "load_step_duration",
    "load_max_duration",
    "progress_interval",
    "concurrency_step_duration",
];

pub const BENCH_MODE_VALUES: &[&str] = &["fixed", "stepping", "adaptive", "closed", "open"];
pub const BENCH_CONCURRENCY_SCHEDULE_VALUES: &[&str] = &["const", "step", "line"];
pub const BENCH_LOAD_SCHEDULE_VALUES: &[&str] =
    &["const", "step", "line", "sine", "spike", "custom"];
pub const BENCH_DURATION_STOP_VALUES: &[&str] = &["close", "wait", "ignore"];
pub const BENCH_ASSERT_MODE_VALUES: &[&str] =
    &["full", "sampled", "off", "fail_fast", "collect_all", "skip"];
pub const BENCH_CACHE_VALUES: &[&str] = &["on", "off", "refresh", "true", "false", "1", "0"];

pub const TLS_KEYS: [&str; 8] = [
    "ca_cert",
    "client_cert",
    "client_key",
    "server_name",
    "insecure",
    "ca_file",
    "cert_file",
    "key_file",
];

pub const PROTO_KEYS: [&str; 3] = ["descriptor", "files", "import_paths"];

fn section_label(section_type: SectionType) -> &'static str {
    match section_type {
        SectionType::Tls => "TLS",
        SectionType::Proto => "PROTO",
        other => other.as_str(),
    }
}

fn suggest_from<'a>(key: &str, known: &'a [&'a str]) -> Option<&'a str> {
    let lower = key.to_ascii_lowercase().replace('-', "_");
    if let Some(found) = known.iter().find(|k| k.to_ascii_lowercase() == lower) {
        return Some(found);
    }
    let contained = known
        .iter()
        .filter(|k| {
            let candidate = k.to_ascii_lowercase();
            candidate.contains(&lower) || lower.contains(&candidate)
        })
        .min_by_key(|k| k.len().abs_diff(lower.len()));
    if let Some(found) = contained {
        return Some(found);
    }
    let mut best: Option<(&'a str, usize)> = None;
    for candidate in known {
        let Some(score) = bounded_edit_distance(&lower, &candidate.to_ascii_lowercase(), 3) else {
            continue;
        };
        match best {
            Some((_, best_score)) if score >= best_score => {}
            _ => best = Some((candidate, score)),
        }
    }
    best.map(|(k, _)| k)
}

pub fn supported_bench_keys() -> Vec<&'static str> {
    let mut keys = Vec::new();
    keys.extend_from_slice(BENCH_NUMERIC_KEYS);
    keys.extend_from_slice(BENCH_DURATION_KEYS);
    keys.push("no_assert");
    keys.push("count_errors_in_latency");
    keys.push("mode");
    keys.push("profile");
    keys.push("load_schedule");
    keys.push("concurrency_schedule");
    keys.push("name");
    keys.push("assert_mode");
    keys.push("duration_stop");
    keys.push("sample_rate");
    keys.push("cache");
    keys.push("latency_percentiles");
    keys.push("warmup_mode");
    keys.push("load_profile");
    keys.push("sources");
    keys.push("thresholds.*");
    keys.sort_unstable();
    keys.dedup();
    keys
}

pub fn is_allowed_value(value: &str, allowed: &[&str]) -> bool {
    let normalized = value.trim().to_ascii_lowercase();
    allowed.iter().any(|v| *v == normalized)
}

pub fn allowed_values_message(allowed: &[&str]) -> String {
    allowed.join(", ")
}

pub fn canonical_bench_key(key: &str) -> Option<&'static str> {
    for canonical in supported_bench_keys() {
        if canonical == "thresholds.*" {
            continue;
        }
        if key == canonical {
            return Some(canonical);
        }
    }
    None
}

pub fn threshold_hint(raw_key: &str) -> Option<String> {
    let key = raw_key.trim().to_ascii_lowercase().replace('-', "_");
    if key.is_empty() || key.starts_with("thresholds") {
        return None;
    }
    const METRICS: [&str; 12] = [
        "rps",
        "rps_observed",
        "throughput",
        "passed",
        "failed",
        "pass_rate",
        "pass_rate_pct",
        "fail_rate",
        "fail_rate_pct",
        "error_rate",
        "error_rate_pct",
        "slowest_ms",
    ];
    if METRICS.contains(&key.as_str()) || key == "max_ms" || key == "total_ns" {
        return Some(format!("thresholds.{key}"));
    }
    let inner = percentile_of(&key)?;
    Some(format!("thresholds.latency_ms.p({inner})"))
}

fn percentile_of(key: &str) -> Option<String> {
    let rest = key
        .strip_prefix("latency_ms.p")
        .or_else(|| key.strip_prefix("latency_ns.p"))
        .or_else(|| key.strip_prefix('p'))?;
    let rest = match rest.strip_prefix('(') {
        Some(paren) => paren.strip_suffix(')')?,
        None => rest,
    };
    let digits = rest
        .strip_suffix("_ms")
        .or_else(|| rest.strip_suffix("ms"))
        .unwrap_or(rest);
    if digits.is_empty() || !digits.chars().all(|c| c.is_ascii_digit() || c == '.') {
        return None;
    }
    let value: f64 = digits.parse().ok()?;
    if !(value > 0.0 && value < 100.0) {
        return None;
    }
    Some(digits.to_string())
}

pub fn threshold_line(key: &str, value: &str) -> Option<String> {
    let target = threshold_hint(key)?;
    let trimmed = value.trim();
    if trimmed.is_empty() || is_valid_threshold_expr(trimmed) {
        return None;
    }
    if trimmed.parse::<f64>().is_err() {
        return None;
    }
    let metric = target.trim_start_matches("thresholds.");
    let floor = matches!(
        metric,
        "rps" | "rps_observed" | "throughput" | "passed" | "pass_rate" | "pass_rate_pct"
    );
    Some(format!(
        "{target}: {}{trimmed}",
        if floor { '>' } else { '<' }
    ))
}

pub fn suggest_options_key(raw_key: &str) -> Option<&'static str> {
    const KEYS: [&str; 6] = [
        "timeout",
        "retry",
        "retry_delay",
        "no_retry",
        "compression",
        "protocol",
    ];
    if raw_key.trim().is_empty() {
        return None;
    }
    suggest_from(raw_key.trim(), &KEYS)
}

pub fn suggest_bench_key(raw_key: &str) -> Option<&'static str> {
    let needle = raw_key.trim().to_ascii_lowercase().replace('-', "_");
    if needle.is_empty() || needle == "thresholds" || needle.starts_with("thresholds.") {
        return None;
    }
    let candidates = supported_bench_keys();
    let mut best: Option<(&'static str, usize)> = None;
    for key in candidates {
        if key == "thresholds.*" {
            continue;
        }
        let key_norm = key.to_ascii_lowercase();
        let Some(score) = bounded_edit_distance(&needle, &key_norm, 3) else {
            continue;
        };
        match best {
            Some((_, best_score)) if score >= best_score => {}
            _ => best = Some((key, score)),
        }
    }
    best.map(|(k, _)| k)
}

fn bounded_edit_distance(a: &str, b: &str, max: usize) -> Option<usize> {
    let a_bytes = a.as_bytes();
    let b_bytes = b.as_bytes();
    if a_bytes == b_bytes {
        return Some(0);
    }
    if a_bytes.len().abs_diff(b_bytes.len()) > max {
        return None;
    }
    let mut prev: Vec<usize> = (0..=b_bytes.len()).collect();
    let mut curr = vec![0; b_bytes.len() + 1];
    for (i, &ac) in a_bytes.iter().enumerate() {
        curr[0] = i + 1;
        let mut row_min = curr[0];
        for (j, &bc) in b_bytes.iter().enumerate() {
            let cost = if ac == bc { 0 } else { 1 };
            let v = (prev[j + 1] + 1).min(curr[j] + 1).min(prev[j] + cost);
            curr[j + 1] = v;
            row_min = row_min.min(v);
        }
        if row_min > max {
            return None;
        }
        std::mem::swap(&mut prev, &mut curr);
    }
    let dist = prev[b_bytes.len()];
    if dist <= max { Some(dist) } else { None }
}

pub fn validate_document_diagnostics(document: &GctfDocument) -> Vec<ValidationError> {
    let mut errors = Vec::new();

    validate_required_sections(document, &mut errors);
    validate_conflicts(document, &mut errors);
    validate_content(document, &mut errors);
    validate_structure(document, &mut errors);

    errors
}

fn located(error: &ValidationError) -> String {
    match error.line {
        Some(line) => format!("Line {}: {}", line + 1, error.message),
        None => error.message.clone(),
    }
}

pub fn validate_document(document: &GctfDocument) -> Result<Vec<ValidationError>> {
    let errors = validate_document_diagnostics(document);
    let has_errors = errors.iter().any(|e| e.severity == ErrorSeverity::Error);

    if has_errors {
        let error_messages: Vec<String> = errors
            .iter()
            .filter(|e| e.severity == ErrorSeverity::Error)
            .map(located)
            .collect();

        bail!("Validation failed:\n{}", error_messages.join("\n"));
    }

    Ok(errors)
}

pub fn validate_document_chain_diagnostics(document: &GctfDocument) -> Vec<ValidationError> {
    let mut all = Vec::new();
    let mut address_seen = [false, false];
    let mut missing_said = [false, false];
    for (idx, doc) in document.iter_chain().enumerate() {
        let mut errors = validate_document_diagnostics(doc);
        let which = usize::from(doc.transport() == apif_ast::Transport::Http);
        let has_own_address = doc.get_address(None).is_some();
        if address_seen[which] || missing_said[which] {
            errors.retain(|e| !e.message.starts_with("ADDRESS section missing"));
        } else {
            missing_said[which] = errors
                .iter()
                .any(|e| e.message.starts_with("ADDRESS section missing"));
        }
        address_seen[which] = address_seen[which] || has_own_address;
        if idx > 0 {
            for error in &mut errors {
                error.message = match error.line {
                    Some(line) => {
                        format!(
                            "document {} (line {}): {}",
                            idx + 1,
                            line + 1,
                            error.message
                        )
                    }
                    None => format!("document {}: {}", idx + 1, error.message),
                };
            }
        }
        all.extend(errors);
    }
    all
}

pub fn validate_document_chain(document: &GctfDocument) -> Result<Vec<ValidationError>> {
    let errors = validate_document_chain_diagnostics(document);
    let has_errors = errors.iter().any(|e| e.severity == ErrorSeverity::Error);

    if has_errors {
        let error_messages: Vec<String> = errors
            .iter()
            .filter(|e| e.severity == ErrorSeverity::Error)
            .map(located)
            .collect();

        bail!("Validation failed:\n{}", error_messages.join("\n"));
    }

    Ok(errors)
}

fn validate_required_sections(document: &GctfDocument, errors: &mut Vec<ValidationError>) {
    if document.get_endpoint().is_none() {
        errors.push(ValidationError {
            message: "ENDPOINT section is required".to_string(),
            line: None,
            severity: ErrorSeverity::Error,
        });
    }

    let env_addr = std::env::var("GRPCTESTIFY_ADDRESS").ok();
    let aimed_by_endpoint = document.transport() == crate::ast::Transport::Http
        && document
            .parse_http_endpoint()
            .is_some_and(|(_, path)| path.starts_with("http://") || path.starts_with("https://"));
    if !aimed_by_endpoint && document.get_address(env_addr.as_deref()).is_none() {
        errors.push(ValidationError {

            message: if document.transport() == crate::ast::Transport::Http {
                "ADDRESS section missing — an HTTP call has no default target: name one here, or aim the file with the active environment or $GRPCTESTIFY_ADDRESS"
            } else {
                "ADDRESS section missing — the target comes from the active environment or $GRPCTESTIFY_ADDRESS"
            }
            .to_string(),
            line: None,
            severity: ErrorSeverity::Warning,
        });
    }

    let http = document.transport() == crate::ast::Transport::Http;
    let verifies = |kind: SectionType| {
        document
            .sections_by_type(kind)
            .iter()
            .any(|section| !section.get_skip())
    };
    let has_response = verifies(SectionType::Response);
    let has_error = !http && verifies(SectionType::Error);
    let has_asserts = verifies(SectionType::Asserts);

    if !has_response && !has_error && !has_asserts {
        errors.push(ValidationError {
            message: if http {
                "At least one verification section (RESPONSE or ASSERTS) is required"
            } else {
                "At least one verification section (RESPONSE, ERROR, or ASSERTS) is required"
            }
            .to_string(),
            line: None,
            severity: ErrorSeverity::Error,
        });
    }
}

fn validate_conflicts(document: &GctfDocument, errors: &mut Vec<ValidationError>) {
    if document.has_response_error_conflict() {
        errors.push(ValidationError {
            message: "Cannot have both RESPONSE and ERROR sections".to_string(),
            line: None,
            severity: ErrorSeverity::Error,
        });
    }
}

fn key_line(raw_content: &str, start_line: usize, key: &str) -> usize {
    raw_content
        .lines()
        .position(|line| gctf_tokenizer::tokenize_kv_line(line).is_some_and(|(k, _)| k == key))
        .map_or(start_line, |offset| start_line + 1 + offset)
}

fn validate_content(document: &GctfDocument, errors: &mut Vec<ValidationError>) {
    let endpoint_line = || {
        document
            .first_section(SectionType::Endpoint)
            .map(|s| s.start_line)
    };

    const METHODS: [&str; 15] = [
        "GET",
        "POST",
        "PUT",
        "PATCH",
        "DELETE",
        "HEAD",
        "OPTIONS",
        "TRACE",
        "CONNECT",
        "PROPFIND",
        "PROPPATCH",
        "MKCOL",
        "COPY",
        "MOVE",
        "LOCK",
    ];
    if let Some((method, _)) = document.parse_http_endpoint()
        && !METHODS.contains(&method.as_str())
    {
        let lower = method.to_ascii_lowercase();
        let hint = METHODS
            .iter()
            .filter_map(|m| {
                bounded_edit_distance(&lower, &m.to_ascii_lowercase(), 2).map(|score| (m, score))
            })
            .min_by_key(|(_, score)| *score)
            .map(|(m, _)| format!(" Hint: did you mean '{m}'?"))
            .unwrap_or_default();
        errors.push(ValidationError {
            message: format!(
                "'{method}' is not one of the usual HTTP methods — it is sent as written.{hint}"
            ),
            line: endpoint_line(),
            severity: ErrorSeverity::Warning,
        });
    }

    if let Some(endpoint) = document.get_endpoint() {
        match document.family() {
            apif_ast::Family::Httf => {
                if document.parse_http_endpoint().is_none() {
                    errors.push(ValidationError {
                        message: format!(
                            "Invalid endpoint for an .httf file: {endpoint}. Expected a method and a path, like `POST /v1/users`"
                        ),
                        line: endpoint_line(),
                        severity: ErrorSeverity::Error,
                    });
                }
            }
            apif_ast::Family::Gctf => {
                if !endpoint.contains('/') {
                    errors.push(ValidationError {
                        message: format!(
                            "Invalid endpoint format: {endpoint}. Expected format: package.Service/Method"
                        ),
                        line: endpoint_line(),
                        severity: ErrorSeverity::Error,
                    });
                }
            }
            apif_ast::Family::Apif => {
                if document.parse_http_endpoint().is_none() && !endpoint.contains('/') {
                    errors.push(ValidationError {
                        message: format!(
                            "Invalid endpoint for an .apif file: {endpoint}. Expected either a method and a path, like `POST /v1/users`, or package.Service/Method"
                        ),
                        line: endpoint_line(),
                        severity: ErrorSeverity::Error,
                    });
                }
            }
        }
    }

    if document.transport() == apif_ast::Transport::Http {
        if let Some(section) = document.first_section(SectionType::Proto) {
            errors.push(ValidationError {
                message: "PROTO belongs to a gRPC step — an HTTP request has no descriptors"
                    .to_string(),
                line: Some(section.start_line),
                severity: ErrorSeverity::Error,
            });
        }
        if let Some(section) = document.first_section(SectionType::Error) {
            errors.push(ValidationError {
                message: "ERROR belongs to a gRPC step — an HTTP answer carries a status, and nothing here reads this section: check the failure with @status() in ASSERTS".to_string(),
                line: Some(section.start_line),
                severity: ErrorSeverity::Error,
            });
        }
        if let Some(section) = document.first_section(SectionType::Tls) {
            errors.push(ValidationError {
                message: "TLS belongs to a gRPC step — an HTTP call is aimed by `https://` in ADDRESS, and this section is not read".to_string(),
                line: Some(section.start_line),
                severity: ErrorSeverity::Warning,
            });
        }
        if let Some(section) = document.first_section(SectionType::Bench) {
            errors.push(ValidationError {
                message:
                    "BENCH belongs to a gRPC step — `bench` dials gRPC and does not run this file"
                        .to_string(),
                line: Some(section.start_line),
                severity: ErrorSeverity::Warning,
            });
        }
    }

    if let Some(address) = document.get_address(None)
        && document.transport() == apif_ast::Transport::Grpc
    {
        let address_line = || {
            document
                .first_section(SectionType::Address)
                .map(|s| s.start_line)
        };
        let bare = address
            .trim()
            .trim_start_matches("http://")
            .trim_start_matches("https://")
            .trim_end_matches('/');
        if !address.contains(':') {
            errors.push(ValidationError {
                message: format!(
                    "Invalid address format: {}. Expected format: host:port",
                    address
                ),
                line: address_line(),
                severity: ErrorSeverity::Error,
            });
        } else if let Some((host, rest)) = bare.rsplit_once(':') {
            let (port, path) = match rest.split_once('/') {
                Some((port, path)) => (port, Some(path)),
                None => (rest, None),
            };
            if !host.is_empty()
                && !port.is_empty()
                && !port.contains("{{")
                && !port.parse::<u16>().is_ok_and(|n| n > 0)
            {
                errors.push(ValidationError {
                    message: format!(
                        "Port '{port}' is not one a call can be made to — a port is 1 to 65535"
                    ),
                    line: address_line(),
                    severity: ErrorSeverity::Error,
                });
            }
            if path.is_some_and(|p| !p.is_empty()) {
                errors.push(ValidationError {
                    message: format!(
                        "A gRPC address is a host and a port: the path in '{}' is not dialled",
                        address.trim()
                    ),
                    line: address_line(),
                    severity: ErrorSeverity::Warning,
                });
            }
        }
    }

    for section_type in [
        SectionType::Request,
        SectionType::Response,
        SectionType::Error,
    ] {
        for section in document.sections_by_type(section_type) {
            match &section.content {
                SectionContent::Json(json) => {
                    let is_valid = if section_type == SectionType::Error {
                        json.is_object() || json.is_array() || json.is_string()
                    } else if document.transport() == apif_ast::Transport::Http {
                        true
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
                    if section_type != SectionType::Response && section_type != SectionType::Request
                    {
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
                            message: format!(
                                "{:?} section contains no JSON messages",
                                section_type
                            ),
                            line: Some(section.start_line),
                            severity: ErrorSeverity::Error,
                        });
                    }
                }
                SectionContent::Empty if section_type == SectionType::Error => {
                    errors.push(ValidationError {
                        message: "ERROR section is empty — give it a JSON body, or use `partial` with `{}` to accept any error".to_string(),
                        line: Some(section.start_line),
                        severity: ErrorSeverity::Error,
                    });
                }
                _ => {}
            }
        }
    }

    for section_type in [
        SectionType::RequestHeaders,
        SectionType::Tls,
        SectionType::Proto,
        SectionType::Options,
        SectionType::Bench,
    ] {
        for section in document.sections_by_type(section_type) {
            if let SectionContent::KeyValues(kv) = &section.content {
                for key in kv.keys() {
                    if key.is_empty() {
                        errors.push(ValidationError {
                            message: format!("Empty key in {:?} section", section_type),
                            line: Some(section.start_line),
                            severity: ErrorSeverity::Error,
                        });
                        continue;
                    }

                    let known: &[&str] = match section_type {
                        SectionType::Tls => &TLS_KEYS,
                        SectionType::Proto => &PROTO_KEYS,
                        _ => &[],
                    };
                    if !known.is_empty() && !known.contains(&key.as_str()) {
                        let hint = suggest_from(key, known)
                            .map(|s| format!(" Hint: did you mean '{}'?", s))
                            .unwrap_or_default();
                        errors.push(ValidationError {
                            message: format!(
                                "Unknown {} key '{}'. Supported keys: {}.{}",
                                section_label(section_type),
                                key,
                                known.join(", "),
                                hint
                            ),
                            line: Some(section.start_line),
                            severity: ErrorSeverity::Warning,
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
                            "protocol" => {
                                let normalized = value.trim().to_ascii_lowercase();
                                if !matches!(
                                    normalized.as_str(),
                                    "grpc" | "grpc-web" | "connectrpc"
                                ) {
                                    errors.push(ValidationError {
                                        message: format!(
                                            "OPTIONS.protocol must be one of: grpc, grpc-web, connectrpc (got '{}')",
                                            value
                                        ),
                                        line: Some(key_line(
                                            &section.raw_content,
                                            section.start_line,
                                            key,
                                        )),
                                        severity: ErrorSeverity::Error,
                                    });
                                }
                            }
                            _ => {
                                errors.push(ValidationError {
                                    message: format!(
                                        "Unknown OPTIONS key '{}'. Supported keys: timeout, retry, retry_delay, no_retry, compression, protocol.{}",
                                        key,
                                        suggest_options_key(key)
                                            .map(|meant| format!(" Hint: did you mean '{meant}'?"))
                                            .unwrap_or_default(),
                                    ),
                                    line: Some(key_line(&section.raw_content, section.start_line, key)),
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
                    validate_bench_key_values(kv, section.start_line, &section.raw_content, errors);
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
                }
                "repeat" => {
                    if attr.parse_u32().is_none_or(|v| v == 0) {
                        errors.push(ValidationError {
                            message: format!(
                                "Attribute #[repeat] must be a positive integer, got '{}'",
                                attr.value
                            ),
                            line: Some(section.start_line),
                            severity: ErrorSeverity::Error,
                        });
                    }
                }
                "compression" => {
                    let normalized = attr.value.trim().to_ascii_lowercase();
                    if !matches!(normalized.as_str(), "none" | "gzip") {
                        errors.push(ValidationError {
                            message: format!(
                                "Attribute #[compression] must be one of: none, gzip (got '{}')",
                                attr.value
                            ),
                            line: Some(section.start_line),
                            severity: ErrorSeverity::Error,
                        });
                    }
                }
                "name" | "tag" | "owner" | "summary" => {}
                _ => {
                    const ATTRIBUTES: [&str; 11] = [
                        "skip",
                        "timeout",
                        "retry",
                        "retry_delay",
                        "no_retry",
                        "repeat",
                        "compression",
                        "name",
                        "tag",
                        "owner",
                        "summary",
                    ];
                    let hint = suggest_from(&attr.name, &ATTRIBUTES)
                        .map(|s| format!(" Hint: did you mean '#[{}]'?", s))
                        .unwrap_or_default();
                    errors.push(ValidationError {
                        message: format!(
                            "Unknown attribute '#[{}]'. Supported attributes: skip, timeout, retry, retry_delay, no_retry, repeat, compression, name, tag, owner, summary.{hint}",
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

    let base = std::path::Path::new(&document.file_path)
        .parent()
        .map(std::path::Path::to_path_buf);
    if let Some(base) = base.filter(|dir| !dir.as_os_str().is_empty()) {
        let mut named: Vec<(&str, String, usize)> = Vec::new();
        for section in document.sections_by_type(SectionType::Proto) {
            if let SectionContent::KeyValues(kv) = &section.content {
                for (key, value) in kv {
                    if key == "descriptor" || key == "files" || key == "import_paths" {
                        for part in value.split(',') {
                            named.push(("PROTO", part.trim().to_string(), section.start_line));
                        }
                    }
                }
            }
        }
        for section in document.sections_by_type(SectionType::Tls) {
            if let SectionContent::KeyValues(kv) = &section.content {
                for (key, value) in kv {
                    if matches!(
                        key.as_str(),
                        "ca_cert"
                            | "ca_file"
                            | "ca"
                            | "client_cert"
                            | "cert_file"
                            | "cert"
                            | "client_key"
                            | "key_file"
                            | "key"
                    ) {
                        named.push(("TLS", value.trim().to_string(), section.start_line));
                    }
                }
            }
        }
        for (section, value, line) in named {
            if value.is_empty() || value.contains("{{") {
                continue;
            }
            let resolved = base.join(&value);
            if !resolved.exists() {
                errors.push(ValidationError {
                    message: if resolved.to_string_lossy() == value {
                        format!("{section} names {value}, and there is nothing there")
                    } else {
                        format!(
                            "{section} names {value}, and there is nothing at {}",
                            resolved.display()
                        )
                    },
                    line: Some(line),
                    severity: ErrorSeverity::Warning,
                });
            }
        }
    }

    for section_type in [
        SectionType::RequestHeaders,
        SectionType::Tls,
        SectionType::Proto,
        SectionType::Options,
    ] {
        for section in document.sections_by_type(section_type) {
            for (offset, line) in section.raw_content.lines().enumerate() {
                let trimmed = line.trim();
                if trimmed.is_empty() || trimmed.starts_with('#') || trimmed.starts_with("//") {
                    continue;
                }
                if apif_ast::gctf_tokenizer::tokenize_kv_line(line).is_none() {
                    errors.push(ValidationError {
                        message: format!(
                            "{} line is not a `key: value` pair, so it is dropped: {trimmed}",
                            section_type.as_str()
                        ),
                        line: Some(section.start_line + 1 + offset),
                        severity: ErrorSeverity::Error,
                    });
                }
            }
        }
    }

    for section in document.sections_by_type(SectionType::Extract) {
        for (offset, line) in section.raw_content.lines().enumerate() {
            let trimmed = line.trim();
            if trimmed.is_empty() || trimmed.starts_with('#') || trimmed.starts_with("//") {
                continue;
            }
            let binds = apif_ast::gctf_tokenizer::tokenize_extract_line(line)
                .and_then(|(name, value)| crate::ternary_ast::ExtractVar::parse_raw(&name, &value))
                .is_some();
            if !binds {
                errors.push(ValidationError {
                    message: format!(
                        "EXTRACT line binds nothing — write `name = filter`: {trimmed}"
                    ),
                    line: Some(section.start_line + 1 + offset),
                    severity: ErrorSeverity::Error,
                });
            }
        }
    }

    for section in document.sections_by_type(SectionType::Asserts) {
        if let SectionContent::Assertions(assertions) = &section.content {
            let line_of = |assertion: &str| -> usize {
                section
                    .raw_content
                    .lines()
                    .position(|l| l.trim() == assertion.trim())
                    .map_or(section.start_line, |i| section.start_line + 1 + i)
            };
            for assertion in assertions {
                if assertion.is_empty() {
                    errors.push(ValidationError {
                        message: "Empty assertion found".to_string(),
                        line: Some(line_of(assertion)),
                        severity: ErrorSeverity::Warning,
                    });
                }
                if let Some(op) = apif_ast::assertion_ast::dangling_operator(assertion) {
                    errors.push(ValidationError {
                        message: if op == "|" {
                            format!("Assertion ends on `|` with nothing after it: {assertion}")
                        } else {
                            format!(
                                "Assertion ends on `{op}` with nothing to compare against: {assertion}"
                            )
                        },
                        line: Some(line_of(assertion)),
                        severity: ErrorSeverity::Error,
                    });
                } else if apif_ast::assertion_ast::reads_nothing(assertion) {
                    errors.push(ValidationError {
                        message: format!(
                            "Assertion reads nothing from the answer, so it passes whatever comes back: {assertion}"
                        ),
                        line: Some(line_of(assertion)),
                        severity: ErrorSeverity::Error,
                    });
                }
            }
        }
    }
}

fn validate_bench_key_values(
    kv: &crate::ast::OrderedStringMap,
    start_line: usize,
    raw_content: &str,
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
            "concurrency_schedule" => {
                if !is_allowed_value(value, BENCH_CONCURRENCY_SCHEDULE_VALUES) {
                    errors.push(ValidationError {
                        message: format!(
                            "BENCH.concurrency_schedule must be one of: {} (got '{}')",
                            allowed_values_message(BENCH_CONCURRENCY_SCHEDULE_VALUES),
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
                if value.trim().replace('_', "").parse::<u64>().is_err() {
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
            "warmup_mode" => {
                let normalized = value.trim().to_ascii_lowercase();
                if normalized != "warmup" && normalized != "dry_run" {
                    errors.push(ValidationError {
                        message: format!(
                            "BENCH.warmup_mode must be 'warmup' or 'dry_run', got '{}'",
                            value
                        ),
                        line: Some(start_line),
                        severity: ErrorSeverity::Error,
                    });
                }
            }
            "name" | "profile" | "sources" | "load_profile" => {}
            _ => {
                if key == "thresholds" || key.starts_with("thresholds.") {
                    validate_bench_threshold_key(
                        key,
                        value,
                        key_line(raw_content, start_line, key),
                        errors,
                    );
                } else {
                    let hint = canonical_bench_key(key)
                        .filter(|canonical| *canonical != key)
                        .map(|canonical| format!(" Hint: use canonical key '{}'.", canonical))
                        .or_else(|| {
                            threshold_line(key, value)
                                .or_else(|| threshold_hint(key))
                                .map(|suggested| format!(" Hint: did you mean '{}'?", suggested))
                        })
                        .or_else(|| {
                            suggest_bench_key(key)
                                .map(|suggested| format!(" Hint: did you mean '{}'?", suggested))
                        })
                        .unwrap_or_default();
                    errors.push(ValidationError {
                        message: format!(
                            "Unknown BENCH key '{}'. Supported keys: {}{}",
                            key, supported_keys_message, hint
                        ),
                        line: Some(key_line(raw_content, start_line, key)),
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
    let unit = trimmed
        .strip_suffix("ms")
        .or_else(|| trimmed.strip_suffix("s"))
        .or_else(|| trimmed.strip_suffix("m"))
        .or_else(|| trimmed.strip_suffix("h"))
        .unwrap_or(trimmed);
    if unit.parse::<f64>().is_ok_and(|n| n < 0.0) {
        errors.push(ValidationError {
            message: format!("BENCH.{} must not be negative, got '{}'", key, value),
            line: Some(start_line),
            severity: ErrorSeverity::Error,
        });
        return;
    }
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

fn validate_structure(document: &GctfDocument, errors: &mut Vec<ValidationError>) {
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

    if meta_count > 1 {
        errors.push(ValidationError {
            message: "Only one META section is allowed per file".to_string(),
            line: meta_first_line,
            severity: ErrorSeverity::Error,
        });
    }

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

    validate_section_order(document, errors);

    validate_bench_sources_exist(document, errors);

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
            SectionType::Response => {}
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

pub fn validation_passed(errors: &[ValidationError]) -> bool {
    !errors.iter().any(|e| e.severity == ErrorSeverity::Error)
}

fn validate_section_order(document: &GctfDocument, errors: &mut Vec<ValidationError>) {
    use SectionType::*;
    let mut seen: Vec<SectionType> = Vec::new();
    let body_required = document.transport() == apif_ast::Transport::Grpc;

    for section in &document.sections {
        let st = &section.section_type;
        if body_required && matches!(st, Response | Error | Asserts) && !seen.contains(&Request) {
            errors.push(ValidationError {
                message: format!(
                    "{:?} section at line {} appears before any REQUEST section",
                    st, section.start_line
                ),
                line: Some(section.start_line),
                severity: ErrorSeverity::Warning,
            });
        }
        if matches!(st, Extract)
            && !seen.contains(&Response)
            && !seen.contains(&Error)
            && !seen.contains(&Asserts)
        {
            errors.push(ValidationError {
                message: format!(
                    "EXTRACT section at line {} appears before RESPONSE, ERROR, or ASSERTS",
                    section.start_line
                ),
                line: Some(section.start_line),
                severity: ErrorSeverity::Warning,
            });
        }
        seen.push(*st);
    }
}

fn has_cycle<'a>(
    adj: &std::collections::BTreeMap<&'a str, Vec<&'a str>>,
    node: &'a str,
    visited: &mut std::collections::BTreeSet<&'a str>,
    stack: &mut std::collections::BTreeSet<&'a str>,
) -> bool {
    if stack.contains(node) {
        return true;
    }
    if visited.contains(node) {
        return false;
    }
    visited.insert(node);
    stack.insert(node);
    if let Some(neighbors) = adj.get(node) {
        for n in neighbors {
            if has_cycle(adj, n, visited, stack) {
                return true;
            }
        }
    }
    stack.remove(node);
    false
}

#[derive(serde::Deserialize)]
struct SourceConfig {
    file: String,
    #[serde(default)]
    name: Option<String>,
    #[serde(default)]
    indexed_by: Option<Vec<String>>,
}

fn validate_bench_sources_exist(document: &GctfDocument, errors: &mut Vec<ValidationError>) {
    for section in &document.sections {
        if section.section_type != SectionType::Bench {
            continue;
        }
        let bench_content = match &section.content {
            SectionContent::KeyValues(kv) => kv,
            _ => continue,
        };
        let Some(sources_yaml) = bench_content.get("sources") else {
            continue;
        };
        match serde_yaml_ng::from_str::<Vec<SourceConfig>>(sources_yaml) {
            Ok(ref defs) => {
                for def in defs {
                    let resolved = std::path::Path::new(&document.file_path)
                        .parent()
                        .unwrap_or(std::path::Path::new("."))
                        .join(&def.file);
                    if !resolved.exists() {
                        errors.push(ValidationError {
                            message: format!(
                                "BENCH data source file not found: {} (resolved: {})",
                                def.file,
                                resolved.display()
                            ),
                            line: Some(section.start_line),
                            severity: ErrorSeverity::Warning,
                        });
                    }
                }

                if defs.len() > 1 {
                    let mut adj: std::collections::BTreeMap<&str, Vec<&str>> =
                        std::collections::BTreeMap::new();
                    for def in defs {
                        let name = def.name.as_deref().unwrap_or("primary");
                        if let Some(idx) = &def.indexed_by {
                            let cols: Vec<&str> = idx.iter().map(|s| s.as_str()).collect();
                            for col in cols {
                                if let Some(target) = col.strip_prefix('@') {
                                    adj.entry(name).or_default().push(target);
                                }
                            }
                        }
                    }
                    let mut visited: std::collections::BTreeSet<&str> =
                        std::collections::BTreeSet::new();
                    let mut stack: std::collections::BTreeSet<&str> =
                        std::collections::BTreeSet::new();
                    for node in adj.keys().copied().collect::<Vec<_>>() {
                        if has_cycle(&adj, node, &mut visited, &mut stack) {
                            errors.push(ValidationError {
                                message: format!(
                                    "Circular dimension join detected involving source '{}'",
                                    node
                                ),
                                line: Some(section.start_line),
                                severity: ErrorSeverity::Error,
                            });
                        }
                    }
                }
            }
            Err(_) => {}
        }
    }
}

#[cfg(test)]
mod threshold_hint_tests {
    use super::{threshold_hint, threshold_line};

    #[test]
    fn a_percentile_is_a_latency_budget_in_milliseconds() {
        assert_eq!(
            threshold_hint("p95_ms").as_deref(),
            Some("thresholds.latency_ms.p(95)")
        );
        assert_eq!(
            threshold_hint("p(99)").as_deref(),
            Some("thresholds.latency_ms.p(99)")
        );
        assert_eq!(
            threshold_hint("p50").as_deref(),
            Some("thresholds.latency_ms.p(50)")
        );
    }

    #[test]
    fn a_named_metric_keeps_its_name() {
        assert_eq!(threshold_hint("rps").as_deref(), Some("thresholds.rps"));
        assert_eq!(
            threshold_hint("error_rate_pct").as_deref(),
            Some("thresholds.error_rate_pct")
        );
        assert_eq!(
            threshold_hint("failed").as_deref(),
            Some("thresholds.failed")
        );
    }

    #[test]
    fn a_bare_number_is_written_as_the_comparison_it_has_to_be() {
        assert_eq!(
            threshold_line("p95_ms", "0.001").as_deref(),
            Some("thresholds.latency_ms.p(95): <0.001")
        );
        assert_eq!(
            threshold_line("error_rate", "1").as_deref(),
            Some("thresholds.error_rate: <1")
        );
    }

    #[test]
    fn throughput_is_a_floor_not_a_budget() {
        assert_eq!(
            threshold_line("rps", "200").as_deref(),
            Some("thresholds.rps: >200")
        );
        assert_eq!(
            threshold_line("pass_rate_pct", "99").as_deref(),
            Some("thresholds.pass_rate_pct: >99")
        );
    }

    #[test]
    fn a_value_that_needs_nothing_is_left_alone() {
        assert!(threshold_line("p95_ms", "<0.5").is_none());
        assert!(threshold_line("rps", ">=200").is_none());
        assert!(threshold_line("p95_ms", "fast").is_none());
        assert!(threshold_line("p95_ms", "").is_none());
        assert!(threshold_line("concurency", "4").is_none());
    }

    #[test]
    fn other_keys_are_not_this_mistake() {
        assert!(threshold_hint("max_rp").is_none());
        assert!(threshold_hint("concurency").is_none());
        assert!(threshold_hint("thresholds.rps").is_none());
        assert!(threshold_hint("p0").is_none());
        assert!(threshold_hint("p100").is_none());
        assert!(threshold_hint("").is_none());
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn errors_of(content: &str, path: &str) -> Vec<String> {
        let doc = crate::parse_gctf_from_str(content, path).expect("parses");
        validate_document_diagnostics(&doc)
            .into_iter()
            .map(|e| e.message)
            .collect()
    }

    const HTTP: &str = "--- ADDRESS ---\nhttps://api.example.com\n\n--- ENDPOINT ---\nPOST /v1/users\n\n--- REQUEST ---\n{}\n\n--- ASSERTS ---\n.ok\n";

    #[test]
    fn an_error_section_on_an_http_step_verifies_nothing() {
        let only_error = "--- ADDRESS ---\nhttps://api.example.com\n\n--- ENDPOINT ---\nGET /v1/a\n\n--- ERROR ---\n{\"code\": 5}\n";
        let said = errors_of(only_error, "a.httf");

        assert!(
            said.iter()
                .any(|m| m.contains("ERROR belongs to a gRPC step")),
            "{said:?}"
        );
        assert!(
            said.iter()
                .any(|m| m.contains("At least one verification section (RESPONSE or ASSERTS)")),
            "{said:?}"
        );
    }

    #[test]
    fn a_section_the_http_transport_never_reads_is_reported() {
        let with_tls = "--- ADDRESS ---\nhttps://api.example.com\n\n--- ENDPOINT ---\nGET /v1/a\n\n--- TLS ---\ninsecure: true\n\n--- ASSERTS ---\n@status() == 200\n";
        let said = errors_of(with_tls, "a.httf");
        assert!(
            said.iter()
                .any(|m| m.contains("TLS belongs to a gRPC step")),
            "{said:?}"
        );

        let with_bench = "--- BENCH ---\nmode: fixed\n\n--- ADDRESS ---\nhttps://api.example.com\n\n--- ENDPOINT ---\nGET /v1/a\n\n--- ASSERTS ---\n@status() == 200\n";
        let said = errors_of(with_bench, "a.httf");
        assert!(
            said.iter()
                .any(|m| m.contains("BENCH belongs to a gRPC step")),
            "{said:?}"
        );
    }

    #[test]
    fn a_grpc_step_keeps_the_sections_it_reads() {
        let content = "--- ADDRESS ---\n127.0.0.1:4770\n\n--- ENDPOINT ---\na.A/One\n\n--- REQUEST_HEADERS ---\nx-token: abc\n\n--- TLS ---\ninsecure: true\n\n--- REQUEST ---\n{}\n\n--- ERROR ---\n{\"code\": 5}\n";
        assert_eq!(errors_of(content, "a.apif"), Vec::<String>::new());
    }

    #[test]
    fn a_transport_the_chain_never_addressed_is_reported() {
        let mixed = "--- ADDRESS ---\n127.0.0.1:4770\n\n--- ENDPOINT ---\na.A/One\n\n--- REQUEST ---\n{}\n\n--- ASSERTS ---\n.ok == true\n\n--- ENDPOINT ---\nGET /v1/orders\n\n--- ASSERTS ---\n@status() == 200\n";
        let doc = crate::parse_gctf_from_str(mixed, "checkout.apif").expect("parses");

        let said: Vec<String> = validate_document_chain_diagnostics(&doc)
            .into_iter()
            .map(|e| e.message)
            .collect();
        assert!(
            said.iter()
                .any(|m| m.contains("document 2")
                    && m.contains("an HTTP call has no default target")),
            "{said:?}"
        );
    }

    #[test]
    fn a_chain_that_addresses_both_transports_is_quiet() {
        let mixed = "--- ADDRESS ---\n127.0.0.1:4770\n\n--- ENDPOINT ---\na.A/One\n\n--- REQUEST ---\n{}\n\n--- ASSERTS ---\n.ok == true\n\n--- ADDRESS ---\nhttps://api.example.com\n\n--- ENDPOINT ---\nGET /v1/orders\n\n--- ASSERTS ---\n@status() == 200\n\n--- ENDPOINT ---\nGET /v1/orders/1\n\n--- ASSERTS ---\n@status() == 200\n";
        let doc = crate::parse_gctf_from_str(mixed, "checkout.apif").expect("parses");

        let said: Vec<String> = validate_document_chain_diagnostics(&doc)
            .into_iter()
            .map(|e| e.message)
            .collect();
        assert!(
            !said.iter().any(|m| m.contains("ADDRESS section missing")),
            "{said:?}"
        );
    }

    #[test]
    fn a_step_of_the_third_family_is_read_as_itself() {
        let mixed = "--- ADDRESS ---\nhttps://api.example.com\n\n--- ENDPOINT ---\nGET /v1/users\n\n--- ASSERTS ---\n@status() == 200\n\n--- ADDRESS ---\n127.0.0.1:4770\n\n--- ENDPOINT ---\nauth.v1.Auth/Login\n\n--- REQUEST ---\n{}\n\n--- ASSERTS ---\n.ok == true\n";
        assert_eq!(errors_of(mixed, "checkout.apif"), Vec::<String>::new());
    }

    #[test]
    fn an_http_step_of_the_third_family_has_no_descriptors() {
        let with_proto = "--- ADDRESS ---\nhttps://api.example.com\n\n--- ENDPOINT ---\nGET /v1/users\n\n--- PROTO ---\ndescriptor: x.bin\n\n--- ASSERTS ---\n@status() == 200\n";
        let said = errors_of(with_proto, "checkout.apif");
        assert!(
            said.iter()
                .any(|m| m.contains("PROTO belongs to a gRPC step")),
            "{said:?}"
        );
    }

    #[test]
    fn an_endpoint_of_the_third_family_that_is_neither_shape() {
        let neither = "--- ADDRESS ---\nhttps://api.example.com\n\n--- ENDPOINT ---\nv1 users now\n\n--- ASSERTS ---\n@status() == 200\n";
        let said = errors_of(neither, "checkout.apif");
        assert!(
            said.iter()
                .any(|m| m.contains("Invalid endpoint for an .apif file")
                    && m.contains("POST /v1/users")
                    && m.contains("package.Service/Method")),
            "{said:?}"
        );
    }

    #[test]
    fn a_whole_file_problem_names_no_line() {
        let doc = crate::parse_gctf_from_str(
            "--- ENDPOINT ---\na.B/C\n\n--- REQUEST ---\n{}\n",
            "bare.gctf",
        )
        .expect("parses");

        let said = validate_document(&doc)
            .expect_err("has an error")
            .to_string();
        assert!(said.contains("At least one verification section"), "{said}");
        assert!(!said.contains("Line 0"), "{said}");
        assert!(!said.contains("Line "), "{said}");
    }

    #[test]
    fn a_located_problem_names_the_editor_line() {
        let doc = crate::parse_gctf_from_str(
            "--- ENDPOINT ---\n\n\n--- ASSERTS ---\n.ok\n",
            "empty-endpoint.gctf",
        )
        .expect("parses");

        let errors = validate_document_diagnostics(&doc);
        let located_lines: Vec<String> = errors
            .iter()
            .filter(|e| e.line.is_some())
            .map(located)
            .collect();
        assert!(!located_lines.is_empty(), "{errors:?}");
        for line in &located_lines {
            assert!(!line.starts_with("Line 0:"), "{line}");
        }
    }

    #[test]
    fn a_chain_step_inherits_the_address_it_was_started_with() {
        let chain = "--- ADDRESS ---\nhttps://api.example.com\n\n--- ENDPOINT ---\nGET /a\n\n--- ASSERTS ---\n@status() == 200\n\n--- ENDPOINT ---\nGET /b\n\n--- ASSERTS ---\n@status() == 200\n";
        let doc = crate::parse_gctf_from_str(chain, "t.httf").expect("parses");
        let messages: Vec<String> = validate_document_chain_diagnostics(&doc)
            .into_iter()
            .map(|e| e.message)
            .collect();
        assert!(
            !messages
                .iter()
                .any(|m| m.contains("ADDRESS section missing")),
            "{messages:?}"
        );
    }

    #[test]
    fn a_missing_address_reads_differently_for_each_family() {
        let http = errors_of(
            "--- ENDPOINT ---\nGET /x\n\n--- ASSERTS ---\n@status() == 200\n",
            "t.httf",
        );
        assert!(
            http.iter()
                .any(|m| m.contains("an HTTP call has no default target")),
            "{http:?}"
        );
        let grpc = errors_of(
            "--- ENDPOINT ---\npkg.S/M\n\n--- REQUEST ---\n{}\n\n--- RESPONSE ---\n{}\n",
            "t.gctf",
        );
        assert!(
            grpc.iter()
                .any(|m| m.starts_with("ADDRESS section missing")
                    && !m.contains("no default target")),
            "{grpc:?}"
        );
    }

    #[test]
    fn a_chain_with_no_address_says_so_once() {
        let chain = "--- ENDPOINT ---\nGET /a\n\n--- ASSERTS ---\n@status() == 200\n\n--- ENDPOINT ---\nGET /b\n\n--- ASSERTS ---\n@status() == 200\n";
        let doc = crate::parse_gctf_from_str(chain, "t.httf").expect("parses");
        let said = validate_document_chain_diagnostics(&doc)
            .into_iter()
            .filter(|e| e.message.contains("ADDRESS section missing"))
            .count();
        assert_eq!(said, 1);
    }

    #[test]
    fn an_http_response_may_be_a_scalar() {
        let quoted = "--- ADDRESS ---\nhttps://api.example.com\n\n--- ENDPOINT ---\nGET /text\n\n--- RESPONSE ---\n\"plain words here\"\n";
        assert_eq!(errors_of(quoted, "t.httf"), Vec::<String>::new());
        let number = "--- ADDRESS ---\nhttps://api.example.com\n\n--- ENDPOINT ---\nGET /n\n\n--- RESPONSE ---\n42\n";
        assert_eq!(errors_of(number, "t.httf"), Vec::<String>::new());
    }

    #[test]
    fn a_grpc_response_still_has_to_be_a_message() {
        let quoted = "--- ADDRESS ---\nh:1\n\n--- ENDPOINT ---\na.B/C\n\n--- RESPONSE ---\n\"plain words here\"\n";
        assert!(
            errors_of(quoted, "t.gctf")
                .iter()
                .any(|m| m.contains("must contain valid JSON object or array"))
        );
    }

    #[test]
    fn an_http_request_without_a_body_is_ordinary() {
        let content = "--- ADDRESS ---\nhttp://api.example.com\n\n--- ENDPOINT ---\nGET /v1/users\n\n--- ASSERTS ---\n.ok\n";
        assert_eq!(errors_of(content, "t.httf"), Vec::<String>::new());
        assert!(
            errors_of(content, "t.gctf")
                .iter()
                .any(|e| e.contains("before any REQUEST"))
        );
    }

    #[test]
    fn an_http_file_names_its_call_as_a_method_and_a_path() {
        assert_eq!(errors_of(HTTP, "t.httf"), Vec::<String>::new());
    }

    #[test]
    fn any_method_is_a_method() {
        let content = HTTP.replace("POST /v1/users", "PROPFIND /dav/");
        assert_eq!(errors_of(&content, "t.httf"), Vec::<String>::new());
    }

    #[test]
    fn an_rpc_endpoint_in_an_http_file_is_reported() {
        let content = HTTP.replace("POST /v1/users", "users.UserService/GetUser");
        let errors = errors_of(&content, "t.httf");
        assert!(
            errors.iter().any(|e| e.contains("POST /v1/users")),
            "{errors:?}"
        );
    }

    #[test]
    fn a_path_without_a_method_is_reported() {
        let content = HTTP.replace("POST /v1/users", "/v1/users");
        assert!(!errors_of(&content, "t.httf").is_empty());
    }

    #[test]
    fn an_http_file_has_no_descriptors() {
        let content = format!("{HTTP}\n--- PROTO ---\ndescriptor: /tmp/x.bin\n");
        let errors = errors_of(&content, "t.httf");
        assert!(errors.iter().any(|e| e.contains("PROTO")), "{errors:?}");
    }

    #[test]
    fn a_base_url_is_an_address_for_an_http_file() {
        let content = HTTP.replace("https://api.example.com", "example.com");
        assert_eq!(errors_of(&content, "t.httf"), Vec::<String>::new());
    }

    #[test]
    fn an_rpc_file_still_wants_a_service_and_a_method() {
        let content = "--- ADDRESS ---\nlocalhost:4770\n\n--- ENDPOINT ---\nGetUser\n\n--- REQUEST ---\n{}\n\n--- ASSERTS ---\n.ok\n";
        let errors = errors_of(content, "t.gctf");
        assert!(
            errors.iter().any(|e| e.contains("package.Service/Method")),
            "{errors:?}"
        );
    }

    #[test]
    fn key_line_finds_the_actual_kv_line_not_the_section_header() {
        let raw = "timeout: 30\nretry-delay: 0.3\n";
        assert_eq!(key_line(raw, 5, "retry-delay"), 5 + 1 + 1);
    }

    #[test]
    fn key_line_falls_back_to_start_line_when_key_absent() {
        let raw = "timeout: 30\n";
        assert_eq!(key_line(raw, 5, "retry-delay"), 5);
    }

    #[test]
    fn key_line_does_not_false_match_key_text_inside_a_value() {
        let raw = "name: \"retry-delay tuning\"\n";
        assert_eq!(key_line(raw, 5, "retry-delay"), 5);
    }

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
                span: SectionSpan::default(),
            },
            Section {
                section_type: SectionType::Endpoint,
                content: SectionContent::Single("my.Service/Method".to_string()),
                inline_options: InlineOptions::default(),
                raw_content: "my.Service/Method".to_string(),
                start_line: 3,
                end_line: 3,
                attributes: Vec::new(),
                span: SectionSpan::default(),
            },
        ];

        doc
    }

    #[test]
    fn validate_required_sections_pass() {
        let doc = create_test_document();
        let result = validate_document(&doc);
        assert!(result.is_err());
    }

    #[test]
    fn validate_endpoint_format() {
        let mut doc = create_test_document();
        doc.sections[1].content = SectionContent::Single("invalid_endpoint".to_string());

        let result = validate_document(&doc);
        assert!(result.is_err());
    }

    #[test]
    fn validate_address_format() {
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
    fn validation_failed() {
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
        assert!(!errors.is_empty());
    }

    #[test]
    fn validate_document_with_response() {
        let mut doc = create_test_document();
        doc.sections.push(Section {
            section_type: SectionType::Response,
            content: SectionContent::Json(serde_json::json!({"result": "ok"})),
            inline_options: InlineOptions::default(),
            raw_content: "{\"result\": \"ok\"}".to_string(),
            start_line: 5,
            end_line: 6,
            attributes: Vec::new(),
            span: SectionSpan::default(),
        });

        let result = validate_document(&doc);
        result.expect("document must validate");
    }

    #[test]
    fn validate_document_with_error_section() {
        let mut doc = create_test_document();
        doc.sections.push(Section {
            section_type: SectionType::Error,
            content: SectionContent::Json(serde_json::json!({"code": 5})),
            inline_options: InlineOptions::default(),
            raw_content: "{\"code\": 5}".to_string(),
            start_line: 5,
            end_line: 6,
            attributes: Vec::new(),
            span: SectionSpan::default(),
        });

        let result = validate_document(&doc);
        result.expect("document must validate");
    }

    #[test]
    fn validate_document_with_request_jsonlines() {
        let mut doc = create_test_document();
        doc.sections.push(Section {
            section_type: SectionType::Request,
            content: SectionContent::JsonLines(vec![
                serde_json::json!({"a": 1}),
                serde_json::json!({"a": 2}),
            ]),
            inline_options: InlineOptions::default(),
            raw_content: "{\"a\": 1}\n{\"a\": 2}".to_string(),
            start_line: 5,
            end_line: 6,
            attributes: Vec::new(),
            span: SectionSpan::default(),
        });
        doc.sections.push(Section {
            section_type: SectionType::Response,
            content: SectionContent::Json(serde_json::json!({"result": "ok"})),
            inline_options: InlineOptions::default(),
            raw_content: "{\"result\": \"ok\"}".to_string(),
            start_line: 7,
            end_line: 8,
            attributes: Vec::new(),
            span: SectionSpan::default(),
        });

        let result = validate_document(&doc);
        result.expect("document must validate");
    }

    #[test]
    fn validate_document_error_partial_option_allowed() {
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
            span: SectionSpan::default(),
        });

        let errors = validate_document_diagnostics(&doc);
        assert!(!errors.iter().any(|e| {
            e.message
                .contains("ERROR section only supports partial and with_asserts")
        }));
    }

    #[test]
    fn validate_document_error_tolerance_still_warns() {
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
            span: SectionSpan::default(),
        });

        let errors = validate_document_diagnostics(&doc);
        assert!(errors.iter().any(|e| {
            e.message
                .contains("ERROR section only supports partial and with_asserts")
        }));
    }

    #[test]
    fn validate_document_warns_on_empty_error_with_asserts() {
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
            span: SectionSpan::default(),
        });
        doc.sections.push(Section {
            section_type: SectionType::Asserts,
            content: SectionContent::Assertions(vec![".code == 5".to_string()]),
            inline_options: InlineOptions::default(),
            raw_content: ".code == 5".to_string(),
            start_line: 6,
            end_line: 6,
            attributes: Vec::new(),
            span: SectionSpan::default(),
        });

        let errors = validate_document_diagnostics(&doc);
        assert!(errors.iter().any(|e| {
            e.message
                .contains("Empty ERROR with with_asserts is redundant")
                && e.severity == ErrorSeverity::Warning
        }));
    }

    #[test]
    fn validate_document_no_warning_for_non_empty_error_with_asserts() {
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
            span: SectionSpan::default(),
        });
        doc.sections.push(Section {
            section_type: SectionType::Asserts,
            content: SectionContent::Assertions(vec![".code == 5".to_string()]),
            inline_options: InlineOptions::default(),
            raw_content: ".code == 5".to_string(),
            start_line: 7,
            end_line: 7,
            attributes: Vec::new(),
            span: SectionSpan::default(),
        });

        let errors = validate_document_diagnostics(&doc);
        assert!(!errors.iter().any(|e| {
            e.message
                .contains("Empty ERROR with with_asserts is redundant")
        }));
    }

    #[test]
    fn validate_document_no_warning_for_empty_error_without_with_asserts() {
        let mut doc = create_test_document();
        doc.sections.push(Section {
            section_type: SectionType::Error,
            content: SectionContent::Empty,
            inline_options: InlineOptions::default(),
            raw_content: "".to_string(),
            start_line: 5,
            end_line: 5,
            attributes: Vec::new(),
            span: SectionSpan::default(),
        });
        doc.sections.push(Section {
            section_type: SectionType::Asserts,
            content: SectionContent::Assertions(vec![".code == 5".to_string()]),
            inline_options: InlineOptions::default(),
            raw_content: ".code == 5".to_string(),
            start_line: 6,
            end_line: 6,
            attributes: Vec::new(),
            span: SectionSpan::default(),
        });

        let errors = validate_document_diagnostics(&doc);
        assert!(!errors.iter().any(|e| {
            e.message
                .contains("Empty ERROR with with_asserts is redundant")
        }));
    }

    #[test]
    fn validate_document_no_warning_for_empty_error_with_non_adjacent_asserts() {
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
            span: SectionSpan::default(),
        });
        doc.sections.push(Section {
            section_type: SectionType::Request,
            content: SectionContent::Json(serde_json::json!({"id": 1})),
            inline_options: InlineOptions::default(),
            raw_content: "{\"id\": 1}".to_string(),
            start_line: 6,
            end_line: 7,
            attributes: Vec::new(),
            span: SectionSpan::default(),
        });
        doc.sections.push(Section {
            section_type: SectionType::Asserts,
            content: SectionContent::Assertions(vec![".code == 5".to_string()]),
            inline_options: InlineOptions::default(),
            raw_content: ".code == 5".to_string(),
            start_line: 8,
            end_line: 8,
            attributes: Vec::new(),
            span: SectionSpan::default(),
        });

        let errors = validate_document_diagnostics(&doc);
        assert!(!errors.iter().any(|e| {
            e.message
                .contains("Empty ERROR with with_asserts is redundant")
        }));
    }

    #[test]
    fn validate_document_with_asserts() {
        let mut doc = create_test_document();
        doc.sections.push(Section {
            section_type: SectionType::Asserts,
            content: SectionContent::Assertions(vec![".id == 1".to_string()]),
            inline_options: InlineOptions::default(),
            raw_content: ".id == 1".to_string(),
            start_line: 5,
            end_line: 5,
            attributes: Vec::new(),
            span: SectionSpan::default(),
        });

        let result = validate_document(&doc);
        result.expect("document must validate");
    }

    #[test]
    fn validate_document_missing_endpoint() {
        let mut doc = create_test_document();
        doc.sections.remove(1);

        let errors = validate_document_diagnostics(&doc);
        let has_endpoint_error = errors.iter().any(|e| e.message.contains("ENDPOINT"));
        assert!(has_endpoint_error);
    }

    #[test]
    fn validate_document_response_error_conflict() {
        let mut doc = create_test_document();
        doc.sections.push(Section {
            section_type: SectionType::Response,
            content: SectionContent::Json(serde_json::json!({"result": "ok"})),
            inline_options: InlineOptions::default(),
            raw_content: "{\"result\": \"ok\"}".to_string(),
            start_line: 5,
            end_line: 6,
            attributes: Vec::new(),
            span: SectionSpan::default(),
        });
        doc.sections.push(Section {
            section_type: SectionType::Error,
            content: SectionContent::Json(serde_json::json!({"code": 5})),
            inline_options: InlineOptions::default(),
            raw_content: "{\"code\": 5}".to_string(),
            start_line: 7,
            end_line: 8,
            attributes: Vec::new(),
            span: SectionSpan::default(),
        });

        let errors = validate_document_diagnostics(&doc);
        let has_conflict_error = errors
            .iter()
            .any(|e| e.message.contains("RESPONSE") && e.message.contains("ERROR"));
        assert!(has_conflict_error);
    }

    #[test]
    fn validate_document_empty_requests() {
        let mut doc = create_test_document();
        doc.sections.push(Section {
            section_type: SectionType::Request,
            content: SectionContent::Empty,
            inline_options: InlineOptions::default(),
            raw_content: "".to_string(),
            start_line: 5,
            end_line: 5,
            attributes: Vec::new(),
            span: SectionSpan::default(),
        });
        doc.sections.push(Section {
            section_type: SectionType::Response,
            content: SectionContent::Json(serde_json::json!({"result": "ok"})),
            inline_options: InlineOptions::default(),
            raw_content: "{\"result\": \"ok\"}".to_string(),
            start_line: 6,
            end_line: 7,
            attributes: Vec::new(),
            span: SectionSpan::default(),
        });

        let result = validate_document(&doc);
        result.expect("document must validate");
    }

    #[test]
    fn validate_document_invalid_request_json() {
        let mut doc = create_test_document();
        doc.sections.push(Section {
            section_type: SectionType::Request,
            content: SectionContent::Json(serde_json::json!({"key": "value"})),
            inline_options: InlineOptions::default(),
            raw_content: "{\"key\": \"value\"}".to_string(),
            start_line: 5,
            end_line: 6,
            attributes: Vec::new(),
            span: SectionSpan::default(),
        });
        doc.sections.push(Section {
            section_type: SectionType::Response,
            content: SectionContent::Json(serde_json::json!({"result": "ok"})),
            inline_options: InlineOptions::default(),
            raw_content: "{\"result\": \"ok\"}".to_string(),
            start_line: 7,
            end_line: 8,
            attributes: Vec::new(),
            span: SectionSpan::default(),
        });

        let result = validate_document(&doc);
        result.expect("document must validate");
    }

    #[test]
    fn validate_document_invalid_response_json() {
        let mut doc = create_test_document();
        doc.sections.push(Section {
            section_type: SectionType::Request,
            content: SectionContent::Json(serde_json::json!({"key": "value"})),
            inline_options: InlineOptions::default(),
            raw_content: "{\"key\": \"value\"}".to_string(),
            start_line: 5,
            end_line: 6,
            attributes: Vec::new(),
            span: SectionSpan::default(),
        });
        doc.sections.push(Section {
            section_type: SectionType::Response,
            content: SectionContent::Json(serde_json::json!({"result": "ok"})),
            inline_options: InlineOptions::default(),
            raw_content: "{\"result\": \"ok\"}".to_string(),
            start_line: 7,
            end_line: 8,
            attributes: Vec::new(),
            span: SectionSpan::default(),
        });

        let errors = validate_document_diagnostics(&doc);
        let has_json_errors = errors.iter().any(|e| e.message.contains("JSON"));
        assert!(!has_json_errors);
    }

    #[test]
    fn validate_error_details_must_be_array() {
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
            span: SectionSpan::default(),
        });

        let errors = validate_document_diagnostics(&doc);
        assert!(
            errors
                .iter()
                .any(|e| e.message.contains("field 'details' must be an array"))
        );
    }

    #[test]
    fn validate_error_details_items_must_be_objects() {
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
            span: SectionSpan::default(),
        });

        let errors = validate_document_diagnostics(&doc);
        assert!(
            errors
                .iter()
                .any(|e| e.message.contains("'details' items must be objects"))
        );
    }

    #[test]
    fn validate_document_address_from_env() {
        unsafe {
            std::env::set_var("GRPCTESTIFY_ADDRESS", "env:5000");
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
            span: SectionSpan::default(),
        });
        doc.sections.push(Section {
            section_type: SectionType::Response,
            content: SectionContent::Json(serde_json::json!({"result": "ok"})),
            inline_options: InlineOptions::default(),
            raw_content: "{\"result\": \"ok\"}".to_string(),
            start_line: 2,
            end_line: 3,
            attributes: Vec::new(),
            span: SectionSpan::default(),
        });

        let result = validate_document(&doc);
        result.expect("document must validate");

        unsafe {
            std::env::remove_var("GRPCTESTIFY_ADDRESS");
        }
    }

    #[test]
    fn validate_options_unknown_key_warning() {
        let mut doc = create_test_document();
        let mut options = crate::ast::OrderedStringMap::new();
        options.insert("unknown".to_string(), "value".to_string());
        doc.sections.push(Section {
            section_type: SectionType::Options,
            content: SectionContent::KeyValues(options),
            inline_options: InlineOptions::default(),
            raw_content: "unknown: value".to_string(),
            start_line: 5,
            end_line: 6,
            attributes: Vec::new(),
            span: SectionSpan::default(),
        });
        doc.sections.push(Section {
            section_type: SectionType::Response,
            content: SectionContent::Json(serde_json::json!({"result": "ok"})),
            inline_options: InlineOptions::default(),
            raw_content: "{\"result\": \"ok\"}".to_string(),
            start_line: 7,
            end_line: 8,
            attributes: Vec::new(),
            span: SectionSpan::default(),
        });

        let diagnostics = validate_document_diagnostics(&doc);
        assert!(diagnostics.iter().any(|d| {
            d.severity == ErrorSeverity::Warning && d.message.contains("Unknown OPTIONS key")
        }));
    }

    #[test]
    fn validate_options_unknown_key_points_at_the_key_line_not_the_header() {
        let mut doc = create_test_document();
        let mut options = crate::ast::OrderedStringMap::new();
        options.insert("timeout".to_string(), "30".to_string());
        options.insert("dry_run".to_string(), "true".to_string());
        doc.sections.push(Section {
            section_type: SectionType::Options,
            content: SectionContent::KeyValues(options),
            inline_options: InlineOptions::default(),
            raw_content: "timeout: 30\ndry_run: true".to_string(),
            start_line: 5,
            end_line: 8,
            attributes: Vec::new(),
            span: SectionSpan::default(),
        });
        doc.sections.push(Section {
            section_type: SectionType::Response,
            content: SectionContent::Json(serde_json::json!({"result": "ok"})),
            inline_options: InlineOptions::default(),
            raw_content: "{\"result\": \"ok\"}".to_string(),
            start_line: 9,
            end_line: 10,
            attributes: Vec::new(),
            span: SectionSpan::default(),
        });

        let diag = validate_document_diagnostics(&doc)
            .into_iter()
            .find(|d| d.message.contains("Unknown OPTIONS key 'dry_run'"))
            .expect("unknown-key warning");
        assert_eq!(
            diag.line,
            Some(7),
            "must point at the dry_run line, not the header (5)"
        );
    }

    #[test]
    fn validate_options_dry_run_is_unknown_key_warning() {
        let mut doc = create_test_document();
        let mut options = crate::ast::OrderedStringMap::new();
        options.insert("dry_run".to_string(), "true".to_string());
        doc.sections.push(Section {
            section_type: SectionType::Options,
            content: SectionContent::KeyValues(options),
            inline_options: InlineOptions::default(),
            raw_content: "dry_run: true".to_string(),
            start_line: 5,
            end_line: 6,
            attributes: Vec::new(),
            span: SectionSpan::default(),
        });
        doc.sections.push(Section {
            section_type: SectionType::Response,
            content: SectionContent::Json(serde_json::json!({"result": "ok"})),
            inline_options: InlineOptions::default(),
            raw_content: "{\"result\": \"ok\"}".to_string(),
            start_line: 7,
            end_line: 8,
            attributes: Vec::new(),
            span: SectionSpan::default(),
        });

        let diagnostics = validate_document_diagnostics(&doc);
        assert!(diagnostics.iter().any(|d| {
            d.severity == ErrorSeverity::Warning
                && d.message
                    .contains("Unknown OPTIONS key 'dry_run'. Supported keys: timeout, retry, retry_delay, no_retry, compression, protocol")
        }));
    }

    #[test]
    fn validate_options_timeout_invalid_error() {
        let mut doc = create_test_document();
        let mut options = crate::ast::OrderedStringMap::new();
        options.insert("timeout".to_string(), "0".to_string());
        doc.sections.push(Section {
            section_type: SectionType::Options,
            content: SectionContent::KeyValues(options),
            inline_options: InlineOptions::default(),
            raw_content: "timeout: 0".to_string(),
            start_line: 5,
            end_line: 6,
            attributes: Vec::new(),
            span: SectionSpan::default(),
        });
        doc.sections.push(Section {
            section_type: SectionType::Response,
            content: SectionContent::Json(serde_json::json!({"result": "ok"})),
            inline_options: InlineOptions::default(),
            raw_content: "{\"result\": \"ok\"}".to_string(),
            start_line: 7,
            end_line: 8,
            attributes: Vec::new(),
            span: SectionSpan::default(),
        });

        let diagnostics = validate_document_diagnostics(&doc);
        assert!(diagnostics.iter().any(|d| {
            d.severity == ErrorSeverity::Error
                && d.message
                    .contains("OPTIONS.timeout must be a positive integer")
        }));
    }

    #[test]
    fn validate_options_snake_case_keys_are_supported() {
        let mut doc = create_test_document();
        let mut options = crate::ast::OrderedStringMap::new();
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
            span: SectionSpan::default(),
        });
        doc.sections.push(Section {
            section_type: SectionType::Response,
            content: SectionContent::Json(serde_json::json!({"result": "ok"})),
            inline_options: InlineOptions::default(),
            raw_content: "{\"result\": \"ok\"}".to_string(),
            start_line: 9,
            end_line: 10,
            attributes: Vec::new(),
            span: SectionSpan::default(),
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
    fn validate_options_compression_invalid_error() {
        let mut doc = create_test_document();
        let mut options = crate::ast::OrderedStringMap::new();
        options.insert("compression".to_string(), "brotli".to_string());
        doc.sections.push(Section {
            section_type: SectionType::Options,
            content: SectionContent::KeyValues(options),
            inline_options: InlineOptions::default(),
            raw_content: "compression: brotli".to_string(),
            start_line: 5,
            end_line: 6,
            attributes: Vec::new(),
            span: SectionSpan::default(),
        });
        doc.sections.push(Section {
            section_type: SectionType::Response,
            content: SectionContent::Json(serde_json::json!({"result": "ok"})),
            inline_options: InlineOptions::default(),
            raw_content: "{\"result\": \"ok\"}".to_string(),
            start_line: 7,
            end_line: 8,
            attributes: Vec::new(),
            span: SectionSpan::default(),
        });

        let diagnostics = validate_document_diagnostics(&doc);
        assert!(diagnostics.iter().any(|d| {
            d.severity == ErrorSeverity::Error
                && d.message
                    .contains("OPTIONS.compression must be one of: none, gzip")
        }));
    }

    #[test]
    fn validate_options_protocol_accepted() {
        for value in ["grpc", "grpc-web", "connectrpc", "GRPC-Web"] {
            let mut doc = create_test_document();
            let mut options = crate::ast::OrderedStringMap::new();
            options.insert("protocol".to_string(), value.to_string());
            doc.sections.push(Section {
                section_type: SectionType::Options,
                content: SectionContent::KeyValues(options),
                inline_options: InlineOptions::default(),
                raw_content: format!("protocol: {}", value),
                start_line: 5,
                end_line: 6,
                attributes: Vec::new(),
                span: SectionSpan::default(),
            });
            doc.sections.push(Section {
                section_type: SectionType::Response,
                content: SectionContent::Json(serde_json::json!({"result": "ok"})),
                inline_options: InlineOptions::default(),
                raw_content: "{\"result\": \"ok\"}".to_string(),
                start_line: 7,
                end_line: 8,
                attributes: Vec::new(),
                span: SectionSpan::default(),
            });

            let diagnostics = validate_document_diagnostics(&doc);
            assert!(
                !diagnostics
                    .iter()
                    .any(|d| d.message.contains("Unknown OPTIONS key 'protocol'")),
                "protocol: {} was reported as unknown",
                value
            );
        }
    }

    #[test]
    fn validate_options_protocol_invalid_error() {
        let mut doc = create_test_document();
        let mut options = crate::ast::OrderedStringMap::new();
        options.insert("protocol".to_string(), "grpcweb".to_string());
        doc.sections.push(Section {
            section_type: SectionType::Options,
            content: SectionContent::KeyValues(options),
            inline_options: InlineOptions::default(),
            raw_content: "protocol: grpcweb".to_string(),
            start_line: 5,
            end_line: 6,
            attributes: Vec::new(),
            span: SectionSpan::default(),
        });
        doc.sections.push(Section {
            section_type: SectionType::Response,
            content: SectionContent::Json(serde_json::json!({"result": "ok"})),
            inline_options: InlineOptions::default(),
            raw_content: "{\"result\": \"ok\"}".to_string(),
            start_line: 7,
            end_line: 8,
            attributes: Vec::new(),
            span: SectionSpan::default(),
        });

        let diagnostics = validate_document_diagnostics(&doc);
        assert!(diagnostics.iter().any(|d| {
            d.severity == ErrorSeverity::Error
                && d.message
                    .contains("OPTIONS.protocol must be one of: grpc, grpc-web, connectrpc")
        }));
    }

    #[test]
    fn validate_options_kebab_case_keys_accepted_without_error() {
        let mut doc = create_test_document();
        let mut options = crate::ast::OrderedStringMap::new();
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
            span: SectionSpan::default(),
        });
        doc.sections.push(Section {
            section_type: SectionType::Response,
            content: SectionContent::Json(serde_json::json!({"result": "ok"})),
            inline_options: InlineOptions::default(),
            raw_content: "{\"result\": \"ok\"}".to_string(),
            start_line: 8,
            end_line: 9,
            attributes: Vec::new(),
            span: SectionSpan::default(),
        });

        let diagnostics = validate_document_diagnostics(&doc);
        assert!(
            !diagnostics
                .iter()
                .any(|d| d.severity == ErrorSeverity::Error)
        );
        assert!(
            !diagnostics
                .iter()
                .any(|d| d.message.contains("is deprecated")),
            "validator must no longer emit deprecation warnings: {diagnostics:?}"
        );
    }

    #[test]
    fn validate_options_no_retry_retry_conflict_warning() {
        let mut doc = create_test_document();
        let mut options = crate::ast::OrderedStringMap::new();
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
            span: SectionSpan::default(),
        });
        doc.sections.push(Section {
            section_type: SectionType::Response,
            content: SectionContent::Json(serde_json::json!({"result": "ok"})),
            inline_options: InlineOptions::default(),
            raw_content: "{\"result\": \"ok\"}".to_string(),
            start_line: 8,
            end_line: 9,
            attributes: Vec::new(),
            span: SectionSpan::default(),
        });

        let diagnostics = validate_document_diagnostics(&doc);
        assert!(diagnostics.iter().any(|d| {
            d.severity == ErrorSeverity::Warning
                && d.message
                    .contains("OPTIONS.no_retry=true conflicts with OPTIONS.retry>0")
        }));
    }

    #[test]
    fn validate_kebab_case_attributes_accepted_without_error() {
        let mut doc = create_test_document();
        doc.sections.push(Section {
            section_type: SectionType::Request,
            content: SectionContent::Json(serde_json::json!({"id": 1})),
            inline_options: InlineOptions::default(),
            raw_content: "{\"id\":1}".to_string(),
            start_line: 5,
            end_line: 6,
            attributes: vec![
                GctfAttribute::new("retry-delay", "0.2"),
                GctfAttribute::flag("no-retry"),
            ],
            span: SectionSpan::default(),
        });
        doc.sections.push(Section {
            section_type: SectionType::Response,
            content: SectionContent::Json(serde_json::json!({"result": "ok"})),
            inline_options: InlineOptions::default(),
            raw_content: "{\"result\": \"ok\"}".to_string(),
            start_line: 7,
            end_line: 8,
            attributes: Vec::new(),
            span: SectionSpan::default(),
        });

        let diagnostics = validate_document_diagnostics(&doc);
        assert!(
            !diagnostics
                .iter()
                .any(|d| d.severity == ErrorSeverity::Error)
        );
        assert!(
            !diagnostics
                .iter()
                .any(|d| d.message.contains("is deprecated")),
            "validator must no longer emit deprecation warnings: {diagnostics:?}"
        );
    }

    #[test]
    fn validate_attribute_repeat_and_compression_are_recognized() {
        let mut doc = create_test_document();
        doc.sections.push(Section {
            section_type: SectionType::Request,
            content: SectionContent::Json(serde_json::json!({"id": 1})),
            inline_options: InlineOptions::default(),
            raw_content: "{\"id\":1}".to_string(),
            start_line: 5,
            end_line: 6,
            attributes: vec![
                GctfAttribute::new("repeat", "3"),
                GctfAttribute::new("compression", "gzip"),
            ],
            span: SectionSpan::default(),
        });
        doc.sections.push(Section {
            section_type: SectionType::Response,
            content: SectionContent::Json(serde_json::json!({"result": "ok"})),
            inline_options: InlineOptions::default(),
            raw_content: "{\"result\": \"ok\"}".to_string(),
            start_line: 7,
            end_line: 8,
            attributes: Vec::new(),
            span: SectionSpan::default(),
        });

        let diagnostics = validate_document_diagnostics(&doc);
        assert!(
            !diagnostics
                .iter()
                .any(|d| d.message.contains("Unknown attribute")),
            "{:?}",
            diagnostics
        );
    }

    #[test]
    fn validate_attribute_repeat_rejects_zero() {
        let mut doc = create_test_document();
        doc.sections.push(Section {
            section_type: SectionType::Request,
            content: SectionContent::Json(serde_json::json!({"id": 1})),
            inline_options: InlineOptions::default(),
            raw_content: "{\"id\":1}".to_string(),
            start_line: 5,
            end_line: 6,
            attributes: vec![GctfAttribute::new("repeat", "0")],
            span: SectionSpan::default(),
        });
        doc.sections.push(Section {
            section_type: SectionType::Response,
            content: SectionContent::Json(serde_json::json!({"result": "ok"})),
            inline_options: InlineOptions::default(),
            raw_content: "{\"result\": \"ok\"}".to_string(),
            start_line: 7,
            end_line: 8,
            attributes: Vec::new(),
            span: SectionSpan::default(),
        });

        let diagnostics = validate_document_diagnostics(&doc);
        assert!(diagnostics.iter().any(|d| {
            d.severity == ErrorSeverity::Error
                && d.message
                    .contains("Attribute #[repeat] must be a positive integer")
        }));
    }

    #[test]
    fn validate_attribute_compression_rejects_unknown_codec() {
        let mut doc = create_test_document();
        doc.sections.push(Section {
            section_type: SectionType::Request,
            content: SectionContent::Json(serde_json::json!({"id": 1})),
            inline_options: InlineOptions::default(),
            raw_content: "{\"id\":1}".to_string(),
            start_line: 5,
            end_line: 6,
            attributes: vec![GctfAttribute::new("compression", "brotli")],
            span: SectionSpan::default(),
        });
        doc.sections.push(Section {
            section_type: SectionType::Response,
            content: SectionContent::Json(serde_json::json!({"result": "ok"})),
            inline_options: InlineOptions::default(),
            raw_content: "{\"result\": \"ok\"}".to_string(),
            start_line: 7,
            end_line: 8,
            attributes: Vec::new(),
            span: SectionSpan::default(),
        });

        let diagnostics = validate_document_diagnostics(&doc);
        assert!(diagnostics.iter().any(|d| {
            d.severity == ErrorSeverity::Error
                && d.message
                    .contains("Attribute #[compression] must be one of: none, gzip")
        }));
    }

    #[test]
    fn validate_bench_dynamic_percentile_key_ok() {
        let mut doc = create_test_document();
        let mut bench = crate::ast::OrderedStringMap::new();
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
                span: SectionSpan::default(),
            },
        );

        let diagnostics = validate_document_diagnostics(&doc);
        assert!(!diagnostics.iter().any(
            |d| d.message.contains("Invalid percentile") || d.message.contains("range (0,100)")
        ));
    }

    #[test]
    fn validate_bench_dynamic_percentile_key_invalid_range() {
        let mut doc = create_test_document();
        let mut bench = crate::ast::OrderedStringMap::new();
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
                span: SectionSpan::default(),
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
    fn a_skipped_check_is_not_a_check() {
        let refused = errors_of(
            "--- ENDPOINT ---\npkg.Svc/M\n\n--- REQUEST ---\n{}\n\n#[skip]\n--- ASSERTS ---\n.ok\n",
            "t.gctf",
        );
        assert!(
            refused
                .iter()
                .any(|m| m.contains("At least one verification section")),
            "{refused:?}"
        );
    }

    #[test]
    fn an_assertion_that_ends_on_its_operator_is_refused() {
        let refused = errors_of(
            "--- ENDPOINT ---\npkg.Svc/M\n\n--- REQUEST ---\n{}\n\n--- ASSERTS ---\n.name ==\n",
            "t.gctf",
        );
        assert!(
            refused
                .iter()
                .any(|m| m.contains("ends on `==` with nothing to compare against")),
            "{refused:?}"
        );

        let fine = errors_of(
            "--- ENDPOINT ---\npkg.Svc/M\n\n--- REQUEST ---\n{}\n\n--- ASSERTS ---\n.name == \"Ada\"\n",
            "t.gctf",
        );
        assert!(
            !fine
                .iter()
                .any(|m| m.contains("nothing to compare against")),
            "{fine:?}"
        );
    }

    #[test]
    fn a_whole_url_in_the_endpoint_needs_no_address() {
        let aimed = errors_of(
            "--- ENDPOINT ---\nGET https://api.example.com/v1/users\n\n--- ASSERTS ---\n@status() == 200\n",
            "t.httf",
        );
        assert!(
            !aimed
                .iter()
                .any(|m| m.starts_with("ADDRESS section missing")),
            "{aimed:?}"
        );

        let bare = errors_of(
            "--- ENDPOINT ---\nGET /v1/users\n\n--- ASSERTS ---\n@status() == 200\n",
            "t.httf",
        );
        assert!(
            bare.iter()
                .any(|m| m.starts_with("ADDRESS section missing")),
            "{bare:?}"
        );
    }

    #[test]
    fn an_address_a_call_cannot_be_made_to_is_named() {
        let grpc = |address: &str| {
            errors_of(
                &format!(
                    "--- ADDRESS ---\n{address}\n\n--- ENDPOINT ---\npkg.Svc/M\n\n--- REQUEST ---\n{{}}\n\n--- RESPONSE ---\n{{}}\n"
                ),
                "t.gctf",
            )
        };

        for bad in ["localhost:99999", "localhost:0", "localhost:65536"] {
            assert!(
                grpc(bad)
                    .iter()
                    .any(|m| m.contains("is not one a call can be made to")),
                "{bad}: {:?}",
                grpc(bad)
            );
        }
        for fine in ["localhost:1", "localhost:65535", "localhost:{{port}}"] {
            assert!(
                !grpc(fine)
                    .iter()
                    .any(|m| m.contains("a call can be made to")),
                "{fine}: {:?}",
                grpc(fine)
            );
        }

        let pathed = grpc("localhost:4770/some/path");
        assert!(
            pathed
                .iter()
                .any(|m| m.contains("the path in 'localhost:4770/some/path' is not dialled")),
            "{pathed:?}"
        );
        assert!(
            !grpc("localhost:4770")
                .iter()
                .any(|m| m.contains("is not dialled"))
        );
    }

    #[test]
    fn a_verb_outside_the_usual_ones_is_named_and_not_refused() {
        let http = |endpoint: &str| {
            errors_of(
                &format!(
                    "--- ADDRESS ---\nhttp://x.test\n\n--- ENDPOINT ---\n{endpoint}\n\n--- ASSERTS ---\n@status() == 200\n"
                ),
                "t.httf",
            )
        };

        let typo = http("PSOT /data.json");
        assert!(
            typo.iter().any(
                |m| m.contains("'PSOT' is not one of the usual HTTP methods")
                    && m.contains("Hint: did you mean 'POST'?")
            ),
            "{typo:?}"
        );

        assert!(
            !http("PROPFIND /dav/")
                .iter()
                .any(|m| m.contains("usual HTTP methods"))
        );
        let purge = http("PURGE /cache");
        assert!(
            purge
                .iter()
                .any(|m| m.contains("'PURGE' is not one of the usual")),
            "{purge:?}"
        );
        assert!(!purge.iter().any(|m| m.contains("Hint:")), "{purge:?}");

        assert!(
            !http("GET /x")
                .iter()
                .any(|m| m.contains("usual HTTP methods"))
        );
    }

    #[test]
    fn an_unknown_name_is_matched_by_a_typo_as_well_as_by_a_piece() {
        const KEYS: [&str; 4] = ["ca_cert", "client_cert", "client_key", "server_name"];
        assert_eq!(suggest_from("ca", &KEYS), Some("ca_cert"));
        assert_eq!(
            suggest_from("Retry-Delay", &["timeout", "retry", "retry_delay"]),
            Some("retry_delay")
        );
        assert_eq!(suggest_from("ca_cret", &KEYS), Some("ca_cert"));
        assert_eq!(suggest_from("Server-Name", &KEYS), Some("server_name"));
        assert_eq!(suggest_from("banana", &KEYS), None);
    }

    #[test]
    fn an_unknown_options_key_names_the_one_it_was_reaching_for() {
        assert_eq!(suggest_options_key("timeuot"), Some("timeout"));
        assert_eq!(suggest_options_key("Retry-Delay"), Some("retry_delay"));
        assert_eq!(suggest_options_key("compresion"), Some("compression"));
        assert_eq!(suggest_options_key("banana"), None);
        assert_eq!(suggest_options_key(""), None);

        let said = errors_of(
            "--- ENDPOINT ---\npkg.Svc/M\n\n--- OPTIONS ---\ntimeuot: 5\n\n--- REQUEST ---\n{}\n\n--- RESPONSE ---\n{}\n",
            "t.gctf",
        );
        assert!(
            said.iter()
                .any(|m| m.contains("Hint: did you mean 'timeout'?")),
            "{said:?}"
        );
    }

    #[test]
    fn a_path_the_disk_does_not_have_is_named() {
        let dir = std::env::temp_dir().join(format!("validator-paths-{}", std::process::id()));
        std::fs::create_dir_all(&dir).expect("dir");
        let here = dir.join("t.gctf");
        std::fs::write(dir.join("real.bin"), b"x").expect("write");

        let said = errors_of(
            "--- ENDPOINT ---\npkg.Svc/M\n\n--- PROTO ---\ndescriptor: gone.bin\n\n--- REQUEST ---\n{}\n\n--- RESPONSE ---\n{}\n",
            &here.to_string_lossy(),
        );
        assert!(
            said.iter()
                .any(|m| m.contains("PROTO names gone.bin, and there is nothing at")),
            "{said:?}"
        );

        let found = errors_of(
            "--- ENDPOINT ---\npkg.Svc/M\n\n--- PROTO ---\ndescriptor: real.bin\n\n--- REQUEST ---\n{}\n\n--- RESPONSE ---\n{}\n",
            &here.to_string_lossy(),
        );
        assert!(
            !found.iter().any(|m| m.contains("there is nothing")),
            "{found:?}"
        );

        let templated = errors_of(
            "--- ENDPOINT ---\npkg.Svc/M\n\n--- PROTO ---\ndescriptor: {{schema}}\n\n--- REQUEST ---\n{}\n\n--- RESPONSE ---\n{}\n",
            &here.to_string_lossy(),
        );
        assert!(
            !templated.iter().any(|m| m.contains("there is nothing")),
            "{templated:?}"
        );

        let draft = errors_of(
            "--- ENDPOINT ---\npkg.Svc/M\n\n--- PROTO ---\ndescriptor: gone.bin\n\n--- REQUEST ---\n{}\n\n--- RESPONSE ---\n{}\n",
            "playground.gctf",
        );
        assert!(
            !draft.iter().any(|m| m.contains("there is nothing")),
            "{draft:?}"
        );

        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn a_key_value_line_that_is_not_one_is_refused() {
        let said = errors_of(
            "--- ENDPOINT ---\npkg.Svc/M\n\n--- REQUEST_HEADERS ---\nauthorization Bearer t\nx-ok: 1\n\n--- REQUEST ---\n{}\n\n--- RESPONSE ---\n{}\n",
            "t.gctf",
        );
        assert!(
            said.iter()
                .any(|m| m.contains("REQUEST_HEADERS line is not a `key: value` pair")),
            "{said:?}"
        );

        let fine = errors_of(
            "--- ENDPOINT ---\npkg.Svc/M\n\n--- REQUEST_HEADERS ---\n// a note\nauthorization: Bearer t\n\n--- REQUEST ---\n{}\n\n--- RESPONSE ---\n{}\n",
            "t.gctf",
        );
        assert!(
            !fine
                .iter()
                .any(|m| m.contains("is not a `key: value` pair")),
            "{fine:?}"
        );
    }

    #[test]
    fn an_extract_line_that_binds_nothing_is_refused() {
        let head = "--- ENDPOINT ---\npkg.Svc/M\n\n--- REQUEST ---\n{}\n\n--- RESPONSE ---\n{}\n\n--- EXTRACT ---\n";
        for line in ["who", "= .name", "who ="] {
            let said = errors_of(&format!("{head}{line}\n"), "t.gctf");
            assert!(
                said.iter()
                    .any(|m| m.contains("EXTRACT line binds nothing")),
                "{line}: {said:?}"
            );
        }
        for line in ["who = .name", "who:number = .n", "// a note", "# a note"] {
            let said = errors_of(&format!("{head}{line}\n"), "t.gctf");
            assert!(
                !said
                    .iter()
                    .any(|m| m.contains("EXTRACT line binds nothing")),
                "{line}: {said:?}"
            );
        }
    }

    #[test]
    fn an_assertion_that_cannot_fail_is_refused() {
        let constant = errors_of(
            "--- ENDPOINT ---\npkg.Svc/M\n\n--- REQUEST ---\n{}\n\n--- ASSERTS ---\n200\n",
            "t.gctf",
        );
        assert!(
            constant
                .iter()
                .any(|m| m.contains("reads nothing from the answer")),
            "{constant:?}"
        );

        let piped = errors_of(
            "--- ENDPOINT ---\npkg.Svc/M\n\n--- REQUEST ---\n{}\n\n--- ASSERTS ---\n.name |\n",
            "t.gctf",
        );
        assert!(
            piped
                .iter()
                .any(|m| m.contains("ends on `|` with nothing after it")),
            "{piped:?}"
        );

        let bound = errors_of(
            "--- ENDPOINT ---\npkg.Svc/M\n\n--- REQUEST ---\n{}\n\n--- EXTRACT ---\ncount = .n\n\n--- ASSERTS ---\ncount == 2\n",
            "t.gctf",
        );
        assert!(
            !bound.iter().any(|m| m.contains("reads nothing")),
            "{bound:?}"
        );
    }

    #[test]
    fn a_check_that_still_runs_is_enough() {
        let said = errors_of(
            "--- ENDPOINT ---\npkg.Svc/M\n\n--- REQUEST ---\n{}\n\n#[skip]\n--- ASSERTS ---\n.ok\n\n--- RESPONSE ---\n{}\n",
            "t.gctf",
        );
        assert!(
            !said
                .iter()
                .any(|m| m.contains("At least one verification section")),
            "{said:?}"
        );
    }

    #[test]
    fn validate_bench_threshold_expression_invalid() {
        let mut doc = create_test_document();
        let mut bench = crate::ast::OrderedStringMap::new();
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
                span: SectionSpan::default(),
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
    fn validate_bench_numeric_keys_accept_digit_separators() {
        let mut doc = create_test_document();
        let mut bench = crate::ast::OrderedStringMap::new();
        bench.insert("concurrency".to_string(), "1_000".to_string());
        bench.insert("requests".to_string(), "1_000_000".to_string());
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
                span: SectionSpan::default(),
            },
        );

        let diagnostics = validate_document_diagnostics(&doc);
        assert!(
            !diagnostics
                .iter()
                .any(|d| d.message.contains("must be a non-negative integer")),
            "digit-separated BENCH numeric keys must validate: {diagnostics:?}"
        );
    }

    #[test]
    fn validate_bench_load_schedule_and_progress_keys() {
        let mut doc = create_test_document();
        let mut bench = crate::ast::OrderedStringMap::new();
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
                span: SectionSpan::default(),
            },
        );

        let diagnostics = validate_document_diagnostics(&doc);
        assert!(!diagnostics.iter().any(|d| {
            d.message.contains("Unknown BENCH key")
                || d.message.contains("BENCH.load_schedule must be one of")
        }));
    }

    #[test]
    fn validate_bench_hyphenated_keys_are_unknown() {
        let mut doc = create_test_document();
        let mut bench = crate::ast::OrderedStringMap::new();
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
                span: SectionSpan::default(),
            },
        );

        let diagnostics = validate_document_diagnostics(&doc);
        assert!(diagnostics.iter().any(|d| {
            d.message.contains("Unknown BENCH key 'load-schedule'")
                && d.message.contains("did you mean 'load_schedule'")
        }));
    }

    #[test]
    fn validate_bench_snake_case_keys_no_deprecation_warning() {
        let mut doc = create_test_document();
        let mut bench = crate::ast::OrderedStringMap::new();
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
                span: SectionSpan::default(),
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
    fn validate_bench_unknown_key_typo_suggestion() {
        let mut doc = create_test_document();
        let mut bench = crate::ast::OrderedStringMap::new();
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
                span: SectionSpan::default(),
            },
        );

        let diagnostics = validate_document_diagnostics(&doc);
        assert!(diagnostics.iter().any(|d| {
            d.message.contains("Unknown BENCH key 'load_shedule'")
                && d.message.contains("did you mean 'load_schedule'")
        }));
    }

    #[test]
    fn validation_error_debug() {
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
    fn error_severity_serialize() {
        let error = ErrorSeverity::Error;
        let json = serde_json::to_string(&error).unwrap();
        assert_eq!(json, "\"error\"");

        let warning = ErrorSeverity::Warning;
        let json = serde_json::to_string(&warning).unwrap();
        assert_eq!(json, "\"warning\"");
    }

    #[test]
    fn section_order_response_before_request_warns() {
        let mut doc = GctfDocument::new("test.gctf".to_string());
        doc.sections.push(Section {
            section_type: SectionType::Response,
            content: SectionContent::Json(serde_json::json!({})),
            inline_options: InlineOptions::default(),
            raw_content: "{}".to_string(),
            start_line: 1,
            end_line: 2,
            attributes: Vec::new(),
            span: SectionSpan::default(),
        });
        let mut errors = Vec::new();
        validate_section_order(&doc, &mut errors);
        assert!(errors.iter().any(|e| e.message.contains("Response")));
    }

    #[test]
    fn section_order_response_after_request_ok() {
        let mut doc = GctfDocument::new("test.gctf".to_string());
        doc.sections.push(Section {
            section_type: SectionType::Request,
            content: SectionContent::Json(serde_json::json!({"x": 1})),
            inline_options: InlineOptions::default(),
            raw_content: "{\"x\": 1}".to_string(),
            start_line: 1,
            end_line: 2,
            attributes: Vec::new(),
            span: SectionSpan::default(),
        });
        doc.sections.push(Section {
            section_type: SectionType::Response,
            content: SectionContent::Json(serde_json::json!({})),
            inline_options: InlineOptions::default(),
            raw_content: "{}".to_string(),
            start_line: 3,
            end_line: 4,
            attributes: Vec::new(),
            span: SectionSpan::default(),
        });
        let mut errors = Vec::new();
        validate_section_order(&doc, &mut errors);
        assert!(errors.is_empty());
    }

    #[test]
    fn section_order_extract_before_response_warns() {
        let mut doc = GctfDocument::new("test.gctf".to_string());
        doc.sections.push(Section {
            section_type: SectionType::Request,
            content: SectionContent::Json(serde_json::json!({})),
            inline_options: InlineOptions::default(),
            raw_content: "{}".to_string(),
            start_line: 1,
            end_line: 2,
            attributes: Vec::new(),
            span: SectionSpan::default(),
        });
        doc.sections.push(Section {
            section_type: SectionType::Extract,
            content: SectionContent::Single("var = .value".to_string()),
            inline_options: InlineOptions::default(),
            raw_content: "var = .value".to_string(),
            start_line: 3,
            end_line: 4,
            attributes: Vec::new(),
            span: SectionSpan::default(),
        });
        let mut errors = Vec::new();
        validate_section_order(&doc, &mut errors);
        assert!(errors.iter().any(|e| e.message.contains("EXTRACT")));
    }

    fn valid_doc() -> GctfDocument {
        let mut doc = create_test_document();
        doc.sections.push(Section {
            section_type: SectionType::Response,
            content: SectionContent::Json(serde_json::json!({"result": "ok"})),
            inline_options: InlineOptions::default(),
            raw_content: "{\"result\": \"ok\"}".to_string(),
            start_line: 5,
            end_line: 6,
            attributes: Vec::new(),
            span: SectionSpan::default(),
        });
        doc
    }

    #[test]
    fn validate_chain_second_document_missing_endpoint() {
        let mut head = valid_doc();
        let mut second = valid_doc();
        second
            .sections
            .retain(|s| s.section_type != SectionType::Endpoint);
        head.next_document = Some(Box::new(second));

        let diagnostics = validate_document_chain_diagnostics(&head);
        assert!(diagnostics.iter().any(|e| {
            e.message.contains("document 2")
                && e.message.contains("ENDPOINT")
                && e.severity == ErrorSeverity::Error
        }));
        assert!(validate_document_chain(&head).is_err());
    }

    #[test]
    fn validate_chain_all_valid_passes() {
        let mut head = valid_doc();
        head.next_document = Some(Box::new(valid_doc()));
        assert!(validate_document_chain(&head).is_ok());

        assert!(validate_document_chain(&valid_doc()).is_ok());
    }
    #[test]
    fn tls_and_proto_typos_are_reported() {
        let src = "--- ENDPOINT ---\npkg.Svc/M\n\n             --- TLS ---\nca_certificate: /ca.pem\nserver_name: api\n\n             --- PROTO ---\nimport_path: /protos\n\n             --- REQUEST ---\n{}\n\n             --- ASSERTS ---\n.ok == true\n";
        let doc = crate::parse_gctf_from_str(src, "t.gctf").expect("parse");
        let errors = validate_document_diagnostics(&doc);

        let tls = errors
            .iter()
            .find(|e| e.message.contains("Unknown TLS key 'ca_certificate'"))
            .expect("the TLS typo is reported");
        assert_eq!(tls.severity, ErrorSeverity::Warning);
        assert!(tls.message.contains("ca_cert"), "{}", tls.message);

        let proto = errors
            .iter()
            .find(|e| e.message.contains("Unknown PROTO key 'import_path'"))
            .expect("the PROTO typo is reported");
        assert!(proto.message.contains("import_paths"), "{}", proto.message);
    }

    #[test]
    fn canonical_tls_and_proto_keys_are_silent() {
        let src = "--- ENDPOINT ---\npkg.Svc/M\n\n             --- TLS ---\nca_cert: /ca.pem\nclient_cert: /c.pem\nclient_key: /k.pem\n             server_name: api\ninsecure: true\n\n             --- PROTO ---\ndescriptor: a.desc\nfiles: a.proto\nimport_paths: /p\n\n             --- REQUEST ---\n{}\n\n             --- ASSERTS ---\n.ok == true\n";
        let doc = crate::parse_gctf_from_str(src, "t.gctf").expect("parse");
        let errors = validate_document_diagnostics(&doc);
        let noise: Vec<_> = errors
            .iter()
            .filter(|e| e.message.contains("Unknown TLS") || e.message.contains("Unknown PROTO"))
            .collect();
        assert!(noise.is_empty(), "{noise:?}");
    }

    #[test]
    fn free_form_bench_keys_do_not_warn_about_themselves() {
        let src = "--- ENDPOINT ---\npkg.Svc/M\n\n             --- BENCH ---\nname: smoke\nprofile: load\nsources: rows.csv\nload_profile: ramp\n\n             --- REQUEST ---\n{}\n\n             --- ASSERTS ---\n.ok == true\n";
        let doc = crate::parse_gctf_from_str(src, "b.gctf").expect("parse");
        let errors = validate_document_diagnostics(&doc);
        let noise: Vec<_> = errors
            .iter()
            .filter(|e| e.message.contains("Unknown BENCH key"))
            .collect();
        assert!(noise.is_empty(), "{noise:?}");
    }
}
