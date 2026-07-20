//! Allure TestOps compatible reporter.
//!
//! Writes individual JSON files per test in the Allure results format.
//! Each file contains test metadata, status, timing, and gRPC call steps.

use crate::report::Reporter;
use crate::report::kernel;
use crate::state::TestResult;
use anyhow::Result;
use serde::Serialize;
use std::fs;
use std::path::PathBuf;
use uuid::Uuid;

/// Allure reporter — writes one result file per test.
pub struct AllureReporter {
    output_dir: PathBuf,
}

impl AllureReporter {
    /// Create a new Allure reporter writing to `output_dir`.
    /// Creates the directory if it doesn't exist.
    pub fn new(output_dir: PathBuf) -> Self {
        if let Err(e) = fs::create_dir_all(&output_dir) {
            eprintln!("Failed to create allure report directory: {}", e);
        }
        Self { output_dir }
    }
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct AllureResult {
    uuid: String,
    history_id: String,
    full_name: String,
    name: String,
    status: String,
    status_details: Option<StatusDetails>,
    start: u128,
    stop: u128,
    stage: String,
    labels: Vec<Label>,
    #[serde(skip_serializing_if = "Option::is_none")]
    parameters: Option<Vec<Parameter>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    steps: Option<Vec<Step>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    attachments: Option<Vec<Attachment>>,
}

#[derive(Serialize)]
struct StatusDetails {
    #[serde(skip_serializing_if = "Option::is_none")]
    message: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    trace: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    flaky: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    known: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    muted: Option<bool>,
}

#[derive(Serialize)]
struct Label {
    name: String,
    value: String,
}

#[derive(Serialize)]
struct Parameter {
    name: String,
    value: String,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct Step {
    name: String,
    status: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    start: Option<u128>,
    #[serde(skip_serializing_if = "Option::is_none")]
    stop: Option<u128>,
    #[serde(skip_serializing_if = "Option::is_none")]
    attachments: Option<Vec<Attachment>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    steps: Option<Vec<Step>>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct Attachment {
    name: String,
    source: String,
    #[serde(rename = "type")]
    content_type: String,
}

fn extract_test_name(path: &str) -> String {
    std::path::Path::new(path)
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or(path)
        .to_string()
}

fn extract_suite_name(path: &str) -> String {
    std::path::Path::new(path)
        .parent()
        .and_then(|p| p.file_name())
        .and_then(|n| n.to_str())
        .unwrap_or("gRPC Tests")
        .to_string()
}

fn build_grpc_steps(
    test_name: &str,
    result: &TestResult,
    start: u128,
    stop: u128,
) -> Option<Vec<Step>> {
    let calls = kernel::build_kernel_calls(test_name, result)?;
    if calls.is_empty() {
        return None;
    }
    let total_docs = calls.len();

    let total_span = stop.saturating_sub(start);
    let per_step = total_span / total_docs as u128;

    let mut steps = Vec::with_capacity(calls.len());

    for (idx, call) in calls.iter().enumerate() {
        // Use the per-call status resolved by the kernel: it attributes the
        // failure to the actual failing document and marks never-executed
        // documents as skipped instead of falsely passed.
        let status = call.status.as_str();

        let step_start = start + per_step.saturating_mul(idx as u128);
        let step_stop = if idx + 1 == total_docs {
            stop
        } else {
            start + per_step.saturating_mul((idx + 1) as u128)
        };

        let name = format!(
            "{} [{}] (requests: {}, expect: {})",
            call.display_name, call.rpc_mode, call.request_count, call.expectation_kind
        );

        let child_steps: Vec<Step> = call
            .phases
            .iter()
            .map(|phase| Step {
                name: format!("{} ({})", phase.kind.to_uppercase(), phase.details),
                status: phase.status.clone(),
                start: None,
                stop: None,
                attachments: None,
                steps: None,
            })
            .collect();

        steps.push(Step {
            name,
            status: status.to_string(),
            start: Some(step_start),
            stop: Some(step_stop),
            attachments: None,
            steps: if child_steps.is_empty() {
                None
            } else {
                Some(child_steps)
            },
        });
    }

    if steps.is_empty() { None } else { Some(steps) }
}

fn collect_parameters(test_name: &str) -> Vec<Parameter> {
    kernel::runtime_properties(test_name)
        .into_iter()
        .map(|(name, value)| Parameter { name, value })
        .collect()
}

/// Serialize and write an Allure result file atomically: write to a temp file in
/// the same directory, then rename it into place. This prevents an Allure
/// consumer (or a crash) from ever observing a partially written JSON file.
fn write_result_atomically(
    output_dir: &std::path::Path,
    file_path: &std::path::Path,
    report: &AllureResult,
) -> Result<()> {
    let bytes = serde_json::to_vec(report)?;
    let tmp_name = format!(".{}.{}.tmp", report.uuid, std::process::id());
    let tmp_path = output_dir.join(tmp_name);
    fs::write(&tmp_path, &bytes)?;
    if let Err(e) = fs::rename(&tmp_path, file_path) {
        let _ = fs::remove_file(&tmp_path);
        return Err(e.into());
    }
    Ok(())
}

impl Reporter for AllureReporter {
    fn on_test_start(&self, _test_name: &str) {}

    fn on_test_end(&self, test_name: &str, result: &TestResult) {
        let uuid = Uuid::new_v4().to_string();
        let namespace = Uuid::NAMESPACE_OID;
        let history_id = Uuid::new_v5(&namespace, test_name.as_bytes()).to_string();

        let status = match result.status {
            crate::state::TestStatus::Pass => "passed",
            crate::state::TestStatus::Fail => "failed",
            crate::state::TestStatus::Skip => "skipped",
        };

        let status_details = if result.error_message.is_some() {
            Some(StatusDetails {
                message: result.error_message.clone(),
                trace: None,
                flaky: None,
                known: None,
                muted: None,
            })
        } else {
            None
        };

        let now = crate::polyfill::runtime::now_unix_millis();
        let duration = result.duration_ms as u128;
        let start = now.saturating_sub(duration);

        // Use META name if available, otherwise file path
        let display_name = result.meta.name.as_deref().unwrap_or(test_name);
        let test_name_short = extract_test_name(display_name);
        let suite_name = extract_suite_name(test_name);

        let fallback_step = if result.call_duration_ms.is_some() {
            Some(vec![Step {
                name: "gRPC call".to_string(),
                status: if result.status == crate::state::TestStatus::Pass {
                    "passed"
                } else {
                    "failed"
                }
                .to_string(),
                start: Some(start),
                stop: Some(now),
                attachments: None,
                steps: None,
            }])
        } else {
            None
        };

        let steps = build_grpc_steps(test_name, result, start, now).or(fallback_step);

        let mut labels = vec![
            Label {
                name: "language".to_string(),
                value: "gctf".to_string(),
            },
            Label {
                name: "framework".to_string(),
                value: "grpctestify".to_string(),
            },
            Label {
                name: "grpctestify_version".to_string(),
                value: env!("CARGO_PKG_VERSION").to_string(),
            },
            Label {
                name: "suite".to_string(),
                value: suite_name,
            },
            Label {
                name: "feature".to_string(),
                value: "gRPC Test".to_string(),
            },
        ];

        if let Some(ref owner) = result.meta.owner {
            labels.push(Label {
                name: "owner".to_string(),
                value: owner.clone(),
            });
        }

        for tag in &result.meta.tags {
            labels.push(Label {
                name: "tag".to_string(),
                value: tag.clone(),
            });
        }

        // Add gRPC endpoint/service/method labels for fast filtering in Allure UI
        let grpc_labels = kernel::collect_grpc_labels(test_name);
        for endpoint in grpc_labels.endpoints {
            labels.push(Label {
                name: "grpc_endpoint".to_string(),
                value: endpoint,
            });
        }
        for service in grpc_labels.services {
            labels.push(Label {
                name: "grpc_service".to_string(),
                value: service,
            });
        }
        for method in grpc_labels.methods {
            labels.push(Label {
                name: "grpc_method".to_string(),
                value: method,
            });
        }
        for package in grpc_labels.packages {
            labels.push(Label {
                name: "grpc_package".to_string(),
                value: package,
            });
        }

        let report = AllureResult {
            uuid: uuid.clone(),
            history_id,
            full_name: display_name.to_string(),
            name: test_name_short,
            status: status.to_string(),
            status_details,
            start,
            stop: now,
            stage: "finished".to_string(),
            labels,
            parameters: Some(collect_parameters(test_name)),
            steps,
            attachments: None,
        };

        let file_name = format!("{}-result.json", uuid);
        let file_path = self.output_dir.join(file_name);

        if let Err(e) = write_result_atomically(&self.output_dir, &file_path, &report) {
            tracing::warn!("Failed to write Allure report file {:?}: {e}", file_path);
        }
    }

    fn on_suite_end(&self, _results: &crate::state::TestResults) -> Result<()> {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::state::TestResult;

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

    const SINGLE_FIXTURE: &str = "\
--- ENDPOINT ---
pkg.Service/M

--- REQUEST ---
{}

--- RESPONSE ---
{}
";

    #[cfg_attr(miri, ignore)]
    #[test]
    fn test_build_grpc_steps_marks_unreached_document_skipped() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("chain.gctf");
        std::fs::write(&path, CHAIN_FIXTURE).unwrap();
        let path_str = path.to_string_lossy().into_owned();

        // Reference a line inside the SECOND document so it is the failing one.
        let doc = crate::parser::parse_gctf(&path).unwrap();
        let chain: Vec<_> = doc.iter_chain().collect();
        let line = chain[1]
            .sections
            .iter()
            .map(|s| s.start_line)
            .max()
            .unwrap();

        let result = TestResult::fail(
            path_str.clone(),
            format!("Assertion failed (attached to RESPONSE at line {line})"),
            30,
            Some(10),
        );

        let steps = build_grpc_steps(&path_str, &result, 0, 300).unwrap();
        assert_eq!(steps.len(), 3);
        assert_eq!(steps[0].status, "passed");
        assert_eq!(steps[1].status, "failed");
        // Never-executed document is skipped, not passed.
        assert_eq!(steps[2].status, "skipped");
    }

    #[cfg_attr(miri, ignore)]
    #[test]
    fn test_allure_result_written_atomically_as_valid_json() {
        let dir = tempfile::tempdir().unwrap();
        let gctf = dir.path().join("t.gctf");
        std::fs::write(&gctf, SINGLE_FIXTURE).unwrap();
        let gctf_str = gctf.to_string_lossy().into_owned();

        let out_dir = dir.path().join("allure-results");
        let reporter = AllureReporter::new(out_dir.clone());
        let result = TestResult::pass(gctf_str.clone(), 5, Some(3));
        reporter.on_test_end(&gctf_str, &result);

        let entries: Vec<String> = std::fs::read_dir(&out_dir)
            .unwrap()
            .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
            .collect();

        // A finished result file exists and no temp file leaked.
        let result_file = entries
            .iter()
            .find(|n| n.ends_with("-result.json"))
            .unwrap_or_else(|| panic!("no result file: {entries:?}"));
        assert!(
            !entries.iter().any(|n| n.ends_with(".tmp")),
            "temp file leaked: {entries:?}"
        );

        // The written file is complete, parseable JSON.
        let content = std::fs::read_to_string(out_dir.join(result_file)).unwrap();
        let _: serde_json::Value =
            serde_json::from_str(&content).expect("allure result must be valid JSON");
    }
}
