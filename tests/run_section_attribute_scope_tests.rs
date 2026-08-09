#![allow(clippy::unwrap_used, clippy::expect_used)] // test/bench code

//! `#[skip]` and `#[repeat(N)]` configure one section and must not propagate.
//!
//! Regression: resolved attributes were fed back into the inherited set
//! wholesale, and `resolve_attributes` offers no way to un-set an inherited
//! value. A single `#[skip]` therefore disabled every *following* section
//! too — a document that skipped its RESPONSE silently stopped running its
//! ASSERTS as well, and reported a pass. The documented behaviour is the
//! opposite: "A skipped section is ignored during execution. The test
//! continues with subsequent sections."

#[path = "support/mod.rs"]
mod support;
use support::{cli_command, spawn_health_server};

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn skip_on_one_section_does_not_disable_the_next() {
    let address = spawn_health_server().await;
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("skip.gctf");
    // RESPONSE is skipped; the ASSERTS block that follows is deliberately
    // false and must still run.
    std::fs::write(
        &file,
        format!(
            "--- ADDRESS ---\n{address}\n\n--- ENDPOINT ---\ngrpc.health.v1.Health/Check\n\n--- REQUEST ---\n{{}}\n\n#[skip]\n--- RESPONSE ---\n{{\"status\": \"WRONG\"}}\n\n--- ASSERTS ---\n.status == \"DEFINITELY_WRONG\"\n"
        ),
    )
    .unwrap();

    let output = cli_command()
        .args(["run", &file.to_string_lossy()])
        .output()
        .expect("failed to run");

    assert!(
        !output.status.success(),
        "the ASSERTS section after a skipped RESPONSE must still be evaluated: stdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn skip_still_skips_its_own_section() {
    let address = spawn_health_server().await;
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("skip-own.gctf");
    // The only wrong expectation is inside the skipped section, so the run
    // must pass.
    std::fs::write(
        &file,
        format!(
            "--- ADDRESS ---\n{address}\n\n--- ENDPOINT ---\ngrpc.health.v1.Health/Check\n\n--- REQUEST ---\n{{}}\n\n#[skip]\n--- RESPONSE ---\n{{\"status\": \"WRONG\"}}\n"
        ),
    )
    .unwrap();

    let output = cli_command()
        .args(["run", &file.to_string_lossy()])
        .output()
        .expect("failed to run");

    assert!(
        output.status.success(),
        "a skipped section must not be evaluated: stdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}
