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

    let validation_status = || {
        match call_status {
            "failed" => "failed",
            "skipped" => "skipped",
            _ => "passed",
        }
        .to_string()
    };
    let request_status = if call_status == "skipped" {
        "skipped".to_string()
    } else {
        "passed".to_string()
    };

    let request_count: usize = plan
        .requests
        .iter()
        .map(|r| {
            if r.content_type == "json-lines" {
                r.content.as_array().map_or(1, |v| v.len().max(1))
            } else {
                1
            }
        })
        .sum();
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
                plan.expectations
                    .iter()
                    .filter(|e| e.expectation_type == "response")
                    .map(|e| e.message_count.unwrap_or(1).max(1))
                    .sum::<usize>()
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
        let single = d.detached();
        let workflow = Workflow::from_document_with_analysis(&single);
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
            rpc_mode: if d.transport() == crate::parser::ast::Transport::Http {
                "HTTP".to_string()
            } else {
                workflow.rpc_mode_name().to_string()
            },
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
    fn chain_has_three_documents() {
        let (_dir, path) = write_fixture();
        let doc = crate::parser::parse_gctf(std::path::Path::new(&path)).unwrap();
        assert_eq!(doc.document_count(), 3);
    }

    #[cfg_attr(miri, ignore)]
    #[test]
    fn failure_attributed_to_actual_document_not_last() {
        let (_dir, path) = write_fixture();
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
        assert_eq!(calls[0].status, "passed");
        assert_eq!(calls[1].status, "failed");
        assert_eq!(calls[2].status, "skipped");

        assert!(
            calls[2].phases.iter().all(|p| p.status == "skipped"),
            "skipped call phases: {:?}",
            calls[2].phases
        );
    }

    #[cfg_attr(miri, ignore)]
    #[test]
    fn failure_without_line_falls_back_to_last_document() {
        let (_dir, path) = write_fixture();
        let result = TestResult::fail(path.clone(), "connection refused".to_string(), 10, None);
        let calls = build_kernel_calls(&path, &result).unwrap();
        assert_eq!(calls[0].status, "passed");
        assert_eq!(calls[1].status, "passed");
        assert_eq!(calls[2].status, "failed");
    }

    #[cfg_attr(miri, ignore)]
    #[test]
    fn passing_chain_marks_all_passed() {
        let (_dir, path) = write_fixture();
        let result = TestResult::pass(path.clone(), 10, None);
        let calls = build_kernel_calls(&path, &result).unwrap();
        assert!(calls.iter().all(|c| c.status == "passed"));
    }
}
