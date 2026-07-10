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

    let request_count = plan.summary.total_requests;
    if request_count > 0 {
        phases.push(KernelPhase {
            kind: "request".to_string(),
            details: format!("messages={}", request_count),
            status: "passed".to_string(),
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
            status: if call_status == "failed" {
                "failed".to_string()
            } else {
                "passed".to_string()
            },
        });
    }

    if doc.first_section(SectionType::Error).is_some() {
        phases.push(KernelPhase {
            kind: "error".to_string(),
            details: "expected error validation".to_string(),
            status: if call_status == "failed" {
                "failed".to_string()
            } else {
                "passed".to_string()
            },
        });
    }

    let assert_blocks = plan.summary.assertion_blocks;
    if assert_blocks > 0 {
        phases.push(KernelPhase {
            kind: "asserts".to_string(),
            details: format!("blocks={}", assert_blocks),
            status: if call_status == "failed" {
                "failed".to_string()
            } else {
                "passed".to_string()
            },
        });
    }

    let extract_blocks = plan.summary.variable_extractions;
    if extract_blocks > 0 {
        phases.push(KernelPhase {
            kind: "extract".to_string(),
            details: format!("blocks={}", extract_blocks),
            status: if call_status == "failed" {
                "failed".to_string()
            } else {
                "passed".to_string()
            },
        });
    }

    phases
}

pub fn build_kernel_calls(test_path: &str, result: &TestResult) -> Option<Vec<KernelCall>> {
    let doc = crate::parser::parse_gctf(std::path::Path::new(test_path)).ok()?;
    let chain: Vec<&GctfDocument> = doc.iter_chain().collect();
    if chain.is_empty() {
        return None;
    }

    let failed_index = if result.status == TestStatus::Fail {
        Some(chain.len().saturating_sub(1))
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

        let status = if failed_index == Some(idx) {
            "failed"
        } else {
            "passed"
        }
        .to_string();

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
    let failed_doc_idx = chain.len().saturating_sub(1);

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
