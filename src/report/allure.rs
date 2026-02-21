use crate::report::Reporter;
use crate::state::{TestResult, TestResults};
use anyhow::Result;
use serde::Serialize;
use std::fs;
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};
use uuid::Uuid;

pub struct AllureReporter {
    output_dir: PathBuf,
}

impl AllureReporter {
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

        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_millis();
        let duration = result.duration_ms as u128;
        let start = now.saturating_sub(duration);

        let test_name_short = extract_test_name(test_name);
        let suite_name = extract_suite_name(test_name);

        let grpc_step = if result.grpc_duration_ms.is_some() {
            Some(Step {
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
            })
        } else {
            None
        };

        let steps = grpc_step.map(|s| vec![s]);

        let report = AllureResult {
            uuid: uuid.clone(),
            history_id,
            full_name: test_name.to_string(),
            name: test_name_short,
            status: status.to_string(),
            status_details,
            start,
            stop: now,
            stage: "finished".to_string(),
            labels: vec![
                Label {
                    name: "language".to_string(),
                    value: "rust".to_string(),
                },
                Label {
                    name: "framework".to_string(),
                    value: "grpctestify".to_string(),
                },
                Label {
                    name: "suite".to_string(),
                    value: suite_name,
                },
                Label {
                    name: "feature".to_string(),
                    value: "gRPC Test".to_string(),
                },
            ],
            steps,
            attachments: None,
        };

        let file_name = format!("{}-result.json", uuid);
        let file_path = self.output_dir.join(file_name);

        if let Ok(file) = fs::File::create(&file_path) {
            let _ = serde_json::to_writer(&file, &report);
        } else {
            eprintln!("Failed to write allure report to {:?}", file_path);
        }
    }

    fn on_suite_end(&self, _results: &TestResults) -> Result<()> {
        Ok(())
    }
}
