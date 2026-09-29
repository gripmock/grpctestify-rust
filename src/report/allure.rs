use crate::report::Reporter;
use crate::report::kernel;
use crate::state::TestResult;
use anyhow::Result;
use serde::Serialize;
use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{LazyLock, Mutex};
use uuid::Uuid;

static HOST_LABEL: LazyLock<String> = LazyLock::new(|| {
    gethostname::gethostname()
        .into_string()
        .unwrap_or_else(|_| "unknown-host".to_string())
});

fn host_label() -> String {
    HOST_LABEL.clone()
}

pub struct AllureReporter {
    output_dir: PathBuf,
    address: Option<String>,
    fixture_state: Mutex<HashMap<PathBuf, DirContainerState>>,
}

#[derive(Default)]
struct DirContainerState {
    children: Vec<String>,
    befores: Vec<FixtureStep>,
    afters: Vec<FixtureStep>,
}

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
struct FixtureStep {
    name: String,
    status: String,
    start: u128,
    stop: u128,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct AllureContainer {
    uuid: String,
    name: String,
    children: Vec<String>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    befores: Vec<FixtureStep>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    afters: Vec<FixtureStep>,
}

enum FixtureKind {
    Setup,
    Teardown,
}

fn fixture_kind(test_name: &str) -> Option<FixtureKind> {
    match crate::commands::run::fixture_role(Path::new(test_name)) {
        Some(crate::commands::run::FixtureRole::Setup) => Some(FixtureKind::Setup),
        Some(crate::commands::run::FixtureRole::Teardown) => Some(FixtureKind::Teardown),
        None => None,
    }
}

impl AllureReporter {
    pub fn new(output_dir: PathBuf) -> Self {
        if let Err(e) = fs::create_dir_all(&output_dir) {
            eprintln!("Failed to create allure report directory: {}", e);
        }
        Self {
            output_dir,
            address: None,
            fixture_state: Mutex::new(HashMap::new()),
        }
    }

    pub fn with_address(mut self, address: String) -> Self {
        self.address = Some(address);
        self
    }
}

const CATEGORIES_JSON: &str = r#"[
  {"name": "Assertion mismatch", "matchedStatuses": ["failed"], "messageRegex": "(?s).*(ssertion|xpected|xpect).*"},
  {"name": "gRPC status error", "matchedStatuses": ["failed"], "messageRegex": "(?s).*gRPC code.*"},
  {"name": "Connection error", "matchedStatuses": ["failed"], "messageRegex": "(?s).*(onnect|navailable|ransport|refused).*"},
  {"name": "Timeout", "matchedStatuses": ["failed"], "messageRegex": "(?s).*(imeout|eadline).*"},
  {"name": "Parse error", "matchedStatuses": ["failed"], "messageRegex": "(?s).*(arse|nvalid|yntax).*"},
  {"name": "Validation error", "matchedStatuses": ["failed"], "messageRegex": "(?s).*alidation.*"}
]"#;

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct AllureResult {
    uuid: String,
    history_id: String,
    test_case_id: String,
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
    #[serde(skip_serializing_if = "Option::is_none")]
    description: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    links: Option<Vec<Link>>,
}

#[derive(Serialize)]
struct Link {
    #[serde(skip_serializing_if = "Option::is_none")]
    name: Option<String>,
    url: String,
    #[serde(rename = "type")]
    link_type: &'static str,
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
    status_details: Option<StatusDetails>,
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

    let has_real_durations = !result.document_durations_ms.is_empty();
    let total_span = stop.saturating_sub(start);
    let per_step = total_span / total_docs as u128;

    let mut steps = Vec::with_capacity(calls.len());
    let mut cursor = start;

    for (idx, call) in calls.iter().enumerate() {
        let status = call.status.as_str();

        let is_last = idx + 1 == total_docs;
        let (step_start, step_stop) = if has_real_durations {
            let step_start = cursor;
            let step_stop = if is_last {
                stop
            } else {
                let doc_ms = result.document_durations_ms.get(idx).copied().unwrap_or(0);
                cursor + doc_ms as u128
            };
            cursor = step_stop;
            (step_start, step_stop)
        } else {
            let step_start = start + per_step.saturating_mul(idx as u128);
            let step_stop = if is_last {
                stop
            } else {
                start + per_step.saturating_mul((idx + 1) as u128)
            };
            (step_start, step_stop)
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
                status_details: None,
                start: None,
                stop: None,
                attachments: None,
                steps: None,
            })
            .collect();

        steps.push(Step {
            name,
            status: status.to_string(),
            status_details: None,
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

fn build_assertion_steps(result: &TestResult) -> Option<Step> {
    if result.assertions.is_empty() {
        return None;
    }

    let mut children = Vec::with_capacity(result.assertions.len());
    let mut any_failed = false;

    for rec in &result.assertions {
        let status = if rec.passed { "passed" } else { "failed" };
        if !rec.passed {
            any_failed = true;
        }

        let status_details = if rec.passed {
            None
        } else {
            Some(StatusDetails {
                message: rec.message.clone(),
                trace: assertion_diff(rec),
                flaky: None,
                known: None,
                muted: None,
            })
        };

        children.push(Step {
            name: format!("line {}: {}", rec.line, rec.expression),
            status: status.to_string(),
            status_details,
            start: None,
            stop: None,
            attachments: None,
            steps: None,
        });
    }

    Some(Step {
        name: format!("Assertions ({})", children.len()),
        status: if any_failed { "failed" } else { "passed" }.to_string(),
        status_details: None,
        start: None,
        stop: None,
        attachments: None,
        steps: Some(children),
    })
}

fn assertion_diff(rec: &crate::state::AssertionRecord) -> Option<String> {
    match (&rec.expected, &rec.actual) {
        (None, None) => None,
        (expected, actual) => Some(format!(
            "expected: {}\nactual:   {}",
            expected.as_deref().unwrap_or("<none>"),
            actual.as_deref().unwrap_or("<none>"),
        )),
    }
}

fn failed_assertions_trace(result: &TestResult) -> Option<String> {
    let mut blocks: Vec<String> = Vec::new();
    for rec in result.assertions.iter().filter(|r| !r.passed) {
        let mut block = format!("line {}: {}", rec.line, rec.expression);
        if let Some(diff) = assertion_diff(rec) {
            block.push('\n');
            block.push_str(&diff);
        } else if let Some(msg) = &rec.message {
            block.push('\n');
            block.push_str(msg);
        }
        blocks.push(block);
    }
    if blocks.is_empty() {
        None
    } else {
        Some(blocks.join("\n\n"))
    }
}

fn collect_parameters(test_name: &str, result: &TestResult) -> Vec<Parameter> {
    kernel::runtime_properties(test_name)
        .into_iter()
        .chain(result.row_params.iter().cloned())
        .map(|(name, value)| Parameter { name, value })
        .collect()
}

fn write_exchange_attachment(
    output_dir: &std::path::Path,
    uuid: &str,
    exchange: &Option<crate::state::CapturedExchange>,
) -> Option<Attachment> {
    let exchange = exchange.as_ref()?;
    let file_name = format!("{}-exchange-attachment.json", uuid);
    let file_path = output_dir.join(&file_name);
    let body = serde_json::json!({
        "headers": exchange.headers,
        "trailers": exchange.trailers,
        "response": exchange.response,
        "truncated": exchange.truncated,
    });
    let bytes = serde_json::to_vec_pretty(&body).ok()?;
    if let Err(e) = fs::write(&file_path, bytes) {
        tracing::warn!("Failed to write Allure exchange attachment {file_path:?}: {e}");
        return None;
    }
    Some(Attachment {
        name: "Captured exchange".to_string(),
        source: file_name,
        content_type: "application/json".to_string(),
    })
}

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

fn build_executor_json() -> Option<String> {
    build_executor_json_from(|k| std::env::var(k).ok())
}

fn build_executor_json_from(get: impl Fn(&str) -> Option<String>) -> Option<String> {
    if get("GITHUB_ACTIONS").as_deref() == Some("true") {
        let server = get("GITHUB_SERVER_URL").unwrap_or_else(|| "https://github.com".to_string());
        let repo = get("GITHUB_REPOSITORY").unwrap_or_default();
        let run_id = get("GITHUB_RUN_ID").unwrap_or_default();
        let run_number = get("GITHUB_RUN_NUMBER").unwrap_or_default();
        let body = serde_json::json!({
            "name": "GitHub Actions",
            "type": "github",
            "buildName": format!("{repo} #{run_number}"),
            "buildUrl": format!("{server}/{repo}/actions/runs/{run_id}"),
            "reportName": "grpctestify",
        });
        return serde_json::to_string_pretty(&body).ok();
    }

    if matches!(get("CI").as_deref(), Some("true") | Some("1")) {
        let body = serde_json::json!({
            "name": get("CI_NAME").unwrap_or_else(|| "CI".to_string()),
            "type": "ci",
            "buildName": get("BUILD_NUMBER"),
            "buildUrl": get("BUILD_URL"),
            "reportName": "grpctestify",
        });
        return serde_json::to_string_pretty(&body).ok();
    }

    None
}

impl Reporter for AllureReporter {
    fn on_test_start(&self, _test_name: &str) {}

    fn on_test_end(&self, test_name: &str, result: &TestResult) {
        let status = match result.status {
            crate::state::TestStatus::Pass => "passed",
            crate::state::TestStatus::Fail => "failed",
            crate::state::TestStatus::Skip => "skipped",
        };

        if let Some(kind) = fixture_kind(test_name) {
            let now = crate::polyfill::runtime::now_unix_millis();
            let start = now.saturating_sub(result.duration_ms as u128);
            let dir = Path::new(test_name)
                .parent()
                .map(Path::to_path_buf)
                .unwrap_or_default();
            let step = FixtureStep {
                name: result
                    .meta
                    .name
                    .clone()
                    .unwrap_or_else(|| test_name.to_string()),
                status: status.to_string(),
                start,
                stop: now,
            };
            if let Ok(mut state) = self.fixture_state.lock() {
                let entry = state.entry(dir).or_default();
                match kind {
                    FixtureKind::Setup => entry.befores.push(step),
                    FixtureKind::Teardown => entry.afters.push(step),
                }
            }
            return;
        }

        let uuid = Uuid::new_v4().to_string();
        let namespace = Uuid::NAMESPACE_OID;
        let history_id = Uuid::new_v5(&namespace, test_name.as_bytes()).to_string();

        let is_flaky = result.status == crate::state::TestStatus::Pass && result.retried;
        let status_details = if result.error_message.is_some() || is_flaky {
            Some(StatusDetails {
                message: result.error_message.clone(),
                trace: failed_assertions_trace(result),
                flaky: is_flaky.then_some(true),
                known: None,
                muted: None,
            })
        } else {
            None
        };

        let now = crate::polyfill::runtime::now_unix_millis();
        let duration = result.duration_ms as u128;
        let start = now.saturating_sub(duration);

        let display_name = result.meta.name.as_deref().unwrap_or(test_name);
        let test_name_short = extract_test_name(display_name);
        let suite_name = extract_suite_name(test_name);
        let test_case_id = Uuid::new_v5(&namespace, test_name.as_bytes()).to_string();

        let fallback_step = if result.call_duration_ms.is_some() {
            Some(vec![Step {
                name: if result.family == "httf" {
                    "HTTP call".to_string()
                } else {
                    "gRPC call".to_string()
                },
                status: if result.status == crate::state::TestStatus::Pass {
                    "passed"
                } else {
                    "failed"
                }
                .to_string(),
                status_details: None,
                start: Some(start),
                stop: Some(now),
                attachments: None,
                steps: None,
            }])
        } else {
            None
        };

        let mut steps = build_grpc_steps(test_name, result, start, now).or(fallback_step);
        if let Some(assertion_step) = build_assertion_steps(result) {
            steps.get_or_insert_with(Vec::new).push(assertion_step);
        }

        let grpc_labels = kernel::collect_grpc_labels(test_name);
        let first_package = grpc_labels.packages.first().cloned();
        let first_service = grpc_labels.services.first().cloned();
        let first_method = grpc_labels.methods.first().cloned();

        let mut labels = vec![
            Label {
                name: "language".to_string(),
                value: crate::parser::ast::Family::of(test_name).ext().to_string(),
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
                name: "host".to_string(),
                value: host_label(),
            },
            Label {
                name: "thread".to_string(),
                value: format!("{:?}", std::thread::current().id()),
            },
            Label {
                name: "suite".to_string(),
                value: first_service.clone().unwrap_or(suite_name),
            },
            Label {
                name: "feature".to_string(),
                value: first_service
                    .clone()
                    .unwrap_or_else(|| "gRPC Test".to_string()),
            },
        ];

        if let Some(package) = first_package.clone() {
            labels.push(Label {
                name: "parentSuite".to_string(),
                value: package.clone(),
            });
            labels.push(Label {
                name: "epic".to_string(),
                value: package,
            });
        }
        if let Some(method) = first_method.clone() {
            labels.push(Label {
                name: "subSuite".to_string(),
                value: method.clone(),
            });
            labels.push(Label {
                name: "story".to_string(),
                value: method,
            });
        }

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

        let attachments = write_exchange_attachment(&self.output_dir, &uuid, &result.exchange)
            .map(|attachment| vec![attachment]);

        let report = AllureResult {
            uuid: uuid.clone(),
            history_id,
            test_case_id,
            full_name: display_name.to_string(),
            name: test_name_short,
            status: status.to_string(),
            status_details,
            start,
            stop: now,
            stage: "finished".to_string(),
            labels,
            parameters: Some(collect_parameters(test_name, result)),
            steps,
            attachments,
            description: result.meta.summary.clone(),
            links: if result.meta.links.is_empty() {
                None
            } else {
                Some(
                    result
                        .meta
                        .links
                        .iter()
                        .map(|url| Link {
                            name: None,
                            url: url.clone(),
                            link_type: "custom",
                        })
                        .collect(),
                )
            },
        };

        let file_name = format!("{}-result.json", uuid);
        let file_path = self.output_dir.join(file_name);

        if let Err(e) = write_result_atomically(&self.output_dir, &file_path, &report) {
            tracing::warn!("Failed to write Allure report file {:?}: {e}", file_path);
        }

        let dir = Path::new(test_name)
            .parent()
            .map(Path::to_path_buf)
            .unwrap_or_default();
        if let Ok(mut state) = self.fixture_state.lock() {
            state.entry(dir).or_default().children.push(uuid);
        }
    }

    fn on_suite_end(&self, results: &crate::state::TestResults) -> Result<()> {
        let categories_path = self.output_dir.join("categories.json");
        if let Err(e) = fs::write(&categories_path, CATEGORIES_JSON) {
            tracing::warn!("Failed to write Allure categories.json: {e}");
        }

        let m = &results.metrics;
        let mut env = format!(
            "grpctestify.version={}\ngrpctestify.parallel_jobs={}\ntests.total={}\ntests.rpc_calls={}\n",
            env!("CARGO_PKG_VERSION"),
            m.parallel_jobs,
            results.total(),
            m.rpc_calls,
        );
        if let Some(address) = &self.address {
            env.push_str(&format!("grpctestify.address={address}\n"));
        }
        let env_path = self.output_dir.join("environment.properties");
        if let Err(e) = fs::write(&env_path, env) {
            tracing::warn!("Failed to write Allure environment.properties: {e}");
        }

        if let Some(executor) = build_executor_json()
            && let Err(e) = fs::write(self.output_dir.join("executor.json"), executor)
        {
            tracing::warn!("Failed to write Allure executor.json: {e}");
        }

        if let Ok(state) = self.fixture_state.lock() {
            for (dir, entry) in state.iter() {
                if entry.befores.is_empty() && entry.afters.is_empty() {
                    continue;
                }
                let container = AllureContainer {
                    uuid: Uuid::new_v4().to_string(),
                    name: extract_test_name(&dir.to_string_lossy()),
                    children: entry.children.clone(),
                    befores: entry.befores.clone(),
                    afters: entry.afters.clone(),
                };
                let path = self
                    .output_dir
                    .join(format!("{}-container.json", container.uuid));
                if let Err(e) = fs::write(&path, serde_json::to_vec(&container).unwrap_or_default())
                {
                    tracing::warn!("Failed to write Allure container {path:?}: {e}");
                }
            }
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {

    #[test]
    fn the_language_label_is_the_family_of_the_file() {
        assert_eq!(
            crate::parser::ast::Family::of("api/users.httf").ext(),
            "httf"
        );
        assert_eq!(
            crate::parser::ast::Family::of("api/users.gctf").ext(),
            "gctf"
        );
    }
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
    fn build_grpc_steps_marks_unreached_document_skipped() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("chain.gctf");
        std::fs::write(&path, CHAIN_FIXTURE).unwrap();
        let path_str = path.to_string_lossy().into_owned();

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
        assert_eq!(steps[2].status, "skipped");
    }

    #[cfg_attr(miri, ignore)]
    #[test]
    fn build_grpc_steps_uses_real_document_durations() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("chain.gctf");
        std::fs::write(&path, CHAIN_FIXTURE).unwrap();
        let path_str = path.to_string_lossy().into_owned();

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
        )
        .with_document_durations(vec![50, 30]);

        let steps = build_grpc_steps(&path_str, &result, 0, 300).unwrap();
        assert_eq!(steps.len(), 3);
        assert_eq!(
            (steps[0].start, steps[0].stop),
            (Some(0), Some(50)),
            "first step uses its real 50ms duration, not an even 100ms split"
        );
        assert_eq!(
            (steps[1].start, steps[1].stop),
            (Some(50), Some(80)),
            "second step starts where the first left off"
        );
        assert_eq!(steps[2].start, Some(80));
        assert_eq!(steps[2].stop, Some(300));
    }

    #[cfg_attr(miri, ignore)]
    #[test]
    fn build_grpc_steps_falls_back_to_even_split_without_real_durations() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("chain.gctf");
        std::fs::write(&path, CHAIN_FIXTURE).unwrap();
        let path_str = path.to_string_lossy().into_owned();

        let result = TestResult::pass(path_str.clone(), 300, Some(300));
        let steps = build_grpc_steps(&path_str, &result, 0, 300).unwrap();
        assert_eq!(steps.len(), 3);
        assert_eq!((steps[0].start, steps[0].stop), (Some(0), Some(100)));
        assert_eq!((steps[1].start, steps[1].stop), (Some(100), Some(200)));
        assert_eq!((steps[2].start, steps[2].stop), (Some(200), Some(300)));
    }

    fn assertion(line: usize, expr: &str, passed: bool) -> crate::state::AssertionRecord {
        crate::state::AssertionRecord {
            line,
            expression: expr.to_string(),
            passed,
            elapsed_ms: 1,
            message: if passed {
                None
            } else {
                Some("assertion failed".to_string())
            },
            endpoint: None,
            expected: if passed {
                None
            } else {
                Some("\"active\"".to_string())
            },
            actual: if passed {
                None
            } else {
                Some("\"pending\"".to_string())
            },
            hint: None,
        }
    }

    #[cfg_attr(miri, ignore)]
    #[test]
    fn allure_emits_assertion_steps_and_diff_trace() {
        let dir = tempfile::tempdir().unwrap();
        let gctf = dir.path().join("t.gctf");
        std::fs::write(&gctf, SINGLE_FIXTURE).unwrap();
        let gctf_str = gctf.to_string_lossy().into_owned();

        let out_dir = dir.path().join("allure-results");
        let reporter = AllureReporter::new(out_dir.clone());
        let result = TestResult::fail(
            gctf_str.clone(),
            "1 assertion failed".to_string(),
            5,
            Some(3),
        )
        .with_assertions(vec![
            assertion(7, ".status == \"active\"", false),
            assertion(8, ".id != null", true),
        ]);
        reporter.on_test_end(&gctf_str, &result);

        let result_file = std::fs::read_dir(&out_dir)
            .unwrap()
            .map(|e| e.unwrap().path())
            .find(|p| p.to_string_lossy().ends_with("-result.json"))
            .expect("result file");
        let json: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(result_file).unwrap()).unwrap();

        let steps = json["steps"].as_array().expect("steps array");
        let assertions_step = steps
            .iter()
            .find(|s| s["name"].as_str().unwrap_or("").starts_with("Assertions"))
            .expect("assertions step");
        assert_eq!(assertions_step["status"], "failed");
        let children = assertions_step["steps"].as_array().unwrap();
        assert_eq!(children.len(), 2);
        let failed = children.iter().find(|s| s["status"] == "failed").unwrap();
        let trace = failed["statusDetails"]["trace"].as_str().unwrap();
        assert!(
            trace.contains("\"active\""),
            "step trace has expected: {trace}"
        );
        assert!(
            trace.contains("\"pending\""),
            "step trace has actual: {trace}"
        );

        let top_trace = json["statusDetails"]["trace"].as_str().unwrap();
        assert!(
            top_trace.contains("line 7"),
            "top trace names the line: {top_trace}"
        );
        assert!(
            top_trace.contains("\"pending\""),
            "top trace has actual: {top_trace}"
        );
    }

    #[cfg_attr(miri, ignore)]
    #[test]
    fn allure_emits_suite_behavior_trees_and_test_case_id() {
        let dir = tempfile::tempdir().unwrap();
        let gctf = dir.path().join("t.gctf");
        std::fs::write(&gctf, SINGLE_FIXTURE).unwrap();
        let gctf_str = gctf.to_string_lossy().into_owned();

        let out_dir = dir.path().join("allure-results");
        let reporter = AllureReporter::new(out_dir.clone());
        reporter.on_test_end(&gctf_str, &TestResult::pass(gctf_str.clone(), 5, Some(3)));

        let result_file = std::fs::read_dir(&out_dir)
            .unwrap()
            .map(|e| e.unwrap().path())
            .find(|p| p.to_string_lossy().ends_with("-result.json"))
            .expect("result file");
        let json: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(result_file).unwrap()).unwrap();

        let labels = json["labels"].as_array().unwrap();
        let has = |name: &str, value: &str| {
            labels
                .iter()
                .any(|l| l["name"] == name && l["value"] == value)
        };
        assert!(has("parentSuite", "pkg"), "parentSuite=pkg: {labels:?}");
        assert!(has("suite", "Service"), "suite=Service: {labels:?}");
        assert!(has("subSuite", "M"), "subSuite=M: {labels:?}");
        assert!(has("epic", "pkg"), "epic=pkg: {labels:?}");
        assert!(has("feature", "Service"), "feature=Service: {labels:?}");
        assert!(has("story", "M"), "story=M: {labels:?}");

        let tcid = json["testCaseId"].as_str().expect("testCaseId");
        assert!(!tcid.is_empty());
    }

    #[cfg_attr(miri, ignore)]
    #[test]
    fn allure_emits_host_and_thread_labels_for_timeline() {
        let dir = tempfile::tempdir().unwrap();
        let gctf = dir.path().join("t.gctf");
        std::fs::write(&gctf, SINGLE_FIXTURE).unwrap();
        let gctf_str = gctf.to_string_lossy().into_owned();

        let out_dir = dir.path().join("allure-results");
        let reporter = AllureReporter::new(out_dir.clone());
        reporter.on_test_end(&gctf_str, &TestResult::pass(gctf_str.clone(), 5, Some(3)));

        let result_file = std::fs::read_dir(&out_dir)
            .unwrap()
            .map(|e| e.unwrap().path())
            .find(|p| p.to_string_lossy().ends_with("-result.json"))
            .expect("result file");
        let json: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(result_file).unwrap()).unwrap();
        let labels = json["labels"].as_array().unwrap();

        let host = labels
            .iter()
            .find(|l| l["name"] == "host")
            .expect("host label present");
        assert!(
            !host["value"].as_str().unwrap().is_empty(),
            "host label must not be empty: {labels:?}"
        );

        let thread = labels
            .iter()
            .find(|l| l["name"] == "thread")
            .expect("thread label present");
        assert!(
            !thread["value"].as_str().unwrap().is_empty(),
            "thread label must not be empty: {labels:?}"
        );
    }

    #[cfg_attr(miri, ignore)]
    #[test]
    fn allure_thread_label_reflects_real_concurrency() {
        let dir = tempfile::tempdir().unwrap();
        let gctf = dir.path().join("t.gctf");
        std::fs::write(&gctf, SINGLE_FIXTURE).unwrap();
        let gctf_str = gctf.to_string_lossy().into_owned();
        let out_dir = dir.path().join("allure-results");
        let reporter = std::sync::Arc::new(AllureReporter::new(out_dir.clone()));

        let handles: Vec<_> = (0..2)
            .map(|_| {
                let reporter = reporter.clone();
                let gctf_str = gctf_str.clone();
                std::thread::spawn(move || {
                    reporter
                        .on_test_end(&gctf_str, &TestResult::pass(gctf_str.clone(), 5, Some(3)));
                })
            })
            .collect();
        for h in handles {
            h.join().unwrap();
        }

        let thread_values: std::collections::HashSet<String> = std::fs::read_dir(&out_dir)
            .unwrap()
            .map(|e| e.unwrap().path())
            .filter(|p| p.to_string_lossy().ends_with("-result.json"))
            .map(|p| {
                let json: serde_json::Value =
                    serde_json::from_str(&std::fs::read_to_string(p).unwrap()).unwrap();
                json["labels"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .find(|l| l["name"] == "thread")
                    .unwrap()["value"]
                    .as_str()
                    .unwrap()
                    .to_string()
            })
            .collect();
        assert_eq!(
            thread_values.len(),
            2,
            "each OS thread must produce a distinct thread label: {thread_values:?}"
        );
    }

    #[cfg_attr(miri, ignore)]
    #[test]
    fn allure_test_case_id_differs_per_row_but_stable_for_reruns() {
        let dir = tempfile::tempdir().unwrap();
        let gctf = dir.path().join("t.gctf");
        std::fs::write(&gctf, SINGLE_FIXTURE).unwrap();
        let base = gctf.to_string_lossy().into_owned();

        let out_dir = dir.path().join("allure-results");
        let reporter = AllureReporter::new(out_dir.clone());
        reporter.on_test_end(
            &format!("{base}#[row=0 x=1]"),
            &TestResult::pass(base.clone(), 5, Some(3)),
        );
        reporter.on_test_end(
            &format!("{base}#[row=1 x=2]"),
            &TestResult::pass(base.clone(), 5, Some(3)),
        );

        let case_id = |test_name: &str, result: &TestResult| {
            let dir = tempfile::tempdir().unwrap();
            let out = dir.path().join("allure-results");
            AllureReporter::new(out.clone()).on_test_end(test_name, result);
            let entry = std::fs::read_dir(&out)
                .unwrap()
                .map(|e| e.unwrap().path())
                .find(|p| p.to_string_lossy().ends_with("-result.json"))
                .unwrap();
            let v: serde_json::Value =
                serde_json::from_str(&std::fs::read_to_string(entry).unwrap()).unwrap();
            v["testCaseId"].as_str().unwrap().to_string()
        };
        let row0_again = case_id(
            &format!("{base}#[row=0 x=1]"),
            &TestResult::pass(base.clone(), 5, Some(3)),
        );

        let ids: Vec<String> = std::fs::read_dir(&out_dir)
            .unwrap()
            .map(|e| e.unwrap().path())
            .filter(|p| p.to_string_lossy().ends_with("-result.json"))
            .map(|p| {
                let v: serde_json::Value =
                    serde_json::from_str(&std::fs::read_to_string(p).unwrap()).unwrap();
                v["testCaseId"].as_str().unwrap().to_string()
            })
            .collect();
        assert_eq!(ids.len(), 2);
        assert_ne!(
            ids[0], ids[1],
            "distinct rows must get distinct testCaseIds, or Allure 3 collapses them into one visible test"
        );
        assert!(
            ids.contains(&row0_again),
            "rerunning the same row must reproduce its exact testCaseId: {ids:?} vs {row0_again}"
        );
    }

    #[test]
    fn executor_json_from_github_actions_env() {
        let env = |k: &str| -> Option<String> {
            match k {
                "GITHUB_ACTIONS" => Some("true".to_string()),
                "GITHUB_SERVER_URL" => Some("https://github.com".to_string()),
                "GITHUB_REPOSITORY" => Some("gripmock/grpctestify".to_string()),
                "GITHUB_RUN_ID" => Some("42".to_string()),
                "GITHUB_RUN_NUMBER" => Some("7".to_string()),
                _ => None,
            }
        };
        let json: serde_json::Value =
            serde_json::from_str(&build_executor_json_from(env).unwrap()).unwrap();
        assert_eq!(json["type"], "github");
        assert_eq!(json["buildName"], "gripmock/grpctestify #7");
        assert_eq!(
            json["buildUrl"],
            "https://github.com/gripmock/grpctestify/actions/runs/42"
        );
    }

    #[test]
    fn executor_json_none_outside_ci() {
        assert!(build_executor_json_from(|_| None).is_none());
    }

    #[cfg_attr(miri, ignore)]
    #[test]
    fn allure_writes_categories_and_environment_sidecars() {
        let dir = tempfile::tempdir().unwrap();
        let out_dir = dir.path().join("allure-results");
        let reporter =
            AllureReporter::new(out_dir.clone()).with_address("localhost:4770".to_string());

        let mut results = crate::state::TestResults::new();
        results.metrics.parallel_jobs = 4;
        results.metrics.rpc_calls = 9;
        reporter.on_suite_end(&results).unwrap();

        let categories = std::fs::read_to_string(out_dir.join("categories.json")).unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&categories).unwrap();
        assert!(
            parsed
                .as_array()
                .unwrap()
                .iter()
                .any(|c| c["name"] == "Assertion mismatch"),
            "categories.json must define the Assertion mismatch bucket"
        );

        let env = std::fs::read_to_string(out_dir.join("environment.properties")).unwrap();
        assert!(
            env.contains("grpctestify.address=localhost:4770"),
            "env: {env}"
        );
        assert!(env.contains("grpctestify.parallel_jobs=4"), "env: {env}");
        assert!(env.contains(env!("CARGO_PKG_VERSION")), "env: {env}");
    }

    #[cfg_attr(miri, ignore)]
    #[test]
    fn allure_flags_retried_pass_as_flaky() {
        let dir = tempfile::tempdir().unwrap();
        let gctf = dir.path().join("t.gctf");
        std::fs::write(&gctf, SINGLE_FIXTURE).unwrap();
        let gctf_str = gctf.to_string_lossy().into_owned();

        let out_dir = dir.path().join("allure-results");
        let reporter = AllureReporter::new(out_dir.clone());
        let result = TestResult::pass(gctf_str.clone(), 5, Some(3)).with_retried(true);
        reporter.on_test_end(&gctf_str, &result);

        let entries: Vec<String> = std::fs::read_dir(&out_dir)
            .unwrap()
            .map(|e| e.unwrap().path())
            .filter(|p| p.to_string_lossy().ends_with("-result.json"))
            .map(|p| std::fs::read_to_string(p).unwrap())
            .collect();
        let json: serde_json::Value = serde_json::from_str(&entries[0]).unwrap();
        assert_eq!(json["status"], "passed");
        assert_eq!(
            json["statusDetails"]["flaky"], true,
            "retried pass must be flagged flaky: {json}"
        );
    }

    #[cfg_attr(miri, ignore)]
    #[test]
    fn allure_clean_pass_has_no_flaky_flag() {
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
            .map(|e| e.unwrap().path())
            .filter(|p| p.to_string_lossy().ends_with("-result.json"))
            .map(|p| std::fs::read_to_string(p).unwrap())
            .collect();
        let json: serde_json::Value = serde_json::from_str(&entries[0]).unwrap();
        assert!(
            json["statusDetails"].is_null(),
            "clean pass must carry no status details: {json}"
        );
    }

    #[cfg_attr(miri, ignore)]
    #[test]
    fn a_fixture_is_a_fixture_in_either_family() {
        assert!(matches!(
            fixture_kind("api/_setup.httf"),
            Some(FixtureKind::Setup)
        ));
        assert!(matches!(
            fixture_kind("api/_teardown.httf"),
            Some(FixtureKind::Teardown)
        ));
        assert!(fixture_kind("api/list.httf").is_none());
    }

    #[cfg_attr(miri, ignore)]
    #[test]
    fn allure_fixture_writes_container_not_result() {
        let dir = tempfile::tempdir().unwrap();
        let out_dir = dir.path().join("allure-results");
        let reporter = AllureReporter::new(out_dir.clone());

        let setup_path = "tests/users/_setup.gctf";
        reporter.on_test_end(setup_path, &TestResult::pass(setup_path, 5, None));
        reporter
            .on_suite_end(&crate::state::TestResults::new())
            .unwrap();

        let entries: Vec<String> = std::fs::read_dir(&out_dir)
            .unwrap()
            .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
            .collect();
        assert!(
            !entries.iter().any(|n| n.ends_with("-result.json")),
            "fixture must not get its own result file: {entries:?}"
        );
        let container_file = entries
            .iter()
            .find(|n| n.ends_with("-container.json"))
            .unwrap_or_else(|| panic!("no container file: {entries:?}"));
        let content = std::fs::read_to_string(out_dir.join(container_file)).unwrap();
        let json: serde_json::Value = serde_json::from_str(&content).unwrap();
        assert_eq!(json["befores"][0]["status"], "passed");
        assert_eq!(json["befores"][0]["name"], setup_path);
    }

    #[cfg_attr(miri, ignore)]
    #[test]
    fn allure_container_links_children_and_fixtures() {
        let dir = tempfile::tempdir().unwrap();
        let gctf = dir.path().join("t.gctf");
        std::fs::write(&gctf, SINGLE_FIXTURE).unwrap();
        let gctf_str = gctf.to_string_lossy().into_owned();
        let setup_path = gctf.parent().unwrap().join("_setup.gctf");
        let teardown_path = gctf.parent().unwrap().join("_teardown.gctf");

        let out_dir = dir.path().join("allure-results");
        let reporter = AllureReporter::new(out_dir.clone());
        reporter.on_test_end(
            &setup_path.to_string_lossy(),
            &TestResult::pass(setup_path.to_string_lossy().into_owned(), 1, None),
        );
        reporter.on_test_end(&gctf_str, &TestResult::pass(gctf_str.clone(), 5, Some(3)));
        reporter.on_test_end(
            &teardown_path.to_string_lossy(),
            &TestResult::pass(teardown_path.to_string_lossy().into_owned(), 1, None),
        );
        reporter
            .on_suite_end(&crate::state::TestResults::new())
            .unwrap();

        let entries: Vec<String> = std::fs::read_dir(&out_dir)
            .unwrap()
            .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
            .collect();
        let result_file = entries
            .iter()
            .find(|n| n.ends_with("-result.json"))
            .unwrap();
        let test_uuid = result_file.strip_suffix("-result.json").unwrap();

        let container_file = entries
            .iter()
            .find(|n| n.ends_with("-container.json"))
            .unwrap_or_else(|| panic!("no container file: {entries:?}"));
        let content = std::fs::read_to_string(out_dir.join(container_file)).unwrap();
        let json: serde_json::Value = serde_json::from_str(&content).unwrap();
        assert_eq!(json["children"][0], test_uuid, "container: {json}");
        assert_eq!(json["befores"].as_array().unwrap().len(), 1);
        assert_eq!(json["afters"].as_array().unwrap().len(), 1);
    }

    #[cfg_attr(miri, ignore)]
    #[test]
    fn allure_no_container_without_fixtures() {
        let dir = tempfile::tempdir().unwrap();
        let gctf = dir.path().join("t.gctf");
        std::fs::write(&gctf, SINGLE_FIXTURE).unwrap();
        let gctf_str = gctf.to_string_lossy().into_owned();

        let out_dir = dir.path().join("allure-results");
        let reporter = AllureReporter::new(out_dir.clone());
        reporter.on_test_end(&gctf_str, &TestResult::pass(gctf_str.clone(), 5, Some(3)));
        reporter
            .on_suite_end(&crate::state::TestResults::new())
            .unwrap();

        let entries: Vec<String> = std::fs::read_dir(&out_dir)
            .unwrap()
            .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
            .collect();
        assert!(
            !entries.iter().any(|n| n.ends_with("-container.json")),
            "no fixtures in this dir, so no container should be written: {entries:?}"
        );
    }

    #[cfg_attr(miri, ignore)]
    #[test]
    fn allure_result_written_atomically_as_valid_json() {
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

        let result_file = entries
            .iter()
            .find(|n| n.ends_with("-result.json"))
            .unwrap_or_else(|| panic!("no result file: {entries:?}"));
        assert!(
            !entries.iter().any(|n| n.ends_with(".tmp")),
            "temp file leaked: {entries:?}"
        );

        let content = std::fs::read_to_string(out_dir.join(result_file)).unwrap();
        let _: serde_json::Value =
            serde_json::from_str(&content).expect("allure result must be valid JSON");
    }
}
