#![allow(clippy::unwrap_used, clippy::expect_used)] // test/bench code
use grpctestify::cli::args::HealthArgs;
use grpctestify::commands::health::handle_health;
use tonic_health::ServingStatus;

/// Spawn a real `grpc.health.v1.Health` server (plus reflection — the
/// grpctestify client always discovers message shapes via reflection, even
/// for the well-known health messages) on an ephemeral port.
async fn spawn_health_server() -> (String, tonic_health::server::HealthReporter) {
    let (reporter, health_service) = tonic_health::server::health_reporter();
    let reflection_service = tonic_reflection::server::Builder::configure()
        .register_encoded_file_descriptor_set(tonic_health::pb::FILE_DESCRIPTOR_SET)
        .build_v1()
        .expect("build reflection service");

    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind ephemeral port");
    let addr = listener.local_addr().expect("local addr");
    let incoming = tokio_stream::wrappers::TcpListenerStream::new(listener);

    tokio::spawn(async move {
        tonic::transport::Server::builder()
            .add_service(health_service)
            .add_service(reflection_service)
            .serve_with_incoming(incoming)
            .await
            .expect("health server run");
    });

    (addr.to_string(), reporter)
}

fn args(address: String, service: &str) -> HealthArgs {
    HealthArgs {
        address,
        protocol: "grpc".to_string(),
        service: if service.is_empty() {
            Vec::new()
        } else {
            vec![service.to_string()]
        },
        format: "json".to_string(),
        tls: false,
        insecure: false,
        timeout: 5,
        watch: false,
        interval: 1.0,
        watch_timeout: 60.0,
    }
}

#[tokio::test]
async fn overall_serving_exits_ok() {
    let (address, reporter) = spawn_health_server().await;
    reporter
        .set_service_status("", ServingStatus::Serving)
        .await;

    handle_health(&args(address, ""))
        .await
        .expect("SERVING must be Ok");
}

#[tokio::test]
async fn named_service_not_serving_is_an_error() {
    let (address, reporter) = spawn_health_server().await;
    reporter
        .set_service_status("my.Service", ServingStatus::NotServing)
        .await;

    let err = handle_health(&args(address, "my.Service"))
        .await
        .expect_err("NOT_SERVING must be an error (non-zero exit)");
    assert!(err.to_string().contains("NOT_SERVING"));
}

#[tokio::test]
async fn unknown_status_is_an_error() {
    let (address, reporter) = spawn_health_server().await;
    reporter
        .set_service_status("my.Service", ServingStatus::Unknown)
        .await;

    let err = handle_health(&args(address, "my.Service"))
        .await
        .expect_err("UNKNOWN must be an error (non-zero exit)");
    assert!(err.to_string().contains("UNKNOWN"));
}

#[tokio::test]
async fn multiple_services_all_serving_is_ok() {
    let (address, reporter) = spawn_health_server().await;
    reporter
        .set_service_status("a.Service", ServingStatus::Serving)
        .await;
    reporter
        .set_service_status("b.Service", ServingStatus::Serving)
        .await;

    let mut a = args(address, "");
    a.service = vec!["a.Service".to_string(), "b.Service".to_string()];
    handle_health(&a).await.expect("both SERVING must be Ok");
}

#[tokio::test]
async fn one_of_several_services_not_serving_fails_and_names_it() {
    let (address, reporter) = spawn_health_server().await;
    reporter
        .set_service_status("a.Service", ServingStatus::Serving)
        .await;
    reporter
        .set_service_status("b.Service", ServingStatus::NotServing)
        .await;

    let mut a = args(address, "");
    a.service = vec!["a.Service".to_string(), "b.Service".to_string()];
    let err = handle_health(&a)
        .await
        .expect_err("one NOT_SERVING service must fail the whole check");
    assert!(
        err.to_string().contains("b.Service") && err.to_string().contains("NOT_SERVING"),
        "error must name the failing service: {err}"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn watch_mode_succeeds_once_service_becomes_serving() {
    let (address, reporter) = spawn_health_server().await;
    reporter
        .set_service_status("", ServingStatus::NotServing)
        .await;

    // Flip to SERVING shortly after the watch loop starts polling.
    let flip_reporter = reporter.clone();
    tokio::spawn(async move {
        tokio::time::sleep(std::time::Duration::from_millis(120)).await;
        flip_reporter
            .set_service_status("", ServingStatus::Serving)
            .await;
    });

    let mut a = args(address, "");
    a.watch = true;
    a.interval = 0.03;
    a.watch_timeout = 3.0;

    handle_health(&a)
        .await
        .expect("watch mode must succeed once the service becomes healthy");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn watch_mode_times_out_when_never_healthy() {
    let (address, reporter) = spawn_health_server().await;
    reporter
        .set_service_status("", ServingStatus::NotServing)
        .await;

    let mut a = args(address, "");
    a.watch = true;
    a.interval = 0.02;
    a.watch_timeout = 0.15;

    let err = handle_health(&a)
        .await
        .expect_err("watch mode must give up and fail after watch_timeout");
    assert!(err.to_string().contains("timed out"), "{err}");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn watch_mode_retries_through_unreachable_server() {
    // Nothing is listening on this address at all — GrpcClient::new itself
    // fails. Watch mode must treat that as "not ready yet" and keep polling
    // rather than propagating a hard connection error immediately.
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind ephemeral port");
    let address = listener.local_addr().expect("local addr").to_string();
    drop(listener); // free the port again; nothing will accept connections to it

    let mut a = args(address, "");
    a.watch = true;
    a.interval = 0.02;
    a.watch_timeout = 0.1;

    let err = handle_health(&a)
        .await
        .expect_err("must eventually give up, not hang or panic");
    assert!(err.to_string().contains("timed out"), "{err}");
}
