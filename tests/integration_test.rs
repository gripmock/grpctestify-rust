use grpctestify::grpc::client::{CompressionMode, GrpcClient, GrpcClientConfig};
use std::net::SocketAddr;
use tokio::sync::oneshot;
use tonic::{transport::Server, Request, Response, Status};

// Include the generated code
pub mod helloworld {
    tonic::include_proto!("helloworld");
}

use helloworld::greeter_server::{Greeter, GreeterServer};
use helloworld::{HelloReply, HelloRequest};

#[derive(Debug, Default)]
pub struct MyGreeter {}

#[tonic::async_trait]
impl Greeter for MyGreeter {
    async fn say_hello(
        &self,
        request: Request<HelloRequest>,
    ) -> Result<Response<HelloReply>, Status> {
        let name = request.into_inner().name;
        let reply = HelloReply {
            message: format!("Hello {}!", name),
        };
        Ok(Response::new(reply))
    }
}

async fn start_server() -> (SocketAddr, oneshot::Sender<()>) {
    let listener = tokio::net::TcpListener::bind("[::1]:0").await.unwrap();
    let addr = listener.local_addr().unwrap();

    let (tx, rx) = oneshot::channel();

    let greeter = MyGreeter::default();

    // Load descriptor set for reflection
    let descriptor_set = std::fs::read(
        std::path::PathBuf::from(std::env::var("OUT_DIR").unwrap())
            .join("helloworld_descriptor.bin"),
    )
    .unwrap();

    let reflection_service = tonic_reflection::server::Builder::configure()
        .register_encoded_file_descriptor_set(&descriptor_set)
        .build_v1()
        .unwrap();

    tokio::spawn(async move {
        Server::builder()
            .add_service(GreeterServer::new(greeter))
            .add_service(reflection_service)
            .serve_with_incoming_shutdown(
                tokio_stream::wrappers::TcpListenerStream::new(listener),
                async {
                    rx.await.ok();
                },
            )
            .await
            .unwrap();
    });

    // Give the server a moment to start accepting connections
    tokio::time::sleep(std::time::Duration::from_millis(100)).await;

    (addr, tx)
}

fn init_tracing() {
    let _ = tracing_subscriber::fmt()
        .with_max_level(tracing::Level::DEBUG)
        .with_test_writer()
        .try_init();
}

#[tokio::test]
async fn test_reflect_command() {
    init_tracing();
    let (addr, tx) = start_server().await;

    let config = GrpcClientConfig {
        address: format!("http://{}", addr),
        timeout_seconds: 5,
        tls_config: None,
        proto_config: None,
        metadata: None,
        target_service: None,
        compression: CompressionMode::None,
    };

    let client = GrpcClient::new(config)
        .await
        .expect("Failed to create client");

    // Test listing services
    let services = client.describe(None).expect("Failed to describe services");
    println!("Services found:\n{}", services);
    assert!(services.contains("Greeter") || services.contains("helloworld.Greeter"));

    // Test describing service
    let service_desc = client
        .describe(Some("helloworld.Greeter"))
        .or_else(|_| client.describe(Some("Greeter")))
        .expect("Failed to describe service");
    println!("Service description:\n{}", service_desc);
    assert!(service_desc.contains("rpc SayHello"));

    let _ = tx.send(());
}

#[tokio::test]
async fn test_run_command_logic() {
    init_tracing();
    let (addr, tx) = start_server().await;

    let config = GrpcClientConfig {
        address: format!("http://{}", addr),
        timeout_seconds: 5,
        tls_config: None,
        proto_config: None,
        metadata: None,
        target_service: None,
        compression: CompressionMode::None,
    };

    // Create client, which loads descriptors
    let mut client = GrpcClient::new(config)
        .await
        .expect("Failed to create client");

    // We must ensure the client is ready to make calls.
    // The channel inside client.client might be closed if the server sent GoAway on the OTHER connection? No.

    let request_json = serde_json::json!({
        "name": "World"
    });

    let response = client
        .call("helloworld.Greeter", "SayHello", vec![request_json])
        .await
        .expect("Call failed");

    assert_eq!(response.messages.len(), 1);
    let msg = &response.messages[0];
    assert_eq!(msg["message"], "Hello World!");

    let _ = tx.send(());
}
