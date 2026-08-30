#![allow(clippy::unwrap_used, clippy::expect_used)] // test/bench code
// Echo test server implementation

use std::net::SocketAddr;
use tokio::sync::mpsc;
use tokio_stream::wrappers::ReceiverStream;
use tonic::transport::Server;
use tonic::{Request, Response, Status, Streaming};

use crate::servers::{TestServerConfig, TestServerHandle};

// Include generated proto code
pub mod echo_proto {
    tonic::include_proto!("echo");
}

use echo_proto::{
    BidiRequest, BidiResponse, EchoRequest, EchoResponse, HelloRequest, HelloResponse,
    RepeatRequest, RepeatResponse, StreamRequest, StreamResponse,
    echo_service_server::{EchoService, EchoServiceServer},
};

/// Echo service implementation
#[derive(Debug, Default)]
pub struct EchoServiceImpl;

#[tonic::async_trait]
impl EchoService for EchoServiceImpl {
    async fn say_hello(
        &self,
        request: Request<HelloRequest>,
    ) -> Result<Response<HelloResponse>, Status> {
        let message = request.into_inner().message;
        Ok(Response::new(HelloResponse {
            message: format!("Hello, {}!", message),
        }))
    }

    async fn echo(&self, request: Request<EchoRequest>) -> Result<Response<EchoResponse>, Status> {
        let inner = request.into_inner();
        Ok(Response::new(EchoResponse {
            text: inner.text,
            count: inner.count,
        }))
    }

    async fn repeat(
        &self,
        request: Request<Streaming<RepeatRequest>>,
    ) -> Result<Response<RepeatResponse>, Status> {
        let mut stream = request.into_inner();
        let mut messages = Vec::new();

        while let Some(result) = stream.message().await? {
            messages.push(result.message);
        }

        Ok(Response::new(RepeatResponse {
            total_messages: messages.len() as i32,
            concatenated: messages.join(" "),
        }))
    }

    type ServerStreamStream = ReceiverStream<Result<StreamResponse, Status>>;

    async fn server_stream(
        &self,
        request: Request<StreamRequest>,
    ) -> Result<Response<Self::ServerStreamStream>, Status> {
        let inner = request.into_inner();
        let (tx, rx) = mpsc::channel(4);

        tokio::spawn(async move {
            for i in 0..inner.count {
                let response = StreamResponse {
                    index: i,
                    message: format!("{} #{}", inner.prefix, i),
                };
                if tx.send(Ok(response)).await.is_err() {
                    break;
                }
            }
        });

        Ok(Response::new(ReceiverStream::new(rx)))
    }

    type BidiStreamStream = ReceiverStream<Result<BidiResponse, Status>>;

    async fn bidi_stream(
        &self,
        request: Request<Streaming<BidiRequest>>,
    ) -> Result<Response<Self::BidiStreamStream>, Status> {
        let mut stream = request.into_inner();
        let (tx, rx) = mpsc::channel(4);

        tokio::spawn(async move {
            let mut sequence = 0;
            while let Ok(Some(req)) = stream.message().await {
                if req.send_response {
                    let response = BidiResponse {
                        echo: req.message,
                        sequence,
                    };
                    sequence += 1;
                    if tx.send(Ok(response)).await.is_err() {
                        break;
                    }
                }
            }
        });

        Ok(Response::new(ReceiverStream::new(rx)))
    }
}

/// Start echo test server
pub async fn start_echo_server(
    config: TestServerConfig,
) -> Result<TestServerHandle, Box<dyn std::error::Error>> {
    let addr = format!("{}:{}", config.host, config.port).parse::<SocketAddr>()?;
    let echo_service = EchoServiceImpl;

    let server = tokio::spawn(async move {
        Server::builder()
            .add_service(EchoServiceServer::new(echo_service))
            .serve(addr)
            .await
    });

    Ok(TestServerHandle { handle: server })
}

/// The echo service on an ephemeral port with no reflection at all — the shape
/// of most real servers, and the one that shows whether a caller read the
/// document's `PROTO` section. Returns the address it is listening on.
pub async fn spawn_echo_server_without_reflection() -> String {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind ephemeral port");
    let addr = listener.local_addr().expect("local addr");
    let incoming = tokio_stream::wrappers::TcpListenerStream::new(listener);

    tokio::spawn(async move {
        Server::builder()
            .add_service(EchoServiceServer::new(EchoServiceImpl))
            .serve_with_incoming(incoming)
            .await
            .expect("echo server run");
    });

    addr.to_string()
}

/// The echo service on an ephemeral port, with reflection, for tests that call
/// it through the play server — which reaches for descriptors before it dials.
/// Returns the address it is listening on.
pub async fn spawn_echo_server_with_reflection() -> String {
    let reflection = tonic_reflection::server::Builder::configure()
        .register_encoded_file_descriptor_set(include_bytes!(concat!(
            env!("OUT_DIR"),
            "/test_servers_descriptor.bin"
        )))
        .build_v1()
        .expect("build reflection service");

    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind ephemeral port");
    let addr = listener.local_addr().expect("local addr");
    let incoming = tokio_stream::wrappers::TcpListenerStream::new(listener);

    tokio::spawn(async move {
        Server::builder()
            .add_service(EchoServiceServer::new(EchoServiceImpl))
            .add_service(reflection)
            .serve_with_incoming(incoming)
            .await
            .expect("echo server run");
    });

    addr.to_string()
}
