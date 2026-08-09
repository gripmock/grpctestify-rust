#![allow(clippy::unwrap_used, clippy::expect_used)] // test/bench code
#![cfg(not(miri))]

//! The calibration target has to behave like a real gRPC server, or the floor
//! it reports is not comparable with a real run.

use grpctestify::bench::calibrate::CalibrationTarget;

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn the_calibration_target_answers_any_unary_method() {
    let target = CalibrationTarget::spawn().await.unwrap();
    let dir = tempfile::tempdir().unwrap();
    std::fs::copy("tests/proto/test.proto", dir.path().join("test.proto")).unwrap();
    let file = dir.path().join("any.gctf");
    let report = dir.path().join("report.json");
    std::fs::write(
        &file,
        format!(
            "--- ADDRESS ---\n{}\n\n--- ENDPOINT ---\ntest.TestService/Test\n\n--- PROTO ---\nfiles: test.proto\nimport_paths: .\n\n--- BENCH ---\nmode: fixed\nrequests: 50\nconcurrency: 4\n\n--- REQUEST ---\n{{\"name\": \"x\"}}\n\n--- RESPONSE partial ---\n{{}}\n",
            target.address()
        ),
    )
    .unwrap();

    let output = std::process::Command::new(env!("CARGO_BIN_EXE_grpctestify"))
        .args([
            "bench",
            &file.to_string_lossy(),
            "--log-format",
            "json",
            "--log-output",
            &report.to_string_lossy(),
        ])
        .output()
        .expect("failed to run bench");

    let json: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(&report).unwrap()).unwrap();
    let summary = &json["summary"];
    assert_eq!(
        summary["ok"].as_u64().unwrap(),
        50,
        "every call must return OK: stderr:\n{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(summary["errors"].as_u64().unwrap(), 0);
    assert!(summary["average_ns"].as_u64().unwrap() > 0);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn dropping_the_target_frees_its_port() {
    let address = {
        let target = CalibrationTarget::spawn().await.unwrap();
        target.address().to_string()
    };
    tokio::time::sleep(std::time::Duration::from_millis(50)).await;
    assert!(
        tokio::net::TcpStream::connect(&address).await.is_err(),
        "{address} still accepts connections after the target was dropped"
    );
}

// The target must serve every RPC mode, not only unary.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn the_calibration_target_serves_streaming_methods() {
    let target = CalibrationTarget::spawn().await.unwrap();
    let dir = tempfile::tempdir().unwrap();
    std::fs::copy(
        "tests/servers/proto/echo.proto",
        dir.path().join("echo.proto"),
    )
    .unwrap();

    for (name, endpoint, body, expect) in [
        (
            "server_stream",
            "echo.EchoService/ServerStream",
            "{\"count\": 1}",
            "--- RESPONSE ---\n{}\n",
        ),
        (
            "client_stream",
            "echo.EchoService/Repeat",
            "{\"message\": \"a\"}\n{\"message\": \"b\"}",
            "--- RESPONSE ---\n{}\n",
        ),
        (
            "bidi",
            "echo.EchoService/BidiStream",
            "{\"message\": \"a\"}",
            "--- RESPONSE ---\n{}\n",
        ),
    ] {
        let file = dir.path().join(format!("{name}.gctf"));
        std::fs::write(
            &file,
            format!(
                "--- ADDRESS ---\n{}\n\n--- ENDPOINT ---\n{endpoint}\n\n--- PROTO ---\nfiles: echo.proto\nimport_paths: .\n\n--- REQUEST ---\n{body}\n\n{expect}",
                target.address()
            ),
        )
        .unwrap();

        let output = std::process::Command::new(env!("CARGO_BIN_EXE_grpctestify"))
            .args(["run", &file.to_string_lossy()])
            .output()
            .expect("failed to run");
        assert!(
            output.status.success(),
            "{name} failed against the calibration target:\n{}",
            String::from_utf8_lossy(&output.stdout)
        );
    }
}
