#![allow(clippy::unwrap_used, clippy::expect_used)] // test/bench code
#![cfg(not(miri))]

#[path = "support/mod.rs"]
mod support;

use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

struct Flaky {
    address: String,
    connections: Arc<AtomicUsize>,
}

async fn flaky_origin(drop_first: usize) -> Flaky {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind");
    let address = format!("http://{}", listener.local_addr().expect("addr"));
    let connections = Arc::new(AtomicUsize::new(0));
    let counter = connections.clone();
    tokio::spawn(async move {
        loop {
            let Ok((mut socket, _)) = listener.accept().await else {
                break;
            };
            let seen = counter.fetch_add(1, Ordering::SeqCst);
            if seen < drop_first {
                drop(socket);
                continue;
            }
            let mut buf = vec![0u8; 4096];
            let _ = tokio::io::AsyncReadExt::read(&mut socket, &mut buf).await;
            let _ = tokio::io::AsyncWriteExt::write_all(
                &mut socket,
                b"HTTP/1.1 200 OK\r\ncontent-length: 12\r\nconnection: close\r\n\r\n{\"ok\": true}",
            )
            .await;
        }
    });
    Flaky {
        address,
        connections,
    }
}

fn write_test(dir: &std::path::Path, name: &str, body: &str) -> std::path::PathBuf {
    let path = dir.join(name);
    std::fs::write(&path, body).expect("write test file");
    path
}

async fn run_cli(args: Vec<String>) -> std::process::Output {
    tokio::task::spawn_blocking(move || {
        support::cli_command()
            .args(&args)
            .output()
            .expect("failed to run grpctestify")
    })
    .await
    .expect("cli task")
}

/// The retry budget is the run's own — `--retry`/`OPTIONS.retry` — applied once,
/// not once per loop. Two loops multiplied it: `#[retry(2)]` cost nine dials,
/// and the inner loop's own hardcoded 100ms-per-attempt backoff was charged on
/// top of `--retry-delay`, which is why the delay schedule now lives in one
/// place (`send_with_retries`, unit-tested on a paused clock).
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn an_http_test_dials_exactly_as_often_as_its_retry_budget_allows() {
    let origin = flaky_origin(2).await;
    let dir = tempfile::tempdir().expect("tempdir");
    let file = write_test(
        dir.path(),
        "flaky.httf",
        &format!(
            "--- ADDRESS ---\n{}\n\n--- ENDPOINT ---\nGET /health\n\n--- ASSERTS ---\n@status() == 200\n",
            origin.address
        ),
    );

    let output = run_cli(vec![
        file.to_string_lossy().to_string(),
        "--retry".to_string(),
        "2".to_string(),
        "--retry-delay".to_string(),
        "0".to_string(),
    ])
    .await;

    assert!(
        output.status.success(),
        "the third dial answers:\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        origin.connections.load(Ordering::SeqCst),
        3,
        "a budget of two retries is three dials, not nine"
    );
}

/// `#[retry(N)]` on the section is the same budget the run resolves, so it too
/// is spent once — and `no_retry` outranks it.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_retry_attribute_is_spent_once_and_no_retry_outranks_it() {
    let origin = flaky_origin(99).await;
    let dir = tempfile::tempdir().expect("tempdir");
    let with_attribute = write_test(
        dir.path(),
        "attribute.httf",
        &format!(
            "--- ADDRESS ---\n{}\n\n--- ENDPOINT ---\nGET /health\n\n#[retry(2)]\n--- REQUEST ---\n{{}}\n\n--- ASSERTS ---\n@status() == 200\n",
            origin.address
        ),
    );

    let output = run_cli(vec![
        with_attribute.to_string_lossy().to_string(),
        "--retry-delay".to_string(),
        "0".to_string(),
    ])
    .await;
    assert!(!output.status.success(), "nothing ever answers");
    assert_eq!(
        origin.connections.load(Ordering::SeqCst),
        3,
        "`#[retry(2)]` is three dials in total"
    );

    let refusing = flaky_origin(99).await;
    let no_retry = write_test(
        dir.path(),
        "no-retry.httf",
        &format!(
            "--- ADDRESS ---\n{}\n\n--- ENDPOINT ---\nGET /health\n\n--- OPTIONS ---\nno_retry: true\n\n#[retry(2)]\n--- REQUEST ---\n{{}}\n\n--- ASSERTS ---\n@status() == 200\n",
            refusing.address
        ),
    );
    let output = run_cli(vec![
        no_retry.to_string_lossy().to_string(),
        "--retry".to_string(),
        "5".to_string(),
        "--retry-delay".to_string(),
        "0".to_string(),
    ])
    .await;
    assert!(!output.status.success());
    assert_eq!(
        refusing.connections.load(Ordering::SeqCst),
        1,
        "`no_retry` wins over `#[retry(2)]` and over --retry"
    );
}
