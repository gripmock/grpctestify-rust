use std::path::{Path, PathBuf};
use std::sync::Arc;

use serde_json::{Value, json};

use crate::commands::bench::{BenchConfigResolved, BenchProgress, run_benchmark};

pub enum BenchEvent {
    Progress(BenchProgress),
    Report(Box<Value>),
    Failed(String),
}

pub fn config_for(path: &Path) -> anyhow::Result<BenchConfigResolved> {
    let parsed = crate::parser::parse_with_recovery(path);
    let section = crate::commands::bench::extract_bench_section(&parsed.document);
    BenchConfigResolved::from_bench_section(section.as_ref())
}

pub fn config_for_all(paths: &[PathBuf]) -> anyhow::Result<BenchConfigResolved> {
    let section = crate::commands::bench::resolve_bench_section(paths, &[])?;
    BenchConfigResolved::from_bench_section(section.as_ref())
}

pub async fn run(
    paths: Vec<PathBuf>,
    cancel: Arc<std::sync::atomic::AtomicBool>,
    on_event: Arc<dyn Fn(BenchEvent) + Send + Sync>,
) {
    let mut config = match config_for_all(&paths) {
        Ok(config) => config,
        Err(e) => {
            on_event(BenchEvent::Failed(e.to_string()));
            return;
        }
    };

    let sink = Arc::clone(&on_event);
    config.progress_sink = Some(Arc::new(move |tick| sink(BenchEvent::Progress(tick))));
    let stopped = Arc::clone(&cancel);
    config.cancel = Some(cancel);

    match run_benchmark(&paths, &config, &[], &[], &[]).await {
        Ok(report) => {
            let verdict = if stopped.load(std::sync::atomic::Ordering::Relaxed) {
                None
            } else {
                report.failure_reason()
            };
            match serde_json::to_value(&report) {
                Ok(value) => on_event(BenchEvent::Report(Box::new(value))),
                Err(e) => on_event(BenchEvent::Failed(format!(
                    "the report could not be serialized: {e}"
                ))),
            }
            if let Some(why) = verdict {
                on_event(BenchEvent::Failed(why));
            }
        }
        Err(e) => on_event(BenchEvent::Failed(format!("{e:#}"))),
    }
}

pub fn progress_event(tick: &BenchProgress) -> Value {
    json!({
        "event": "bench_progress",
        "elapsed_s": tick.elapsed_s,
        "requests": tick.requests,
        "errors": tick.errors,
        "rps": tick.rps,
        "targetRps": tick.target_rps,
        "errorPct": tick.error_pct,
        "timestamp": apif_cfg_runtime::now_rfc3339(),
    })
}

pub fn report_event(report: Value) -> Value {
    json!({
        "event": "bench_report",
        "report": report,
        "timestamp": apif_cfg_runtime::now_rfc3339(),
    })
}

#[cfg(test)]
#[cfg(not(miri))]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicBool, Ordering};

    #[tokio::test]
    async fn a_cancelled_bench_stops_and_says_it_was_cancelled() {
        let dir = std::env::temp_dir().join(format!("gctf-bench-cancel-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let file = dir.join("load.gctf");
        std::fs::write(
            &file,
            "--- ADDRESS ---\n127.0.0.1:1\n\n--- ENDPOINT ---\na.A/One\n\n--- REQUEST ---\n{}\n\n--- ASSERTS ---\n.ok == true\n\n--- BENCH ---\nmode: fixed\nconcurrency: 2\nduration: 60s\n",
        )
        .unwrap();

        let cancel = Arc::new(AtomicBool::new(false));
        let flag = Arc::clone(&cancel);
        tokio::spawn(async move {
            tokio::time::sleep(std::time::Duration::from_millis(400)).await;
            flag.store(true, Ordering::Relaxed);
        });

        let ended: Arc<std::sync::Mutex<Option<Value>>> = Arc::new(std::sync::Mutex::new(None));
        let sink = Arc::clone(&ended);
        let started = std::time::Instant::now();
        run(
            vec![file],
            cancel,
            Arc::new(move |event| match event {
                BenchEvent::Report(report) => *sink.lock().unwrap() = Some(*report),
                BenchEvent::Failed(why) => *sink.lock().unwrap() = Some(json!({ "failed": why })),
                BenchEvent::Progress(_) => {}
            }),
        )
        .await;
        let elapsed = started.elapsed();

        assert!(
            elapsed < std::time::Duration::from_secs(20),
            "a cancelled 60 s bench must not run to its duration: took {elapsed:?}"
        );
        let report = ended
            .lock()
            .unwrap()
            .clone()
            .expect("a report was produced");
        let reason = report
            .pointer("/run/end_reason")
            .and_then(|v| v.as_str())
            .unwrap_or_default();
        assert_eq!(reason, "user_cancelled", "{report:#}");

        std::fs::remove_dir_all(&dir).ok();
    }
}
