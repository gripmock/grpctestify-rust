use crate::execution::{ExecutionPlan, Workflow};
use crate::parser::ast::{GctfDocument, SectionType};
use crate::state::{TestResult, TestStatus};
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct KernelReportContext {
    pub tool: String,
    pub version: String,
    pub generated_at: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct KernelPhase {
    pub kind: String,
    pub details: String,
    pub status: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct KernelCall {
    pub call_index: usize,
    pub doc_index: usize,
    pub endpoint: String,
    pub package: Option<String>,
    pub service: Option<String>,
    pub method: Option<String>,
    pub request_count: usize,
    pub expectation_kind: String,
    pub rpc_mode: String,
    pub display_name: String,
    pub status: String,
    pub phases: Vec<KernelPhase>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct KernelGrpcLabels {
    pub endpoints: Vec<String>,
    pub packages: Vec<String>,
    pub services: Vec<String>,
    pub methods: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct KernelFailure {
    pub call_index: usize,
    pub phase: String,
    pub category: String,
    pub message: String,
    pub grpc_code: Option<i32>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct KernelTestCase {
    pub case_index: usize,
    pub test_path: String,
    pub display_name: String,
    pub status: String,
    pub duration_ms: Option<u64>,
    pub calls: Vec<KernelCall>,
    pub failures: Vec<KernelFailure>,
    pub grpc_labels: KernelGrpcLabels,
}

fn call_display_name(doc: &GctfDocument, call_index: usize, endpoint: &str) -> String {
    let named = doc
        .sections
        .iter()
        .find_map(|s| s.get_attribute("name").map(|a| a.value.trim().to_string()))
        .filter(|s| !s.is_empty());

    match named {
        Some(name) => format!("#{} {} ({})", call_index, name, endpoint),
        None => format!("#{} {}", call_index, endpoint),
    }
}

fn call_phases(doc: &GctfDocument, call_status: &str) -> Vec<KernelPhase> {
    let plan = ExecutionPlan::from_document(doc);
    let mut phases = Vec::new();

    // A validation phase reflects the call status: failed for the failing call,
    // skipped for calls the chain never reached, otherwise passed.
    let validation_status = || {
        match call_status {
            "failed" => "failed",
            "skipped" => "skipped",
            _ => "passed",
        }
        .to_string()
    };
    // The request phase only "passes" when the call actually ran.
    let request_status = if call_status == "skipped" {
        "skipped".to_string()
    } else {
        "passed".to_string()
    };

    let request_count = plan.summary.total_requests;
    if request_count > 0 {
        phases.push(KernelPhase {
            kind: "request".to_string(),
            details: format!("messages={}", request_count),
            status: request_status,
        });
    }

    let response_sections = doc.sections_by_type(SectionType::Response);
    if !response_sections.is_empty() {
        phases.push(KernelPhase {
            kind: "response".to_string(),
            details: format!(
                "sections={}, total_messages={}",
                response_sections.len(),
                plan.summary.total_responses
            ),
            status: validation_status(),
        });
    }

    if doc.first_section(SectionType::Error).is_some() {
        phases.push(KernelPhase {
            kind: "error".to_string(),
            details: "expected error validation".to_string(),
            status: validation_status(),
        });
    }

    let assert_blocks = plan.summary.assertion_blocks;
    if assert_blocks > 0 {
        phases.push(KernelPhase {
            kind: "asserts".to_string(),
            details: format!("blocks={}", assert_blocks),
            status: validation_status(),
        });
    }

    let extract_blocks = plan.summary.variable_extractions;
    if extract_blocks > 0 {
        phases.push(KernelPhase {
            kind: "extract".to_string(),
            details: format!("blocks={}", extract_blocks),
            status: validation_status(),
        });
    }

    phases
}

/// Extract the first line number referenced by an error message.
///
/// Failure messages embed the absolute file line of the offending section
/// (e.g. `"ASSERTS section at line 42 ..."`). Chain documents preserve absolute
/// line numbers, so this lets us map a failure back to the document it belongs
/// to instead of blindly blaming the last document in the chain.
fn extract_error_line(message: &str) -> Option<usize> {
    let mut rest = message;
    while let Some(pos) = rest.find("line ") {
        let after = &rest[pos + "line ".len()..];
        let digits: String = after.chars().take_while(|c| c.is_ascii_digit()).collect();
        if let Ok(n) = digits.parse::<usize>() {
            return Some(n);
        }
        rest = after;
    }
    None
}

/// Absolute file line range `[start, end]` covered by a document's sections.
fn doc_line_range(doc: &GctfDocument) -> Option<(usize, usize)> {
    let mut start = usize::MAX;
    let mut end = 0usize;
    for s in &doc.sections {
        start = start.min(s.start_line);
        end = end.max(s.end_line.max(s.start_line));
    }
    if start == usize::MAX {
        None
    } else {
        Some((start, end))
    }
}

/// Determine which document in the chain actually failed.
///
/// Under fail-fast execution the failure belongs to a single document; the ones
/// after it were never executed. We locate it via the line number embedded in
/// the error message. When no line can be extracted (e.g. an opaque transport
/// error) we fall back to the last document, preserving prior behaviour.
fn resolve_failed_doc_index(chain: &[&GctfDocument], error_message: &Option<String>) -> usize {
    let last = chain.len().saturating_sub(1);
    let Some(msg) = error_message else {
        return last;
    };
    let Some(line) = extract_error_line(msg) else {
        return last;
    };
    for (idx, d) in chain.iter().enumerate() {
        if let Some((start, end)) = doc_line_range(d)
            && line >= start
            && line <= end
        {
            return idx;
        }
    }
    last
}

/// Per-document status within a chain, given the resolved failing index.
/// Documents before the failure passed; the failing one failed; documents after
/// it were never executed (fail-fast) and are reported as skipped, not passed.
fn chain_call_status(idx: usize, failed_index: Option<usize>) -> &'static str {
    match failed_index {
        Some(f) if idx == f => "failed",
        Some(f) if idx > f => "skipped",
        _ => "passed",
    }
}

pub fn build_kernel_calls(test_path: &str, result: &TestResult) -> Option<Vec<KernelCall>> {
    let doc = crate::parser::parse_gctf(std::path::Path::new(test_path)).ok()?;
    let chain: Vec<&GctfDocument> = doc.iter_chain().collect();
    if chain.is_empty() {
        return None;
    }

    let failed_index = if result.status == TestStatus::Fail {
        Some(resolve_failed_doc_index(&chain, &result.error_message))
    } else {
        None
    };

    let mut calls = Vec::with_capacity(chain.len());
    for (idx, d) in chain.iter().enumerate() {
        let endpoint = d
            .get_endpoint()
            .unwrap_or_else(|| "<missing endpoint>".to_string());
        let parsed = d.parse_endpoint();
        let plan = ExecutionPlan::from_document(d);
        let workflow = Workflow::from_document_with_analysis(d);
        let request_count = plan.summary.total_requests;

        let has_error_expectation = d.first_section(SectionType::Error).is_some();
        let has_response_expectation = d.first_section(SectionType::Response).is_some();
        let expectation_kind = if has_error_expectation {
            "ERROR"
        } else if has_response_expectation {
            "RESPONSE"
        } else {
            "ASSERTS"
        }
        .to_string();

        let status = chain_call_status(idx, failed_index).to_string();

        let (package, service, method) = match parsed {
            Some((pkg, svc, mtd)) => {
                let pkg = if pkg.is_empty() { None } else { Some(pkg) };
                (pkg, Some(svc), Some(mtd))
            }
            None => (None, None, None),
        };

        calls.push(KernelCall {
            call_index: idx + 1,
            doc_index: idx + 1,
            endpoint: endpoint.clone(),
            package,
            service,
            method,
            request_count,
            expectation_kind,
            rpc_mode: workflow.rpc_mode_name().to_string(),
            display_name: call_display_name(d, idx + 1, &endpoint),
            phases: call_phases(d, &status),
            status,
        });
    }

    Some(calls)
}

pub fn collect_grpc_labels(test_path: &str) -> KernelGrpcLabels {
    let doc = match crate::parser::parse_gctf(std::path::Path::new(test_path)) {
        Ok(d) => d,
        Err(_) => return KernelGrpcLabels::default(),
    };

    let mut endpoints = BTreeSet::new();
    let mut services = BTreeSet::new();
    let mut methods = BTreeSet::new();
    let mut packages = BTreeSet::new();

    for d in doc.iter_chain() {
        if let Some(endpoint) = d.get_endpoint() {
            endpoints.insert(endpoint);
        }
        if let Some((pkg, service, method)) = d.parse_endpoint() {
            if !pkg.is_empty() {
                packages.insert(pkg);
            }
            services.insert(service);
            methods.insert(method);
        }
    }

    KernelGrpcLabels {
        endpoints: endpoints.into_iter().collect(),
        packages: packages.into_iter().collect(),
        services: services.into_iter().collect(),
        methods: methods.into_iter().collect(),
    }
}

pub fn report_context() -> KernelReportContext {
    KernelReportContext {
        tool: "grpctestify".to_string(),
        version: env!("CARGO_PKG_VERSION").to_string(),
        generated_at: crate::polyfill::runtime::now_timestamp(),
    }
}

pub fn runtime_properties(test_path: &str) -> Vec<(String, String)> {
    let mut props = vec![(
        "grpctestify.version".to_string(),
        env!("CARGO_PKG_VERSION").to_string(),
    )];

    let doc = match crate::parser::parse_gctf(std::path::Path::new(test_path)) {
        Ok(d) => d,
        Err(_) => return props,
    };

    props.push((
        "documents.count".to_string(),
        doc.document_count().to_string(),
    ));

    if let Ok(runtime) = crate::execution::runner_helpers::resolve_effective_runtime_options(
        &doc,
        crate::execution::runner_helpers::CliRuntimeDefaults {
            timeout_seconds: 30,
            retry: 0,
            retry_delay_seconds: 1.0,
            no_retry: false,
        },
    ) {
        props.push((
            "runtime.timeout".to_string(),
            runtime.timeout_seconds.value.to_string(),
        ));
        props.push(("runtime.retry".to_string(), runtime.retry.value.to_string()));
        props.push((
            "runtime.retry_delay".to_string(),
            runtime.retry_delay_seconds.value.to_string(),
        ));
        props.push((
            "runtime.no_retry".to_string(),
            runtime.no_retry.value.to_string(),
        ));
        props.push(("runtime.compression".to_string(), runtime.compression.value));
    }

    props
}

pub fn build_kernel_test_case(test_path: &str, result: &TestResult) -> Option<KernelTestCase> {
    let doc = crate::parser::parse_gctf(std::path::Path::new(test_path)).ok()?;
    let calls = build_kernel_calls(test_path, result)?;
    let grpc_labels = collect_grpc_labels(test_path);
    let failures = extract_failures(test_path, result);

    let display_name = doc
        .sections
        .iter()
        .find_map(|s| s.get_attribute("name").map(|a| a.value.trim().to_string()))
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| {
            std::path::Path::new(test_path)
                .file_name()
                .map(|n| n.to_string_lossy().to_string())
                .unwrap_or_else(|| test_path.to_string())
        });

    Some(KernelTestCase {
        case_index: 0,
        test_path: test_path.to_string(),
        display_name,
        status: match result.status {
            TestStatus::Pass => "passed".to_string(),
            TestStatus::Fail => "failed".to_string(),
            TestStatus::Skip => "skipped".to_string(),
        },
        duration_ms: Some(result.duration_ms),
        calls,
        failures,
        grpc_labels,
    })
}

fn extract_failures(test_path: &str, result: &TestResult) -> Vec<KernelFailure> {
    let mut failures = Vec::new();
    if result.status != TestStatus::Fail {
        return failures;
    }

    let doc = match crate::parser::parse_gctf(std::path::Path::new(test_path)) {
        Ok(d) => d,
        Err(_) => return failures,
    };

    let chain: Vec<&GctfDocument> = doc.iter_chain().collect();
    let failed_doc_idx = resolve_failed_doc_index(&chain, &result.error_message);

    failures.push(KernelFailure {
        call_index: failed_doc_idx + 1,
        phase: "execution".to_string(),
        category: categorize_failure(&result.error_message),
        message: result.error_message.clone().unwrap_or_default(),
        grpc_code: extract_grpc_code(&result.error_message),
    });

    failures
}

fn categorize_failure(error_msg: &Option<String>) -> String {
    let msg = match error_msg {
        Some(m) => m.to_lowercase(),
        None => return "unknown".to_string(),
    };

    if msg.contains("timeout") {
        "timeout".to_string()
    } else if msg.contains("connection") || msg.contains("network") {
        "connection_error".to_string()
    } else if msg.contains("assertion") || msg.contains("diff") || msg.contains("expected") {
        "assertion_failure".to_string()
    } else if msg.contains("parse") || msg.contains("syntax") {
        "parse_error".to_string()
    } else if msg.contains("validation") {
        "validation_error".to_string()
    } else {
        "execution_error".to_string()
    }
}

fn extract_grpc_code(error_msg: &Option<String>) -> Option<i32> {
    let msg = error_msg.as_ref()?;

    if let Some(pos) = msg.find("gRPC code ") {
        let rest = &msg[pos + 10..];
        if let Some(end) = rest.find([' ', ',', ')']) {
            let code_str = &rest[..end];
            return code_str.parse().ok();
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    const CHAIN_FIXTURE: &str = "\
--- ENDPOINT ---
pkg.Service/MethodA

--- REQUEST ---
{}

--- RESPONSE ---
{}

--- ENDPOINT ---
pkg.Service/MethodB

--- REQUEST ---
{}

--- RESPONSE ---
{}

--- ENDPOINT ---
pkg.Service/MethodC

--- REQUEST ---
{}

--- RESPONSE ---
{}
";

    fn write_fixture() -> (tempfile::TempDir, String) {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("chain.gctf");
        std::fs::write(&path, CHAIN_FIXTURE).unwrap();
        let path_str = path.to_string_lossy().into_owned();
        (dir, path_str)
    }

    #[test]
    fn test_extract_error_line() {
        assert_eq!(
            extract_error_line("ASSERTS section at line 42 has no context"),
            Some(42)
        );
        assert_eq!(extract_error_line("no numbers here"), None);
        assert_eq!(extract_error_line("gRPC code 5 (NOT_FOUND)"), None);
    }

    #[cfg_attr(miri, ignore)]
    #[test]
    fn test_chain_has_three_documents() {
        let (_dir, path) = write_fixture();
        let doc = crate::parser::parse_gctf(std::path::Path::new(&path)).unwrap();
        assert_eq!(doc.document_count(), 3);
    }

    #[cfg_attr(miri, ignore)]
    #[test]
    fn test_failure_attributed_to_actual_document_not_last() {
        let (_dir, path) = write_fixture();
        // Determine an absolute line that falls inside the SECOND document,
        // then craft an error message referencing it.
        let doc = crate::parser::parse_gctf(std::path::Path::new(&path)).unwrap();
        let chain: Vec<&GctfDocument> = doc.iter_chain().collect();
        let (start, end) = doc_line_range(chain[1]).unwrap();
        let mid = (start + end) / 2;

        let result = TestResult::fail(
            path.clone(),
            format!("Assertion failed (attached to RESPONSE at line {mid})"),
            10,
            None,
        );

        let calls = build_kernel_calls(&path, &result).unwrap();
        assert_eq!(calls.len(), 3);
        // Document before the failure passed.
        assert_eq!(calls[0].status, "passed");
        // The failing document is the one that actually failed, not the last.
        assert_eq!(calls[1].status, "failed");
        // The never-executed document is skipped, not falsely passed.
        assert_eq!(calls[2].status, "skipped");

        // Its phases must not claim "passed" either.
        assert!(
            calls[2].phases.iter().all(|p| p.status == "skipped"),
            "skipped call phases: {:?}",
            calls[2].phases
        );

        // extract_failures must point at document #2, not the last document.
        let failures = extract_failures(&path, &result);
        assert_eq!(failures.len(), 1);
        assert_eq!(failures[0].call_index, 2);
    }

    #[cfg_attr(miri, ignore)]
    #[test]
    fn test_failure_without_line_falls_back_to_last_document() {
        let (_dir, path) = write_fixture();
        let result = TestResult::fail(path.clone(), "connection refused".to_string(), 10, None);
        let calls = build_kernel_calls(&path, &result).unwrap();
        assert_eq!(calls[0].status, "passed");
        assert_eq!(calls[1].status, "passed");
        assert_eq!(calls[2].status, "failed");
    }

    #[cfg_attr(miri, ignore)]
    #[test]
    fn test_passing_chain_marks_all_passed() {
        let (_dir, path) = write_fixture();
        let result = TestResult::pass(path.clone(), 10, None);
        let calls = build_kernel_calls(&path, &result).unwrap();
        assert!(calls.iter().all(|c| c.status == "passed"));
        assert!(extract_failures(&path, &result).is_empty());
    }
}
