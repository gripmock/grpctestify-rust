use axum::Json;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::response::sse::{Event, KeepAlive, Sse};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::collections::HashMap;
use std::convert::Infallible;
use std::future::Future;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use tokio::sync::broadcast;
use tokio_stream::{Stream, StreamExt};

use super::PlayState;
use super::api::{reject_traversal, reports_base, require_gctf, resolve_file};

const MAX_JOBS: usize = 40;
const MAX_EVENTS: usize = 20_000;
const CHANNEL_CAPACITY: usize = 256;

fn lock<T>(m: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum JobKind {
    Run,
    Bench,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum JobStatus {
    Running,
    Passed,
    Failed,
    Cancelled,
}

#[derive(Clone, Debug, Serialize)]
pub struct JobSummary {
    pub id: String,
    pub reports: Vec<String>,
    pub kind: JobKind,
    pub status: JobStatus,
    pub paths: Vec<String>,
    pub started_ms: u64,
    pub finished_ms: Option<u64>,
    pub total: usize,
    pub passed: usize,
    pub failed: usize,
    pub skipped: usize,
    pub duration_ms: u64,
}

#[derive(Debug, Serialize)]
pub struct JobDetail {
    #[serde(flatten)]
    pub summary: JobSummary,
    pub events: Vec<Value>,
}

struct JobState {
    status: JobStatus,
    reports: Vec<String>,
    finished_ms: Option<u64>,
    passed: usize,
    failed: usize,
    skipped: usize,
    duration_ms: u64,
    events: Vec<Value>,
    coverage: Option<Value>,
    results: Option<Box<apif_state::TestResults>>,
    tx: Option<broadcast::Sender<Value>>,
}

pub struct Job {
    id: String,
    kind: JobKind,
    total: usize,
    formats: Vec<super::reports::Format>,
    paths: Vec<String>,
    started_ms: u64,
    cancel: Arc<AtomicBool>,
    cancel_signal: tokio::sync::watch::Sender<bool>,
    state: Mutex<JobState>,
}

impl Job {
    pub fn coverage(&self) -> Option<Value> {
        lock(&self.state).coverage.clone()
    }

    fn emit(&self, event: Value) {
        let mut st = lock(&self.state);
        if st.events.len() < MAX_EVENTS {
            st.events.push(event.clone());
        }
        if let Some(tx) = &st.tx {
            let _ = tx.send(event);
        }
    }

    fn summary(&self) -> JobSummary {
        let st = lock(&self.state);
        JobSummary {
            id: self.id.clone(),
            reports: st.reports.clone(),
            kind: self.kind,
            status: st.status,
            paths: self.paths.clone(),
            started_ms: self.started_ms,
            finished_ms: st.finished_ms,
            total: self.total,
            passed: st.passed,
            failed: st.failed,
            skipped: st.skipped,
            duration_ms: st.duration_ms,
        }
    }

    fn subscribe(&self) -> (Vec<Value>, Option<broadcast::Receiver<Value>>) {
        let st = lock(&self.state);
        (st.events.clone(), st.tx.as_ref().map(|tx| tx.subscribe()))
    }
}

#[derive(Default)]
pub struct JobRegistry {
    jobs: Mutex<Vec<Arc<Job>>>,
}

impl JobRegistry {
    fn insert(&self, job: Arc<Job>) {
        let mut jobs = lock(&self.jobs);
        jobs.push(job);
        let mut overflow = jobs.len().saturating_sub(MAX_JOBS);
        if overflow > 0 {
            jobs.retain(|j| {
                if overflow == 0 || lock(&j.state).status == JobStatus::Running {
                    return true;
                }
                overflow -= 1;
                false
            });
        }
    }

    pub(crate) fn get(&self, id: &str) -> Option<Arc<Job>> {
        lock(&self.jobs).iter().find(|j| j.id == id).cloned()
    }

    fn list(&self) -> Vec<JobSummary> {
        lock(&self.jobs).iter().rev().map(|j| j.summary()).collect()
    }
}

#[derive(Deserialize)]
pub struct CreateJobRequest {
    #[serde(default)]
    pub data: Option<String>,
    #[serde(default = "default_kind")]
    pub kind: JobKind,
    #[serde(default)]
    pub reports: Vec<String>,
    pub paths: Vec<String>,
    pub up_to_step: Option<usize>,
}

fn default_kind() -> JobKind {
    JobKind::Run
}

pub(crate) fn dataset_rows(
    path: &std::path::Path,
) -> Option<Vec<std::collections::HashMap<String, Value>>> {
    use crate::parser::ast::{SectionContent, SectionType};
    let doc = crate::parser::parse_with_recovery(path).document;
    let section = doc.first_section(SectionType::Dataset)?;
    match &section.content {
        SectionContent::Rows(rows) => Some(
            rows.iter()
                .map(crate::commands::run::dataset_row_vars)
                .collect(),
        ),
        SectionContent::Empty => Some(Vec::new()),
        _ => None,
    }
}

pub(crate) type JobFile = (
    String,
    std::path::PathBuf,
    Vec<std::collections::HashMap<String, Value>>,
);

pub async fn create_job(
    State(state): State<Arc<PlayState>>,
    Json(req): Json<CreateJobRequest>,
) -> Result<Json<JobSummary>, (StatusCode, String)> {
    if req.paths.is_empty() {
        return Err((StatusCode::BAD_REQUEST, "No files to run".to_string()));
    }

    let mut files = Vec::with_capacity(req.paths.len());
    for rel in &req.paths {
        reject_traversal(rel)?;
        require_gctf(rel)?;
        if req.kind == JobKind::Bench
            && crate::parser::ast::Family::of(rel) != crate::parser::ast::Family::Gctf
        {
            continue;
        }
        let path = resolve_file(&state, rel)
            .ok_or((StatusCode::NOT_FOUND, format!("File not found: {rel}")))?;
        files.push((rel.clone(), path));
    }

    if files.is_empty() {
        return Err((
            StatusCode::BAD_REQUEST,
            "The load runner measures gRPC calls — there is no .gctf file in this selection"
                .to_string(),
        ));
    }

    let data = match req.data.as_deref().filter(|d| !d.trim().is_empty()) {
        Some(rel) => {
            reject_traversal(rel)?;
            let path = resolve_file(&state, rel).ok_or((
                StatusCode::NOT_FOUND,
                format!("Data source not found: {rel}"),
            ))?;
            let rows = tokio::task::spawn_blocking({
                let path = path.clone();
                move || crate::commands::run::collect_data_rows(&path, None)
            })
            .await
            .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?
            .map_err(|e| (StatusCode::BAD_REQUEST, format!("{rel}: {e}")))?;
            if rows.is_empty() {
                return Err((
                    StatusCode::BAD_REQUEST,
                    format!("{rel} produced no rows — there is nothing to run"),
                ));
            }
            Some(rows)
        }
        None => None,
    };

    let mut expanded: Vec<JobFile> = Vec::with_capacity(files.len());
    if req.kind == JobKind::Run {
        for (rel, path) in &files {
            let rows = tokio::task::spawn_blocking({
                let path = path.clone();
                move || dataset_rows(&path)
            })
            .await
            .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
            match rows {
                Some(rows) if rows.is_empty() => {
                    return Err((
                        StatusCode::BAD_REQUEST,
                        format!("{rel}: DATASET section has zero rows — there is nothing to run"),
                    ));
                }
                Some(_) if data.is_some() => {
                    return Err((
                        StatusCode::BAD_REQUEST,
                        format!(
                            "{rel} has a DATASET section, which is its own row source — a data source cannot be combined with it"
                        ),
                    ));
                }
                Some(rows) => expanded.push((rel.clone(), path.clone(), rows)),
                None => expanded.push((rel.clone(), path.clone(), Vec::new())),
            }
        }
    }

    let cases: usize = match &data {
        Some(rows) => files.len() * rows.len().max(1),
        None if req.kind == JobKind::Run => {
            expanded.iter().map(|(_, _, rows)| rows.len().max(1)).sum()
        }
        None => files.len(),
    };

    let (tx, _) = broadcast::channel(CHANNEL_CAPACITY);
    let formats: Vec<_> = req
        .reports
        .iter()
        .filter_map(|name| super::reports::Format::parse(name))
        .collect();

    let job = Arc::new(Job {
        id: uuid::Uuid::new_v4().to_string(),
        kind: req.kind,
        total: cases,
        formats,
        paths: files.iter().map(|(rel, _)| rel.clone()).collect(),
        started_ms: apif_cfg_runtime::now_unix_millis() as u64,
        cancel: Arc::new(AtomicBool::new(false)),
        cancel_signal: tokio::sync::watch::channel(false).0,
        state: Mutex::new(JobState {
            status: JobStatus::Running,
            reports: Vec::new(),
            finished_ms: None,
            passed: 0,
            failed: 0,
            skipped: 0,
            duration_ms: 0,
            events: Vec::new(),
            coverage: None,
            results: None,
            tx: Some(tx),
        }),
    });

    if req.kind == JobKind::Bench {
        if files.is_empty() {
            return Err((StatusCode::BAD_REQUEST, "A bench needs a file".to_string()));
        }
        let paths: Vec<std::path::PathBuf> = files.iter().map(|(_, p)| p.clone()).collect();
        if let Err(e) = super::bench_job::config_for_all(&paths) {
            let base = format!("{}/", super::api::primary_dir(&state).display());
            return Err((StatusCode::BAD_REQUEST, format!("{e:#}").replace(&base, "")));
        }
    }

    state.jobs.insert(job.clone());
    let summary = job.summary();
    match req.kind {
        JobKind::Run => {
            let env = state
                .project_root
                .as_deref()
                .and_then(|root| root.parent())
                .map(super::project::project_variables)
                .unwrap_or_default();
            tokio::spawn(run_job(
                job,
                expanded,
                req.up_to_step,
                reports_base(&state).to_path_buf(),
                data,
                env,
            ));
        }
        JobKind::Bench => {
            tokio::spawn(bench_job(job, files));
        }
    }
    Ok(Json(summary))
}

pub async fn list_jobs(State(state): State<Arc<PlayState>>) -> Json<Vec<JobSummary>> {
    Json(state.jobs.list())
}

pub async fn get_job(
    State(state): State<Arc<PlayState>>,
    Path(id): Path<String>,
) -> Result<Json<JobDetail>, (StatusCode, String)> {
    let job = state
        .jobs
        .get(&id)
        .ok_or((StatusCode::NOT_FOUND, "No such job".to_string()))?;
    let events = lock(&job.state).events.clone();
    Ok(Json(JobDetail {
        summary: job.summary(),
        events,
    }))
}

pub async fn cancel_job(
    State(state): State<Arc<PlayState>>,
    Path(id): Path<String>,
) -> Result<Json<JobSummary>, (StatusCode, String)> {
    let job = state
        .jobs
        .get(&id)
        .ok_or((StatusCode::NOT_FOUND, "No such job".to_string()))?;
    job.cancel.store(true, Ordering::Relaxed);
    let _ = job.cancel_signal.send(true);
    Ok(Json(job.summary()))
}

pub async fn job_report(
    State(state): State<Arc<PlayState>>,
    Path((id, name)): Path<(String, String)>,
) -> Result<axum::response::Response, (StatusCode, String)> {
    let job = state
        .jobs
        .get(&id)
        .ok_or((StatusCode::NOT_FOUND, "No such job".to_string()))?;

    let name =
        report_name(&job, &name).ok_or((StatusCode::NOT_FOUND, "No such report".to_string()))?;

    let format = super::reports::Format::parse(match name.rsplit('.').next() {
        Some("xml") => "junit",
        Some(ext) => ext,
        None => "json",
    })
    .unwrap_or(super::reports::Format::Json);

    let path = super::reports::dir_for(reports_base(&state), &id).join(&name);
    if !path.exists() {
        let results = lock(&job.state).results.clone();
        let Some(results) = results else {
            return Err((StatusCode::NOT_FOUND, "No such report".to_string()));
        };
        let written = super::reports::write(reports_base(&state), &id, &[format], &results);
        if written.is_empty() {
            return Err((
                StatusCode::INTERNAL_SERVER_ERROR,
                "the report could not be written".to_string(),
            ));
        }
        let mut st = lock(&job.state);
        if !st.reports.contains(&name) {
            st.reports.push(name.clone());
        }
    }
    if format.is_directory() {
        let results = std::fs::read_dir(&path)
            .map(|entries| {
                entries
                    .filter_map(Result::ok)
                    .filter(|e| e.file_name().to_string_lossy().ends_with("-result.json"))
                    .count()
            })
            .unwrap_or(0);
        let said = serde_json::json!({
            "path": path.to_string_lossy(),
            "files": results,
            "open": format!("allure serve {}", path.to_string_lossy()),
        });
        return axum::response::Response::builder()
            .header("content-type", "application/json")
            .body(axum::body::Body::from(said.to_string()))
            .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()));
    }

    let body = std::fs::read(&path)
        .map_err(|e| (StatusCode::NOT_FOUND, format!("Report unreadable: {e}")))?;

    axum::response::Response::builder()
        .header("content-type", format.content_type())
        .header(
            "content-disposition",
            format!("attachment; filename=\"{name}\""),
        )
        .body(axum::body::Body::from(body))
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))
}

fn report_name(job: &Job, asked: &str) -> Option<String> {
    let st = lock(&job.state);
    if st.reports.iter().any(|n| n == asked) {
        return Some(asked.to_string());
    }
    let format = super::reports::Format::parse(asked)
        .or_else(|| super::reports::Format::parse(asked.rsplit('.').next().unwrap_or(asked)))?;
    st.results.is_some().then(|| format.file_name().to_string())
}

pub async fn job_events(
    State(state): State<Arc<PlayState>>,
    Path(id): Path<String>,
) -> Result<Sse<impl Stream<Item = Result<Event, Infallible>>>, (StatusCode, String)> {
    let job = state
        .jobs
        .get(&id)
        .ok_or((StatusCode::NOT_FOUND, "No such job".to_string()))?;

    let (backlog, rx) = job.subscribe();
    let live = match rx {
        Some(rx) => tokio_stream::wrappers::BroadcastStream::new(rx),
        None => tokio_stream::wrappers::BroadcastStream::new(broadcast::channel(1).1),
    };

    let stream = tokio_stream::iter(backlog)
        .chain(live.filter_map(|e| e.ok()))
        .map(|event| Ok(Event::default().data(event.to_string())));

    Ok(Sse::new(stream).keep_alive(KeepAlive::default()))
}

async fn bench_job(job: Arc<Job>, files: Vec<(String, std::path::PathBuf)>) {
    if files.is_empty() {
        lock(&job.state).status = JobStatus::Failed;
        lock(&job.state).tx = None;
        return;
    }
    let rel = if files.len() == 1 {
        files[0].0.clone()
    } else {
        format!("{} files", files.len())
    };
    let paths: Vec<std::path::PathBuf> = files.iter().map(|(_, p)| p.clone()).collect();

    forget_target_schema().await;

    job.emit(json!({
        "event": "suite_start",
        "testCount": 1,
        "timestamp": apif_cfg_runtime::now_rfc3339(),
    }));
    job.emit(json!({
        "event": "test_start",
        "testId": rel,
        "timestamp": apif_cfg_runtime::now_rfc3339(),
    }));

    let started = std::time::Instant::now();
    let failure: Arc<Mutex<Option<String>>> = Arc::new(Mutex::new(None));
    let sink_job = Arc::clone(&job);
    let sink_failure = Arc::clone(&failure);

    super::bench_job::run(
        paths,
        Arc::clone(&job.cancel),
        Arc::new(move |event| match event {
            super::bench_job::BenchEvent::Progress(tick) => {
                sink_job.emit(super::bench_job::progress_event(&tick));
            }
            super::bench_job::BenchEvent::Report(report) => {
                sink_job.emit(super::bench_job::report_event(*report));
            }
            super::bench_job::BenchEvent::Failed(message) => {
                *lock(&sink_failure) = Some(message);
            }
        }),
    )
    .await;

    let duration = started.elapsed().as_millis() as u64;
    let failed = lock(&failure).clone();

    let cancelled = job.cancel.load(Ordering::Relaxed);
    let mut event = match (&failed, cancelled) {
        (Some(message), _) => json!({ "event": "test_fail", "testId": rel, "message": message }),
        (None, true) => json!({
            "event": "test_skip",
            "testId": rel,
            "interrupted": true,
            "message": "Cancelled — the measurement had already started",
        }),
        (None, false) => json!({ "event": "test_pass", "testId": rel }),
    };
    event["duration"] = json!(duration);
    event["timestamp"] = json!(apif_cfg_runtime::now_rfc3339());
    {
        let mut st = lock(&job.state);
        if failed.is_some() {
            st.failed += 1;
        } else if cancelled {
            st.skipped += 1;
        } else {
            st.passed += 1;
        }
    }
    job.emit(event);

    let (passed, failed_count) = {
        let st = lock(&job.state);
        (st.passed, st.failed)
    };
    let skipped = lock(&job.state).skipped;
    job.emit(json!({
        "event": "suite_end",
        "summary": {
            "total": 1,
            "passed": passed,
            "failed": failed_count,
            "skipped": skipped,
            "duration": duration,
        },
        "timestamp": apif_cfg_runtime::now_rfc3339(),
    }));

    let mut st = lock(&job.state);
    st.duration_ms = duration;
    st.finished_ms = Some(apif_cfg_runtime::now_unix_millis() as u64);
    st.status = if job.cancel.load(Ordering::Relaxed) {
        JobStatus::Cancelled
    } else if failed_count > 0 {
        JobStatus::Failed
    } else {
        JobStatus::Passed
    };
    st.tx = None;
}

type DataRows = Vec<std::collections::HashMap<String, Value>>;

type CallInFlight<'a> =
    std::pin::Pin<Box<dyn Future<Output = (Value, HashMap<String, Value>)> + Send + 'a>>;

async fn run_job(
    job: Arc<Job>,
    files: Vec<JobFile>,
    up_to_step: Option<usize>,
    root: std::path::PathBuf,
    data: Option<DataRows>,
    env: std::collections::HashMap<String, String>,
) {
    forget_target_schema().await;

    let rows = data.unwrap_or_default();

    let (files, fixtures) = partition_job_fixtures(files);
    let active_dirs: std::collections::BTreeSet<std::path::PathBuf> =
        files.iter().map(|(rel, _, _)| dir_of(rel)).collect();
    let fixture_count: usize = active_dirs
        .iter()
        .filter_map(|dir| fixtures.get(dir))
        .map(|fx| usize::from(fx.setup.is_some()) + usize::from(fx.teardown.is_some()))
        .sum();

    let count: usize = files
        .iter()
        .map(|(_, _, own)| rows.len().max(own.len()).max(1))
        .sum::<usize>()
        + fixture_count;
    let workers = std::thread::available_parallelism()
        .map(|n| n.get())
        .unwrap_or(4)
        .max(1);
    job.emit(json!({
        "event": "suite_start",
        "testCount": count,
        "workers": workers,
        "timestamp": apif_cfg_runtime::now_rfc3339(),
    }));

    let suite_start = std::time::Instant::now();
    let mut results = apif_state::TestResults::new();

    let env_address = super::project::address_of(&env);

    let base: std::collections::HashMap<String, Value> = env
        .into_iter()
        .map(|(k, v)| (k, Value::String(v)))
        .collect();

    let env_address = env_address.map(std::sync::Arc::new);
    let coverage = Arc::new(crate::report::CoverageCollector::new());

    let mut fixture_results = Vec::new();
    let mut dir_vars: std::collections::HashMap<std::path::PathBuf, Vars> = Default::default();
    let mut dir_setup_failed: std::collections::HashMap<std::path::PathBuf, String> =
        Default::default();
    for dir in &active_dirs {
        let Some((rel, path)) = fixtures.get(dir).and_then(|fx| fx.setup.clone()) else {
            continue;
        };
        let (event, bound, result) = run_fixture_file(
            &job,
            &rel,
            path,
            (!base.is_empty()).then(|| base.clone()),
            env_address.as_deref().map(|a| a.as_str()),
            coverage.clone(),
        )
        .await;
        if event["event"] == "test_pass" {
            dir_vars.insert(dir.clone(), bound);
        } else {
            dir_setup_failed.insert(dir.clone(), file_name_of(&rel));
        }
        fixture_results.push(result);
        job.emit(event);
    }

    let work: Vec<(String, std::path::PathBuf, Option<Vars>, Option<String>)> =
        work_list(files, &rows, &base)
            .into_iter()
            .map(|(rel, path, vars)| {
                let dir = dir_of(&rel);
                let seeded = match dir_vars.get(&dir) {
                    Some(seed) => {
                        let mut merged = seed.clone();
                        if let Some(vars) = vars {
                            merged.extend(vars);
                        }
                        Some(merged)
                    }
                    None => vars,
                };
                (rel, path, seeded, dir_setup_failed.get(&dir).cloned())
            })
            .collect();

    let collected = std::sync::Mutex::new(fixture_results);

    let tasks =
        futures::StreamExt::map(futures::stream::iter(work), |(rel, path, vars, blocked)| {
            let job = job.clone();
            let env_address = env_address.clone();
            let coverage = Some(coverage.clone());
            let collected = &collected;
            async move {
                if let Some(fixture) = blocked {
                    let event = json!({
                        "event": "test_skip",
                        "testId": rel,
                        "duration": 0,
                        "message": format!("Skipped: directory setup fixture ({fixture}) failed"),
                        "timestamp": apif_cfg_runtime::now_rfc3339(),
                    });
                    lock(&job.state).skipped += 1;
                    if let Ok(mut done) = collected.lock() {
                        done.push(result_for(&rel, &event, 0));
                    }
                    job.emit(event);
                    return;
                }
                if job.cancel.load(Ordering::Relaxed) {
                    job.emit(json!({
                        "event": "test_skip",
                        "testId": rel,
                        "duration": 0,
                        "message": "Cancelled before it ran",
                        "timestamp": apif_cfg_runtime::now_rfc3339(),
                    }));
                    let mut st = lock(&job.state);
                    st.skipped += 1;
                    return;
                }

                job.emit(json!({
                    "event": "test_start",
                    "testId": rel,
                    "timestamp": apif_cfg_runtime::now_rfc3339(),
                }));

                let started = std::time::Instant::now();
                let mut cancelled = job.cancel_signal.subscribe();
                let call: CallInFlight<'_> = Box::pin(run_one(
                    &rel,
                    path,
                    up_to_step,
                    vars,
                    env_address.as_deref().map(|a| a.as_str()),
                    coverage.clone(),
                ));
                let event = tokio::select! {
                    biased;
                    _ = cancelled.wait_for(|stop| *stop) => json!({
                        "event": "test_skip",
                        "testId": rel,
                        "interrupted": true,
                        "message": "Cancelled — the call had already gone out",
                    }),
                    e = call => e.0,
                };
                let duration = started.elapsed().as_millis() as u64;

                let mut event = event;
                event["duration"] = json!(duration);
                event["timestamp"] = json!(apif_cfg_runtime::now_rfc3339());

                {
                    let mut st = lock(&job.state);
                    match event["event"].as_str() {
                        Some("test_pass") => st.passed += 1,
                        Some("test_skip") => st.skipped += 1,
                        _ => st.failed += 1,
                    }
                }
                if let Ok(mut done) = collected.lock() {
                    done.push(result_for(&rel, &event, duration));
                }
                job.emit(event);
            }
        });
    futures::StreamExt::collect::<Vec<()>>(futures::StreamExt::buffer_unordered(tasks, workers))
        .await;

    let mut teardown_results = Vec::new();
    for dir in &active_dirs {
        let Some((rel, path)) = fixtures.get(dir).and_then(|fx| fx.teardown.clone()) else {
            continue;
        };
        let (event, _, result) = run_fixture_file(
            &job,
            &rel,
            path,
            (!base.is_empty()).then(|| base.clone()),
            env_address.as_deref().map(|a| a.as_str()),
            coverage.clone(),
        )
        .await;
        teardown_results.push(result);
        job.emit(event);
    }

    let mut collected = collected.into_inner().unwrap_or_default();
    collected.append(&mut teardown_results);
    for result in collected {
        results.add(result);
    }

    let total_ms = suite_start.elapsed().as_millis() as u64;
    let (passed, failed, skipped) = {
        let st = lock(&job.state);
        (st.passed, st.failed, st.skipped)
    };

    let coverage_report = coverage.generate_json_report();
    let covered = coverage_report.summary.covered;
    let methods = coverage_report.summary.total;
    if methods > 0
        && let Ok(whole) = serde_json::to_value(&coverage_report)
    {
        lock(&job.state).coverage = Some(whole);
    }
    let untested: Vec<String> = coverage_report
        .files
        .iter()
        .flat_map(|file| {
            file.methods
                .iter()
                .filter(|m| m.calls == 0)
                .map(move |m| format!("{}/{}", file.uri, m.name))
        })
        .collect();

    job.emit(json!({
        "event": "suite_end",
        "summary": {
            "total": passed + failed + skipped,
            "passed": passed,
            "failed": failed,
            "skipped": skipped,
            "duration": total_ms,
        },
        "coverage": (methods > 0).then(|| json!({
            "covered": covered,
            "methods": methods,
            "untested": untested,
        })),
        "timestamp": apif_cfg_runtime::now_rfc3339(),
    }));

    results.metrics.total_duration_ms = total_ms;
    results.metrics.sum_test_ms = results.all().iter().map(|r| r.duration_ms).sum();
    results.metrics.end_time = apif_cfg_runtime::now_timestamp();

    let written = super::reports::write(&root, &job.id, &job.formats, &results);

    let mut st = lock(&job.state);
    st.reports = written;
    st.results = Some(Box::new(results));
    st.duration_ms = total_ms;
    st.finished_ms = Some(apif_cfg_runtime::now_unix_millis() as u64);
    st.status = if job.cancel.load(Ordering::Relaxed) {
        JobStatus::Cancelled
    } else if failed > 0 {
        JobStatus::Failed
    } else {
        JobStatus::Passed
    };
    st.tx = None;
}

type Vars = std::collections::HashMap<String, Value>;

#[derive(Default, Debug)]
struct JobFixtures {
    setup: Option<(String, std::path::PathBuf)>,
    teardown: Option<(String, std::path::PathBuf)>,
}

fn dir_of(rel: &str) -> std::path::PathBuf {
    std::path::Path::new(rel)
        .parent()
        .map(std::path::Path::to_path_buf)
        .unwrap_or_default()
}

fn file_name_of(rel: &str) -> String {
    std::path::Path::new(rel)
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| rel.to_string())
}

fn partition_job_fixtures(
    files: Vec<JobFile>,
) -> (
    Vec<JobFile>,
    std::collections::HashMap<std::path::PathBuf, JobFixtures>,
) {
    use crate::commands::run::FixtureRole;
    let mut tests = Vec::with_capacity(files.len());
    let mut fixtures: std::collections::HashMap<std::path::PathBuf, JobFixtures> =
        Default::default();
    for (rel, path, own) in files {
        let dir = dir_of(&rel);
        match crate::commands::run::fixture_role(std::path::Path::new(&rel)) {
            Some(FixtureRole::Setup) => {
                fixtures.entry(dir).or_default().setup = Some((rel, path));
            }
            Some(FixtureRole::Teardown) => {
                fixtures.entry(dir).or_default().teardown = Some((rel, path));
            }
            None => tests.push((rel, path, own)),
        }
    }
    (tests, fixtures)
}

async fn run_fixture_file(
    job: &Arc<Job>,
    rel: &str,
    path: std::path::PathBuf,
    vars: Option<Vars>,
    env_address: Option<&str>,
    coverage: Arc<crate::report::CoverageCollector>,
) -> (Value, Vars, apif_state::TestResult) {
    job.emit(json!({
        "event": "test_start",
        "testId": rel,
        "timestamp": apif_cfg_runtime::now_rfc3339(),
    }));
    let started = std::time::Instant::now();
    let (mut event, bound) = run_one(rel, path, None, vars, env_address, Some(coverage)).await;
    let duration = started.elapsed().as_millis() as u64;
    event["duration"] = json!(duration);
    event["timestamp"] = json!(apif_cfg_runtime::now_rfc3339());
    {
        let mut st = lock(&job.state);
        match event["event"].as_str() {
            Some("test_pass") => st.passed += 1,
            Some("test_skip") => st.skipped += 1,
            _ => st.failed += 1,
        }
    }
    let result = result_for(rel, &event, duration);
    (event, bound, result)
}

fn work_list(
    files: Vec<(String, std::path::PathBuf, Vec<Vars>)>,
    rows: &[Vars],
    base: &Vars,
) -> Vec<(String, std::path::PathBuf, Option<Vars>)> {
    files
        .into_iter()
        .flat_map(|(rel, path, own)| {
            let source = if rows.is_empty() { own } else { rows.to_vec() };
            if source.is_empty() {
                return vec![(rel, path, (!base.is_empty()).then(|| base.clone()))];
            }
            source
                .iter()
                .enumerate()
                .map(|(i, vars)| {
                    let mut merged = base.clone();
                    merged.extend(vars.clone());
                    (
                        crate::commands::run::format_row_name(&rel, i, vars),
                        path.clone(),
                        Some(merged),
                    )
                })
                .collect()
        })
        .collect()
}

pub(crate) async fn run_with_retries(
    runner: &crate::execution::runner::TestRunner,
    document: &crate::parser::GctfDocument,
    vars: Option<std::collections::HashMap<String, Value>>,
    max_retries: u32,
    retry_delay: f64,
) -> anyhow::Result<crate::execution::runner::TestExecutionResult> {
    run_with_retries_capturing(runner, document, vars, max_retries, retry_delay)
        .await
        .map(|(result, _)| result)
}

pub(crate) async fn run_with_retries_capturing(
    runner: &crate::execution::runner::TestRunner,
    document: &crate::parser::GctfDocument,
    vars: Option<std::collections::HashMap<String, Value>>,
    max_retries: u32,
    retry_delay: f64,
) -> anyhow::Result<(crate::execution::runner::TestExecutionResult, Vars)> {
    let mut attempt = 0u32;
    loop {
        let outcome = runner
            .run_chain(document, vars.clone().unwrap_or_default())
            .await;
        let retryable = outcome
            .as_ref()
            .is_ok_and(|(result, _)| crate::commands::run::should_retry_result(result));
        if !retryable || attempt >= max_retries {
            return outcome;
        }
        attempt += 1;
        if retry_delay > 0.0 {
            tokio::time::sleep(std::time::Duration::from_secs_f64(retry_delay)).await;
        }
    }
}

pub(crate) fn retry_plan(document: &crate::parser::GctfDocument) -> (u32, f64) {
    let Ok(runtime) = crate::execution::runner_helpers::resolve_effective_runtime_options(
        document,
        crate::execution::runner_helpers::CliRuntimeDefaults {
            timeout_seconds: 30,
            retry: 0,
            retry_delay_seconds: 0.0,
            no_retry: false,
        },
    ) else {
        return (0, 0.0);
    };
    let attempts = if runtime.no_retry.value {
        0
    } else {
        runtime.retry.value
    };
    (attempts, runtime.retry_delay_seconds.value)
}

fn truncate_chain(document: &mut crate::parser::GctfDocument, steps: usize) {
    if steps == 0 {
        return;
    }
    let mut current = document;
    for _ in 1..steps {
        match current.next_document.as_deref_mut() {
            Some(next) => current = next,
            None => return,
        }
    }
    current.next_document = None;
}

pub(super) async fn forget_target_schema() {
    apif_grpc_transport::tonic::descriptor::clear_descriptor_cache().await;
    crate::grpc::web_reflection::clear_mode_cache().await;
}

fn assertions_of(event: &Value) -> Vec<apif_state::AssertionRecord> {
    event["assertions"]
        .as_array()
        .map(|list| {
            list.iter()
                .map(|a| apif_state::AssertionRecord {
                    line: a["line"].as_u64().unwrap_or(0) as usize,
                    expression: a["expression"].as_str().unwrap_or_default().to_string(),
                    passed: a["passed"].as_bool().unwrap_or(false),
                    elapsed_ms: 0,
                    message: a["message"].as_str().map(ToString::to_string),
                    endpoint: a["endpoint"].as_str().map(ToString::to_string),
                    expected: a["expected"].as_str().map(ToString::to_string),
                    actual: a["actual"].as_str().map(ToString::to_string),
                    hint: a["hint"].as_str().map(ToString::to_string),
                })
                .collect()
        })
        .unwrap_or_default()
}

fn documents_of(event: &Value) -> Vec<u64> {
    event["documents"]
        .as_array()
        .map(|list| list.iter().filter_map(Value::as_u64).collect())
        .unwrap_or_default()
}

fn config_of(event: &Value) -> apif_state::ConfigSummary {
    serde_json::from_value(event["config"].clone()).unwrap_or_default()
}

fn exchange_of(event: &Value) -> Option<apif_state::CapturedExchange> {
    let response = event.get("response")?;
    let messages: Vec<Value> = response["messages"].as_array().cloned().unwrap_or_default();
    let pairs = |key: &str| {
        response[key]
            .as_object()
            .map(|map| {
                map.iter()
                    .map(|(k, v)| (k.clone(), v.as_str().unwrap_or_default().to_string()))
                    .collect::<std::collections::HashMap<_, _>>()
            })
            .unwrap_or_default()
    };
    Some(apif_state::CapturedExchange::capture(
        pairs("headers"),
        pairs("trailers"),
        messages,
    ))
}

fn meta_of(event: &Value) -> apif_state::TestMeta {
    serde_json::from_value(event["meta"].clone()).unwrap_or_default()
}

fn result_for(rel: &str, event: &Value, duration_ms: u64) -> apif_state::TestResult {
    let call_ms = event["grpcDuration"].as_u64();
    match event["event"].as_str() {
        Some("test_pass") => {
            let mut result = apif_state::TestResult::pass(rel, duration_ms, call_ms);
            result.assertions = assertions_of(event);
            result.document_durations_ms = documents_of(event);
            result.config_summary = config_of(event);
            result.meta = meta_of(event);
            result
        }
        Some("test_skip") => {
            let mut result = apif_state::TestResult::pass(rel, duration_ms, call_ms);
            result.status = apif_state::TestStatus::Skip;
            result.error_message = event["message"].as_str().map(ToString::to_string);
            result
        }
        _ => {
            let mut result = apif_state::TestResult::fail(
                rel,
                event["message"].as_str().unwrap_or("failed").to_string(),
                duration_ms,
                call_ms,
            );
            result.assertions = assertions_of(event);
            result.document_durations_ms = documents_of(event);
            result.config_summary = config_of(event);
            result.meta = meta_of(event);
            result.exchange = exchange_of(event);
            result
        }
    }
}

const MAX_CAPTURED_BYTES: usize = 64 * 1024;

pub(crate) fn bounded_response(captured: &crate::grpc::GrpcResponse) -> Option<Value> {
    let response = json!({
        "messages": captured.messages,
        "headers": captured.headers,
        "trailers": captured.trailers,
        "error": captured.error,
    });
    let size = serde_json::to_string(&response)
        .map(|s| s.len())
        .unwrap_or(usize::MAX);
    if size > MAX_CAPTURED_BYTES {
        return None;
    }
    Some(response)
}

async fn run_one(
    rel: &str,
    path: std::path::PathBuf,
    up_to_step: Option<usize>,
    vars: Option<std::collections::HashMap<String, Value>>,
    env_address: Option<&str>,
    coverage: Option<Arc<crate::report::CoverageCollector>>,
) -> (Value, Vars) {
    let parsed =
        tokio::task::spawn_blocking(move || crate::parser::parse_with_recovery(&path)).await;

    let recovered = match parsed {
        Ok(p) => p,
        Err(e) => {
            return (
                json!({ "event": "test_fail", "testId": rel, "message": e.to_string() }),
                Vars::new(),
            );
        }
    };

    if let Some(fault) = super::api::first_parse_error(&recovered.diagnostics) {
        return (
            json!({ "event": "test_fail", "testId": rel, "message": format!("Parse error: {fault}") }),
            Vars::new(),
        );
    }

    let mut document = recovered.document;

    if let Some(steps) = up_to_step {
        truncate_chain(&mut document, steps);
    }

    if let Err(e) = crate::parser::validate_document_chain(&document) {
        return (
            json!({
                "event": "test_fail",
                "testId": rel,
                "message": format!("Validation error: {e}"),
            }),
            Vars::new(),
        );
    }

    let (max_retries, retry_delay) = retry_plan(&document);

    let runner =
        crate::execution::runner::TestRunner::new(false, 30, false, false, false, coverage)
            .with_capture_exchange(true);
    let runner = match env_address {
        Some(address) => runner.with_env_address(address.to_string()),
        None => runner,
    };
    let exec =
        run_with_retries_capturing(&runner, &document, vars.clone(), max_retries, retry_delay)
            .await;

    match exec {
        Ok((result, bound)) => {
            let assertions: Vec<Value> = result
                .assertions
                .iter()
                .map(|a| {
                    let mut j = json!({
                        "line": a.line,
                        "expression": a.expression,
                        "passed": a.passed,
                    });
                    if let Some(expected) = &a.expected {
                        j["expected"] = json!(expected);
                    }
                    if let Some(actual) = &a.actual {
                        j["actual"] = json!(actual);
                    }
                    if let Some(message) = &a.message {
                        j["message"] = json!(message);
                    }
                    if let Some(hint) = &a.hint {
                        j["hint"] = json!(hint);
                    }
                    if let Some(endpoint) = &a.endpoint {
                        j["endpoint"] = json!(endpoint);
                    }
                    if a.elapsed_ms > 0 {
                        j["elapsedMs"] = json!(a.elapsed_ms);
                    }
                    j
                })
                .collect();

            let mut event = match &result.status {
                crate::execution::runner::TestExecutionStatus::Pass => {
                    json!({ "event": "test_pass", "testId": rel })
                }
                crate::execution::runner::TestExecutionStatus::Fail(message) => {
                    json!({ "event": "test_fail", "testId": rel, "message": message })
                }
            };
            if let Some(ms) = result.call_duration_ms {
                event["grpcDuration"] = json!(ms);
            }
            if let Some(code) = result.grpc_status {
                event["grpcStatus"] = json!(code);
            }
            if let Some(address) = &result.dialled_address {
                event["address"] = json!(address);
            }
            if !assertions.is_empty() {
                event["assertions"] = json!(assertions);
            }
            if !result.document_durations_ms.is_empty() {
                event["documents"] = json!(result.document_durations_ms);
            }
            if !result.extracted.is_empty() {
                event["extracted"] = json!(result.extracted);
            }
            let shape = apif_state::ConfigSummary::from_document(&document);
            if !shape.is_empty() {
                event["config"] = json!(shape);
            }
            if result.meta != apif_state::TestMeta::default() {
                event["meta"] = json!(result.meta);
            }
            if event["event"] == "test_fail"
                && let Some(captured) = &result.captured_response
                && let Some(response) = bounded_response(captured)
            {
                event["response"] = response;
            }
            if result.document_durations_ms.len() > 1 {
                event["responseStep"] = json!(result.document_durations_ms.len() - 1);
            }
            (event, bound)
        }
        Err(e) => (
            json!({ "event": "test_fail", "testId": rel, "message": e.to_string() }),
            Vars::new(),
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_job_reads_the_fixture_convention() {
        let file = |rel: &str| {
            (
                rel.to_string(),
                std::path::PathBuf::from(rel),
                Vec::<Vars>::new(),
            )
        };
        let (tests, fixtures) = partition_job_fixtures(vec![
            file("fx/_setup.httf"),
            file("fx/a.httf"),
            file("fx/_teardown.gctf"),
            file("other/b.gctf"),
        ]);

        assert_eq!(
            tests
                .iter()
                .map(|(rel, _, _)| rel.as_str())
                .collect::<Vec<_>>(),
            vec!["fx/a.httf", "other/b.gctf"]
        );
        let fx = &fixtures[&std::path::PathBuf::from("fx")];
        assert_eq!(
            fx.setup.as_ref().map(|(rel, _)| rel.as_str()),
            Some("fx/_setup.httf")
        );
        assert_eq!(
            fx.teardown.as_ref().map(|(rel, _)| rel.as_str()),
            Some("fx/_teardown.gctf")
        );
        assert!(!fixtures.contains_key(&std::path::PathBuf::from("other")));
    }

    #[test]
    fn a_fixture_is_named_by_its_own_directory() {
        assert_eq!(dir_of("a/b/c.gctf"), std::path::PathBuf::from("a/b"));
        assert_eq!(dir_of("top.gctf"), std::path::PathBuf::from(""));
        assert_eq!(file_name_of("a/b/_setup.httf"), "_setup.httf");
    }

    #[tokio::test]
    #[cfg_attr(miri, ignore)]
    async fn a_setup_seeds_the_directory_it_sits_in() {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let asked = std::sync::Arc::new(std::sync::Mutex::new(Vec::<String>::new()));
        let seen = asked.clone();
        tokio::spawn(async move {
            loop {
                let Ok((mut socket, _)) = listener.accept().await else {
                    return;
                };
                let mut buf = vec![0u8; 2048];
                let read = tokio::io::AsyncReadExt::read(&mut socket, &mut buf)
                    .await
                    .unwrap_or(0);
                let request = String::from_utf8_lossy(&buf[..read]).to_string();
                if let Some(line) = request.lines().next() {
                    seen.lock().unwrap().push(line.to_string());
                }
                let _ = tokio::io::AsyncWriteExt::write_all(
                    &mut socket,
                    b"HTTP/1.1 200 OK\r\ncontent-length: 13\r\n\r\n{\"ok\":true}  ",
                )
                .await;
            }
        });

        let root = std::env::temp_dir().join(format!("gctf-fixture-job-{}", std::process::id()));
        let dir = root.join("fx");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(
            dir.join("_setup.httf"),
            format!(
                "--- ADDRESS ---\nhttp://{addr}\n\n--- ENDPOINT ---\nGET /seed\n\n--- ASSERTS ---\n@status() == 200\n\n--- EXTRACT ---\nwhere = \"seeded\"\n"
            ),
        )
        .unwrap();
        std::fs::write(
            dir.join("a.httf"),
            format!(
                "--- ADDRESS ---\nhttp://{addr}\n\n--- ENDPOINT ---\nGET /{{{{where}}}}\n\n--- ASSERTS ---\n@status() == 200\n"
            ),
        )
        .unwrap();
        let files = vec![
            (
                "fx/_setup.httf".to_string(),
                dir.join("_setup.httf"),
                Vec::new(),
            ),
            ("fx/a.httf".to_string(), dir.join("a.httf"), Vec::new()),
        ];

        let job = job_with("seeded");
        run_job(
            Arc::clone(&job),
            files.clone(),
            None,
            root.clone(),
            None,
            Default::default(),
        )
        .await;

        {
            let st = lock(&job.state);
            assert_eq!(
                (st.passed, st.failed, st.skipped),
                (2, 0, 0),
                "{:?}",
                st.events
            );
        }
        let lines = asked.lock().unwrap().clone();
        assert!(
            lines.iter().any(|l| l.starts_with("GET /seeded ")),
            "the test dialled what the setup bound: {lines:?}",
        );

        std::fs::write(
            dir.join("_setup.httf"),
            format!(
                "--- ADDRESS ---\nhttp://{addr}\n\n--- ENDPOINT ---\nGET /seed\n\n--- ASSERTS ---\n@status() == 599\n"
            ),
        )
        .unwrap();
        let job = job_with("blocked");
        run_job(
            Arc::clone(&job),
            files,
            None,
            root,
            None,
            Default::default(),
        )
        .await;

        let st = lock(&job.state);
        assert_eq!((st.passed, st.failed, st.skipped), (0, 1, 1));
        let skipped = st
            .events
            .iter()
            .find(|e| e["event"] == "test_skip")
            .expect("the test its setup blocked is reported");
        assert_eq!(skipped["testId"], serde_json::json!("fx/a.httf"));
        assert!(
            skipped["message"]
                .as_str()
                .unwrap_or_default()
                .contains("(_setup.httf)"),
            "{skipped}",
        );
    }

    #[tokio::test]
    #[cfg_attr(miri, ignore)]
    async fn a_file_that_asks_for_a_retry_gets_one() {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let seen = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let counter = seen.clone();
        tokio::spawn(async move {
            loop {
                let Ok((mut socket, _)) = listener.accept().await else {
                    return;
                };
                let attempt = counter.fetch_add(1, Ordering::Relaxed);
                if attempt == 0 {
                    drop(socket);
                    continue;
                }
                let mut buf = vec![0u8; 2048];
                let _ = tokio::io::AsyncReadExt::read(&mut socket, &mut buf).await;
                let _ = tokio::io::AsyncWriteExt::write_all(
                    &mut socket,
                    b"HTTP/1.1 200 OK\r\ncontent-length: 14\r\n\r\n{\"name\":\"Ada\"}",
                )
                .await;
            }
        });

        let src = format!(
            "--- ADDRESS ---\nhttp://{addr}\n\n--- ENDPOINT ---\nGET /x\n\n--- OPTIONS ---\nretry: 2\n\n--- ASSERTS ---\n.name == \"Ada\"\n"
        );
        let document = crate::parser::parse_content_with_recovery(&src, "flaky.httf").document;
        let (max_retries, retry_delay) = retry_plan(&document);
        assert_eq!((max_retries, retry_delay), (2, 0.0));

        let runner = crate::execution::runner::TestRunner::new(false, 5, false, false, false, None);
        let result = run_with_retries(&runner, &document, None, max_retries, retry_delay)
            .await
            .expect("the run itself must not error");

        assert!(
            matches!(
                result.status,
                crate::execution::runner::TestExecutionStatus::Pass
            ),
            "a file asking for two retries must survive one dropped connection: {:?}",
            result.status
        );
        assert!(seen.load(Ordering::Relaxed) >= 2, "the call was made once");
    }

    #[test]
    fn a_result_carries_the_checks_its_event_carried() {
        let event = json!({
            "event": "test_fail",
            "testId": "a.gctf",
            "message": "Validation failed",
            "assertions": [
                { "line": 14, "expression": "--- RESPONSE ---", "passed": false,
                  "expected": "Hello, World!", "actual": "Hello World" },
                { "line": 20, "expression": ".ok == true", "passed": true },
            ],
        });
        let result = result_for("a.gctf", &event, 41);
        assert_eq!(result.assertions.len(), 2);
        let failed = result
            .assertions
            .iter()
            .find(|a| !a.passed)
            .expect("the check that did not hold");
        assert_eq!(failed.line, 14);
        assert_eq!(failed.actual.as_deref(), Some("Hello World"));
    }

    #[test]
    fn a_result_carries_what_each_step_of_a_chain_spent() {
        let event = json!({
            "event": "test_pass",
            "testId": "checkout.apif",
            "documents": [31, 12],
        });
        assert_eq!(
            result_for("checkout.apif", &event, 43).document_durations_ms,
            vec![31, 12]
        );
    }

    #[test]
    fn a_stopped_chain_keeps_the_steps_that_ran() {
        let event = json!({
            "event": "test_fail",
            "testId": "checkout.apif",
            "message": "step 2 failed",
            "documents": [31, 12],
        });
        let result = result_for("checkout.apif", &event, 43);
        assert_eq!(result.document_durations_ms, vec![31, 12]);
        assert!(matches!(result.status, apif_state::TestStatus::Fail));
    }

    #[test]
    fn a_single_document_file_reports_no_steps() {
        let event = json!({ "event": "test_pass", "testId": "a.gctf" });
        assert!(
            result_for("a.gctf", &event, 3)
                .document_durations_ms
                .is_empty()
        );
    }

    #[test]
    fn a_result_carries_the_shape_of_the_file() {
        let event = json!({
            "event": "test_pass",
            "testId": "checkout.apif",
            "config": { "sections": ["ADDRESS", "ENDPOINT"], "chain_steps": 2, "tls": true },
        });
        let summary = result_for("checkout.apif", &event, 9).config_summary;
        assert_eq!(
            summary.sections,
            vec!["ADDRESS".to_string(), "ENDPOINT".to_string()]
        );
        assert_eq!(summary.chain_steps, 2);
        assert!(summary.tls);
    }

    #[tokio::test]
    #[cfg_attr(miri, ignore)]
    async fn a_file_the_command_line_refuses_is_refused_here() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("tagged.gctf");
        std::fs::write(
            &path,
            "--- META ---\ntags: smoke, auth\n\n--- ENDPOINT ---\na.B/C\n\n--- REQUEST ---\n{}\n\n--- ASSERTS ---\n.ok\n",
        )
        .expect("write");

        let (event, _) = run_one("tagged.gctf", path, None, None, None, None).await;

        assert_eq!(event["event"], "test_fail");
        let said = event["message"].as_str().unwrap_or_default();
        assert!(said.starts_with("Parse error:"), "{said}");
        assert!(
            said.contains("tags"),
            "it names what it could not read: {said}"
        );
    }

    #[tokio::test]
    #[cfg_attr(miri, ignore)]
    async fn a_file_that_parses_is_not_refused() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("ok.gctf");
        std::fs::write(
            &path,
            "--- ADDRESS ---\n127.0.0.1:1\n\n--- ENDPOINT ---\na.B/C\n\n--- REQUEST ---\n{}\n\n--- ASSERTS ---\n.ok\n",
        )
        .expect("write");

        let (event, _) = run_one("ok.gctf", path, None, None, None, None).await;

        let said = event["message"].as_str().unwrap_or_default();
        assert!(!said.starts_with("Parse error:"), "{said}");
    }

    #[test]
    fn a_result_carries_what_the_file_says_about_itself() {
        let event = json!({
            "event": "test_pass",
            "testId": "auth/login.gctf",
            "meta": { "name": "login flow", "tags": ["smoke", "auth"], "owner": "ada" },
        });
        let meta = result_for("auth/login.gctf", &event, 4).meta;
        assert_eq!(meta.name.as_deref(), Some("login flow"));
        assert_eq!(meta.tags, vec!["smoke".to_string(), "auth".to_string()]);
        assert_eq!(meta.owner.as_deref(), Some("ada"));
    }

    #[test]
    fn a_file_without_meta_says_nothing() {
        let event = json!({ "event": "test_pass", "testId": "a.gctf" });
        assert_eq!(
            result_for("a.gctf", &event, 1).meta,
            apif_state::TestMeta::default()
        );
    }

    #[test]
    fn a_failure_carries_what_came_back() {
        let event = json!({
            "event": "test_fail",
            "testId": "a.gctf",
            "message": "assertion failed",
            "response": {
                "messages": [{ "message": "hi" }],
                "headers": { "content-type": "application/grpc" },
                "trailers": { "grpc-status": "0" },
                "error": null,
            },
        });
        let exchange = result_for("a.gctf", &event, 5)
            .exchange
            .expect("what came back");
        assert_eq!(exchange.response.len(), 1);
        assert_eq!(
            exchange.headers.get("content-type").map(String::as_str),
            Some("application/grpc")
        );
        assert_eq!(
            exchange.trailers.get("grpc-status").map(String::as_str),
            Some("0")
        );
    }

    #[test]
    fn a_pass_keeps_no_exchange() {
        let event = json!({ "event": "test_pass", "testId": "a.gctf" });
        assert!(result_for("a.gctf", &event, 1).exchange.is_none());
    }

    #[test]
    fn a_result_without_checks_carries_none() {
        let event =
            json!({ "event": "test_fail", "testId": "a.gctf", "message": "Connection refused" });
        assert!(result_for("a.gctf", &event, 3).assertions.is_empty());
    }

    fn state_in(dir: &std::path::Path) -> Arc<PlayState> {
        Arc::new(PlayState {
            collections_dir: dir.to_path_buf(),
            collections_dirs: vec![dir.to_path_buf()],
            shares_dir: dir.join("shares"),
            project_root: None,
            project_settings: None,
            history_lock: tokio::sync::Mutex::new(()),
            write_lock: tokio::sync::Mutex::new(()),
            collections_mtime: Arc::new(std::sync::atomic::AtomicU64::new(0)),
            jobs: Default::default(),
        })
    }

    fn job_with(id: &str) -> Arc<Job> {
        Arc::new(Job {
            id: id.to_string(),
            kind: JobKind::Run,
            total: 0,
            formats: vec![],
            paths: vec![],
            started_ms: 0,
            cancel: Arc::new(AtomicBool::new(false)),
            cancel_signal: tokio::sync::watch::channel(false).0,
            state: Mutex::new(JobState {
                status: JobStatus::Running,
                reports: Vec::new(),
                finished_ms: None,
                passed: 0,
                failed: 0,
                skipped: 0,
                duration_ms: 0,
                events: Vec::new(),
                coverage: None,
                results: None,
                tx: None,
            }),
        })
    }

    #[cfg_attr(miri, ignore)]
    #[tokio::test]
    async fn a_bench_stopped_by_hand_is_not_reported_as_passed() {
        let dir = std::env::temp_dir().join(format!("gctf-bench-status-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let file = dir.join("load.gctf");
        std::fs::write(
            &file,
            "--- ADDRESS ---\n127.0.0.1:1\n\n--- ENDPOINT ---\na.A/One\n\n--- REQUEST ---\n{}\n\n--- ASSERTS ---\n.ok == true\n\n--- BENCH ---\nmode: fixed\nconcurrency: 2\nduration: 60s\n",
        )
        .unwrap();

        let job = job_with("stopped");
        job.cancel.store(true, Ordering::Relaxed);
        bench_job(Arc::clone(&job), vec![("load.gctf".to_string(), file)]).await;

        let st = lock(&job.state);
        assert_eq!(
            st.status,
            JobStatus::Cancelled,
            "a bench that measured less than it was asked to must not report a pass",
        );
        assert_eq!(
            (st.passed, st.skipped),
            (0, 1),
            "and the file it stopped on did not pass"
        );

        let stopped = st
            .events
            .iter()
            .find(|e| e["event"] == "test_skip")
            .expect("the file it stopped on is reported");
        assert_eq!(stopped["interrupted"], serde_json::json!(true));
        assert!(
            stopped["message"]
                .as_str()
                .unwrap_or_default()
                .contains("already started"),
            "{stopped}",
        );
    }

    #[test]
    fn eviction_keeps_the_jobs_that_are_still_running() {
        let registry = JobRegistry::default();
        let live = job_with("live");
        registry.insert(Arc::clone(&live));
        for i in 0..MAX_JOBS {
            let done = job_with(&format!("done-{i}"));
            lock(&done.state).status = JobStatus::Passed;
            registry.insert(done);
        }

        assert!(
            registry.get("live").is_some(),
            "a running job must survive the cap — evicting it leaves it running with no stream, no cancel and no report",
        );
        assert!(
            registry.get("done-0").is_none(),
            "the oldest finished job goes instead"
        );
        assert_eq!(registry.list().len(), MAX_JOBS);
    }

    const BENCH_A: &str = "--- BENCH ---\nmode: fixed\nconcurrency: 4\n\n--- ENDPOINT ---\npkg.Svc/M\n\n--- ASSERTS ---\n.ok\n";
    const BENCH_B: &str = "--- BENCH ---\nmode: fixed\nconcurrency: 9\n\n--- ENDPOINT ---\npkg.Svc/M\n\n--- ASSERTS ---\n.ok\n";

    #[cfg_attr(miri, ignore)]
    #[tokio::test]
    async fn a_bench_with_no_files_is_a_bad_request() {
        let dir = tempfile::tempdir().expect("tempdir");
        let err = create_job(
            State(state_in(dir.path())),
            Json(CreateJobRequest {
                data: None,
                kind: JobKind::Bench,
                reports: vec![],
                paths: vec![],
                up_to_step: None,
            }),
        )
        .await
        .expect_err("a bench needs a file");
        assert_eq!(err.0, StatusCode::BAD_REQUEST);
    }

    fn plan_of(content: &str) -> (u32, f64) {
        let doc = crate::parser::parse_gctf_from_str(content, "t.httf").expect("parses");
        retry_plan(&doc)
    }

    #[test]
    fn a_run_reads_the_retry_the_file_asks_for() {
        let base = "--- ADDRESS ---\nhttp://127.0.0.1:1\n\n--- ENDPOINT ---\nGET /x\n\n--- ASSERTS ---\n.ok\n";
        assert_eq!(plan_of(base), (0, 0.0));
        assert_eq!(
            plan_of(&format!(
                "--- OPTIONS ---\nretry: 2\nretry_delay: 0.25\n\n{base}"
            )),
            (2, 0.25)
        );
    }

    #[test]
    fn no_retry_wins_over_a_count() {
        let base = "--- ADDRESS ---\nhttp://127.0.0.1:1\n\n--- ENDPOINT ---\nGET /x\n\n--- ASSERTS ---\n.ok\n";
        assert_eq!(
            plan_of(&format!(
                "--- OPTIONS ---\nretry: 5\nno_retry: true\n\n{base}"
            ))
            .0,
            0
        );
    }

    #[cfg_attr(miri, ignore)]
    #[tokio::test]
    async fn a_transport_failure_is_tried_again() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("dead.httf");
        std::fs::write(
            &path,
            "--- OPTIONS ---\nretry: 2\nretry_delay: 0.2\n\n--- ADDRESS ---\nhttp://127.0.0.1:1\n\n--- ENDPOINT ---\nGET /x\n\n--- ASSERTS ---\n.ok\n",
        )
        .expect("write");

        let started = std::time::Instant::now();
        let (event, _) = run_one("dead.httf", path, None, None, None, None).await;
        assert_eq!(event["event"], "test_fail");
        assert!(
            started.elapsed() >= std::time::Duration::from_millis(400),
            "two waits of 0.2s did not happen: {:?}",
            started.elapsed()
        );
    }

    const DATASET_FILE: &str = "--- ENDPOINT ---\ns.S/M\n\n--- REQUEST ---\n{\"id\": \"{{dataset.id}}\"}\n\n--- DATASET ---\n- id: \"1\"\n- id: \"2\"\n- id: \"3\"\n";

    #[cfg_attr(miri, ignore)]
    #[test]
    fn a_file_with_a_dataset_becomes_one_case_per_row() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("d.gctf");
        std::fs::write(&path, DATASET_FILE).expect("write");

        let rows = dataset_rows(&path).expect("the file has a DATASET");
        assert_eq!(rows.len(), 3);

        let work = work_list(vec![("d.gctf".to_string(), path, rows)], &[], &Vars::new());
        assert_eq!(work.len(), 3);
        assert!(work[1].0.contains("row=1"));
        assert_eq!(
            work[1].2.as_ref().and_then(|v| v.get("dataset.id")),
            Some(&json!("2"))
        );
    }

    #[test]
    fn a_row_wins_over_the_environment_it_runs_under() {
        let mut base = Vars::new();
        base.insert("dataset.id".to_string(), json!("from-env"));
        let mut row = Vars::new();
        row.insert("dataset.id".to_string(), json!("from-row"));

        let work = work_list(
            vec![(
                "d.gctf".to_string(),
                std::path::PathBuf::from("d.gctf"),
                vec![row],
            )],
            &[],
            &base,
        );
        assert_eq!(
            work[0].2.as_ref().and_then(|v| v.get("dataset.id")),
            Some(&json!("from-row"))
        );
    }

    #[cfg_attr(miri, ignore)]
    #[test]
    fn a_file_without_a_dataset_is_one_case() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("p.gctf");
        std::fs::write(&path, "--- ENDPOINT ---\ns.S/M\n\n--- REQUEST ---\n{}\n").expect("write");
        assert!(dataset_rows(&path).is_none());
        assert_eq!(
            work_list(
                vec![("p.gctf".to_string(), path, vec![])],
                &[],
                &Vars::new()
            )
            .len(),
            1
        );
    }

    #[cfg_attr(miri, ignore)]
    #[tokio::test]
    async fn a_dataset_and_a_data_source_are_refused_together() {
        let dir = tempfile::tempdir().expect("tempdir");
        std::fs::write(dir.path().join("d.gctf"), DATASET_FILE).expect("write");
        std::fs::write(dir.path().join("rows.ndjson"), "{\"id\": \"9\"}\n").expect("write");
        let err = create_job(
            State(state_in(dir.path())),
            Json(CreateJobRequest {
                data: Some("rows.ndjson".to_string()),
                kind: JobKind::Run,
                reports: vec![],
                paths: vec!["d.gctf".to_string()],
                up_to_step: None,
            }),
        )
        .await
        .expect_err("two row sources");
        assert_eq!(err.0, StatusCode::BAD_REQUEST);
        assert!(err.1.contains("DATASET"), "{}", err.1);
    }

    #[cfg_attr(miri, ignore)]
    #[tokio::test]
    async fn a_dataset_with_no_rows_is_refused() {
        let dir = tempfile::tempdir().expect("tempdir");
        std::fs::write(
            dir.path().join("d.gctf"),
            "--- ENDPOINT ---\ns.S/M\n\n--- REQUEST ---\n{}\n\n--- DATASET ---\n",
        )
        .expect("write");
        let err = create_job(
            State(state_in(dir.path())),
            Json(CreateJobRequest {
                data: None,
                kind: JobKind::Run,
                reports: vec![],
                paths: vec!["d.gctf".to_string()],
                up_to_step: None,
            }),
        )
        .await
        .expect_err("no rows");
        assert_eq!(err.0, StatusCode::BAD_REQUEST);
        assert!(err.1.contains("zero rows"), "{}", err.1);
    }

    #[cfg_attr(miri, ignore)]
    #[tokio::test]
    async fn a_bench_of_files_that_disagree_is_refused() {
        let dir = tempfile::tempdir().expect("tempdir");
        std::fs::write(dir.path().join("a.gctf"), BENCH_A).expect("write");
        std::fs::write(dir.path().join("b.gctf"), BENCH_B).expect("write");

        let err = create_job(
            State(state_in(dir.path())),
            Json(CreateJobRequest {
                data: None,
                kind: JobKind::Bench,
                reports: vec![],
                paths: vec!["a.gctf".to_string(), "b.gctf".to_string()],
                up_to_step: None,
            }),
        )
        .await
        .expect_err("two files, two BENCH configs — the CLI refuses this too");
        assert_eq!(err.0, StatusCode::BAD_REQUEST);
        assert!(err.1.contains("BENCH"), "says what disagreed: {}", err.1);
    }

    #[cfg_attr(miri, ignore)]
    #[tokio::test]
    async fn a_bench_of_files_sharing_one_config_is_accepted() {
        let dir = tempfile::tempdir().expect("tempdir");
        std::fs::write(dir.path().join("a.gctf"), BENCH_A).expect("write");
        std::fs::write(dir.path().join("b.gctf"), BENCH_A).expect("write");

        let job = create_job(
            State(state_in(dir.path())),
            Json(CreateJobRequest {
                data: None,
                kind: JobKind::Bench,
                reports: vec![],
                paths: vec!["a.gctf".to_string(), "b.gctf".to_string()],
                up_to_step: None,
            }),
        )
        .await
        .expect("two files, one BENCH config");
        assert_eq!(job.0.total, 2);
    }

    #[test]
    fn run_to_here_keeps_the_steps_up_to_the_one_asked_for() {
        let src = "--- ENDPOINT ---\na.A/One\n\n--- REQUEST ---\n{}\n\n--- ENDPOINT ---\nb.B/Two\n\n--- REQUEST ---\n{}\n\n--- ENDPOINT ---\nc.C/Three\n\n--- REQUEST ---\n{}\n";
        let steps_of = |doc: &crate::parser::GctfDocument| doc.iter_chain().count();

        let mut two = crate::parser::parse_gctf_from_str(src, "chain.gctf").expect("parse");
        truncate_chain(&mut two, 2);
        assert_eq!(steps_of(&two), 2);

        let mut whole = crate::parser::parse_gctf_from_str(src, "chain.gctf").expect("parse");
        truncate_chain(&mut whole, 0);
        assert_eq!(steps_of(&whole), 3, "0 means the file as written");

        let mut past_the_end =
            crate::parser::parse_gctf_from_str(src, "chain.gctf").expect("parse");
        truncate_chain(&mut past_the_end, 9);
        assert_eq!(steps_of(&past_the_end), 3);
    }

    #[test]
    fn a_failure_carries_the_exchange_unless_it_is_enormous() {
        use std::collections::HashMap;

        let small = crate::grpc::GrpcResponse {
            headers: HashMap::from([("content-type".to_string(), "application/grpc".to_string())]),
            trailers: HashMap::new(),
            messages: vec![serde_json::json!({ "ok": false })],
            error: Some("assertion failed".to_string()),
        };
        let carried = bounded_response(&small).expect("a small response is carried");
        assert_eq!(carried["messages"][0]["ok"], false);
        assert_eq!(carried["error"], "assertion failed");
        assert_eq!(carried["headers"]["content-type"], "application/grpc");

        let huge = crate::grpc::GrpcResponse {
            headers: HashMap::new(),
            trailers: HashMap::new(),
            messages: vec![serde_json::json!({ "blob": "x".repeat(MAX_CAPTURED_BYTES) })],
            error: None,
        };
        assert!(
            bounded_response(&huge).is_none(),
            "an oversized exchange is dropped rather than streamed"
        );
    }

    #[cfg_attr(miri, ignore)]
    #[tokio::test]
    async fn a_cancelled_job_skips_the_files_it_has_not_reached() {
        let dir = tempfile::tempdir().expect("tempdir");
        let file = dir.path().join("a.gctf");
        std::fs::write(
            &file,
            "--- ENDPOINT ---\npkg.Svc/M\n\n--- ASSERTS ---\n.ok\n",
        )
        .expect("write");

        let job = job_with("cancelled");
        job.cancel.store(true, Ordering::Relaxed);
        run_job(
            job.clone(),
            vec![
                ("a.gctf".to_string(), file, vec![]),
                ("b.gctf".to_string(), dir.path().join("b.gctf"), vec![]),
            ],
            None,
            dir.path().to_path_buf(),
            None,
            std::collections::HashMap::new(),
        )
        .await;

        let summary = job.summary();
        assert!(matches!(summary.status, JobStatus::Cancelled));
        assert_eq!(summary.skipped, 2, "nothing runs after a cancel");
        let events = lock(&job.state).events.clone();
        assert_eq!(events.first().expect("first")["event"], "suite_start");
        assert_eq!(events.last().expect("last")["event"], "suite_end");
    }

    #[cfg_attr(miri, ignore)]
    #[tokio::test]
    async fn a_file_that_verifies_nothing_is_refused_the_way_run_refuses_it() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("nocheck.gctf");
        std::fs::write(
            &path,
            "--- ADDRESS ---\nlocalhost:4770\n\n--- ENDPOINT ---\npkg.Svc/M\n\n--- REQUEST ---\n{}\n",
        )
        .expect("write");

        let (event, _) = run_one("nocheck.gctf", path, None, None, None, None).await;
        assert_eq!(event["event"], "test_fail");
        assert!(
            event["message"].as_str().is_some_and(
                |m| m.contains("Validation error") && m.contains("verification section")
            ),
            "{event}"
        );
    }

    #[cfg_attr(miri, ignore)]
    #[tokio::test]
    async fn a_suite_runs_at_the_width_the_command_line_runs_at() {
        let dir = tempfile::tempdir().expect("tempdir");
        let mut files = Vec::new();
        for i in 0..6 {
            let name = format!("dead{i}.httf");
            let path = dir.path().join(&name);
            std::fs::write(
                &path,
                "--- ADDRESS ---\nhttp://127.0.0.1:1\n\n--- ENDPOINT ---\nGET /x\n\n--- ASSERTS ---\n@status() == 200\n",
            )
            .expect("write");
            files.push((name, path, vec![]));
        }

        let job = job_with("wide");
        run_job(
            job.clone(),
            files,
            None,
            dir.path().to_path_buf(),
            None,
            std::collections::HashMap::new(),
        )
        .await;

        let events = lock(&job.state).events.clone();
        let workers = events.first().expect("first")["workers"]
            .as_u64()
            .expect("the suite says how wide it ran");
        assert!(workers >= 1);
        let started: Vec<&Value> = events
            .iter()
            .filter(|e| e["event"] == "test_start")
            .collect();
        assert_eq!(started.len(), 6, "every file starts");
        let summary = job.summary();
        assert_eq!(summary.failed, 6, "every file is judged");
        assert_eq!(summary.passed + summary.skipped, 0);
    }
}
