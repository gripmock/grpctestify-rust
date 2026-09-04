#![allow(clippy::unwrap_used, clippy::expect_used)] // test/bench code
#![cfg(feature = "test-servers")]

//! `grpctestify call` and `run`, against a server that does not serve
//! reflection — which is what most servers are.

use std::io::Write;

use grpctestify::cli::args::CallArgs;

#[path = "servers/servers.rs"]
mod servers;

const TEST_SERVERS_DESCRIPTOR: &[u8] =
    include_bytes!(concat!(env!("OUT_DIR"), "/test_servers_descriptor.bin"));

fn call_args(file: std::path::PathBuf) -> CallArgs {
    CallArgs {
        protocol: "grpc".to_string(),
        address: None,
        file: Some(file),
        endpoint: None,
        data: None,
        header: Vec::new(),
        doc_index: None,
        include: false,
        verbose: false,
        very_verbose: false,
        output: None,
        dump_header: None,
        silent: true,
        show_error: false,
        fail: false,
        location: false,
        connect_timeout: 5,
        insecure: false,
        plaintext: true,
        tls_ca: None,
        tls_cert: None,
        tls_key: None,
        max_time: 10,
        bench: false,
        concurrency: None,
        requests: None,
        duration: None,
    }
}

/// Regression: `call` built its client with `proto_config: None`, so the file's
/// `PROTO` section was read by `run` and ignored by `call` — the same file
/// passed one and failed the other with "does not serve reflection".
#[tokio::test]
async fn call_reads_the_descriptor_the_file_names() {
    let address = servers::echo::spawn_echo_server_without_reflection().await;

    let dir = tempfile::tempdir().unwrap();
    let descriptor = dir.path().join("test_servers.bin");
    std::fs::File::create(&descriptor)
        .unwrap()
        .write_all(TEST_SERVERS_DESCRIPTOR)
        .unwrap();

    let file = dir.path().join("echo.gctf");
    std::fs::write(
        &file,
        format!(
            "--- ADDRESS ---\n{address}\n\n\
             --- ENDPOINT ---\necho.EchoService/SayHello\n\n\
             --- PROTO ---\ndescriptor: {}\n\n\
             --- REQUEST ---\n{{\n  \"message\": \"World\"\n}}\n",
            descriptor.display()
        ),
    )
    .unwrap();

    grpctestify::commands::handle_call(&call_args(file))
        .await
        .expect("the call reads the descriptor rather than asking the server for one");
}

/// `--address` outranks the file's own ADDRESS section, the way `--protocol`
/// and the TLS flags do.
#[tokio::test]
async fn the_address_flag_is_where_the_call_goes() {
    let address = servers::echo::spawn_echo_server_without_reflection().await;

    let dir = tempfile::tempdir().unwrap();
    let descriptor = dir.path().join("test_servers.bin");
    std::fs::File::create(&descriptor)
        .unwrap()
        .write_all(TEST_SERVERS_DESCRIPTOR)
        .unwrap();

    let file = dir.path().join("echo.gctf");
    std::fs::write(
        &file,
        format!(
            "--- ADDRESS ---\n127.0.0.1:1\n\n\
             --- ENDPOINT ---\necho.EchoService/SayHello\n\n\
             --- PROTO ---\ndescriptor: {}\n\n\
             --- REQUEST ---\n{{\n  \"message\": \"World\"\n}}\n",
            descriptor.display()
        ),
    )
    .unwrap();

    let mut args = call_args(file);
    args.address = Some(address);

    grpctestify::commands::handle_call(&args)
        .await
        .expect("the flag decides where the call goes, not the file");
}

/// Regression: `{{NAME}}` was documented as coming from the project's active
/// `.env.<name>`, and came from nowhere — a file using one failed every run
/// with "Unresolved variable placeholder", in the workbench and in CI alike.
#[tokio::test]
async fn a_run_resolves_placeholders_from_the_active_environment() {
    let address = servers::echo::spawn_echo_server_without_reflection().await;

    let dir = tempfile::tempdir().unwrap();
    grpctestify::serve::project::init_project_dir(dir.path()).unwrap();
    let root = dir.path().join(".grpctestify");

    let descriptor = dir.path().join("test_servers.bin");
    std::fs::File::create(&descriptor)
        .unwrap()
        .write_all(TEST_SERVERS_DESCRIPTOR)
        .unwrap();

    grpctestify::serve::project::write_dotenv(&root, "example", "WHO=placeholder\n").unwrap();
    /* The machine's own value wins, which is the whole point of `.local`. */
    grpctestify::serve::project::write_dotenv_local(&root, "example", "WHO=Ada\n").unwrap();

    let file = root.join("collections").join("greet.gctf");
    let body = "{\n  \"message\": \"{{WHO}}\"\n}";
    std::fs::write(
        &file,
        format!(
            "--- ADDRESS ---\n{address}\n\n\
             --- ENDPOINT ---\necho.EchoService/SayHello\n\n\
             --- PROTO ---\ndescriptor: {}\n\n\
             --- REQUEST ---\n{body}\n\n\
             --- ASSERTS ---\n.message == \"Hello, Ada!\"\n",
            descriptor.display()
        ),
    )
    .unwrap();

    let vars: std::collections::HashMap<String, serde_json::Value> =
        grpctestify::serve::project::project_variables(dir.path())
            .into_iter()
            .map(|(k, v)| (k, serde_json::Value::String(v)))
            .collect();
    assert_eq!(
        vars.get("WHO"),
        Some(&serde_json::Value::String("Ada".to_string())),
        "the local value wins over the shared one"
    );

    /* The runner with those variables — which is what both `run` and the job
    API hand it. The assertion compares against "Hello, Ada!", so it passes
    only if `{{WHO}}` reached the wire resolved. */
    let document = grpctestify::parse_gctf(&file).expect("parse");
    let result =
        grpctestify::execution::runner::TestRunner::new(false, 10, false, false, false, None)
            .run_test_with_variables(&document, vars)
            .await
            .expect("run");

    assert!(
        matches!(
            result.status,
            grpctestify::execution::runner::TestExecutionStatus::Pass
        ),
        "the placeholder resolved and the assert passed: {:?}",
        result.status
    );
}

/// Regression: the same `ADDRESS {{TARGET}}` resolved in an `.httf` — the HTTP
/// branch interpolates the address it dials — and was refused in a `.gctf` with
/// "Invalid address format", which never mentions the variable.
#[tokio::test]
async fn a_grpc_address_reads_the_variables_the_http_one_does() {
    let address = servers::echo::spawn_echo_server_without_reflection().await;

    let dir = tempfile::tempdir().unwrap();
    grpctestify::serve::project::init_project_dir(dir.path()).unwrap();
    let root = dir.path().join(".grpctestify");

    let descriptor = dir.path().join("test_servers.bin");
    std::fs::File::create(&descriptor)
        .unwrap()
        .write_all(TEST_SERVERS_DESCRIPTOR)
        .unwrap();

    grpctestify::serve::project::write_dotenv(&root, "example", &format!("TARGET={address}\n"))
        .unwrap();

    let file = root.join("collections").join("addr.gctf");
    std::fs::write(
        &file,
        format!(
            "--- ADDRESS ---\n{{{{TARGET}}}}\n\n\
             --- ENDPOINT ---\necho.EchoService/SayHello\n\n\
             --- PROTO ---\ndescriptor: {}\n\n\
             --- REQUEST ---\n{{\n  \"message\": \"Ada\"\n}}\n\n\
             --- ASSERTS ---\n.message == \"Hello, Ada!\"\n",
            descriptor.display()
        ),
    )
    .unwrap();

    let vars: std::collections::HashMap<String, serde_json::Value> =
        grpctestify::serve::project::project_variables(dir.path())
            .into_iter()
            .map(|(k, v)| (k, serde_json::Value::String(v)))
            .collect();

    let document = grpctestify::parse_gctf(&file).expect("parse");
    let result =
        grpctestify::execution::runner::TestRunner::new(false, 10, false, false, false, None)
            .run_test_with_variables(&document, vars)
            .await
            .expect("run");

    assert!(
        matches!(
            result.status,
            grpctestify::execution::runner::TestExecutionStatus::Pass
        ),
        "the address resolved and the call went to the echo server: {:?}",
        result.status
    );
}
