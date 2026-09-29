use anyhow::{Context, Result};
use std::net::SocketAddr;
use tonic::Status;
use tonic::codec::{Codec, DecodeBuf, Decoder, EncodeBuf, Encoder};

#[derive(Clone, Copy, Default)]
struct NoopCodec;

impl Codec for NoopCodec {
    type Encode = ();
    type Decode = ();
    type Encoder = NoopCodec;
    type Decoder = NoopCodec;

    fn encoder(&mut self) -> Self::Encoder {
        *self
    }

    fn decoder(&mut self) -> Self::Decoder {
        *self
    }
}

impl Encoder for NoopCodec {
    type Item = ();
    type Error = Status;

    fn encode(&mut self, _item: Self::Item, _dst: &mut EncodeBuf<'_>) -> Result<(), Self::Error> {
        Ok(())
    }
}

impl Decoder for NoopCodec {
    type Item = ();
    type Error = Status;

    fn decode(&mut self, src: &mut DecodeBuf<'_>) -> Result<Option<Self::Item>, Self::Error> {
        use prost_reflect::bytes::Buf;
        src.advance(src.remaining());
        Ok(Some(()))
    }
}

#[derive(Clone, Copy)]
struct NoopService;

type NoopStream = std::pin::Pin<Box<dyn tokio_stream::Stream<Item = Result<(), Status>> + Send>>;

impl tonic::server::StreamingService<()> for NoopService {
    type Response = ();
    type ResponseStream = NoopStream;
    type Future = std::pin::Pin<
        Box<dyn std::future::Future<Output = Result<tonic::Response<NoopStream>, Status>> + Send>,
    >;

    fn call(&mut self, request: tonic::Request<tonic::Streaming<()>>) -> Self::Future {
        Box::pin(async move {
            let mut inbound = request.into_inner();
            let mut received = 0usize;
            while inbound.message().await?.is_some() {
                received += 1;
            }
            let replies = received.max(1);
            let stream: NoopStream = Box::pin(tokio_stream::iter((0..replies).map(|_| Ok(()))));
            Ok(tonic::Response::new(stream))
        })
    }
}

async fn answer(request: axum::extract::Request) -> axum::response::Response {
    tonic::server::Grpc::new(NoopCodec)
        .streaming(NoopService, request)
        .await
        .map(axum::body::Body::new)
}

pub struct CalibrationTarget {
    address: String,
    task: tokio::task::JoinHandle<()>,
}

impl CalibrationTarget {
    #[must_use]
    pub fn address(&self) -> &str {
        &self.address
    }

    pub async fn spawn() -> Result<Self> {
        let listener = tokio::net::TcpListener::bind(("127.0.0.1", 0))
            .await
            .context("failed to bind the calibration target")?;
        let local: SocketAddr = listener
            .local_addr()
            .context("failed to read the calibration target's address")?;
        let router = axum::Router::new().fallback(answer);
        let task = tokio::spawn(async move {
            let _ = axum::serve(listener, router).await;
        });
        Ok(Self {
            address: local.to_string(),
            task,
        })
    }
}

impl Drop for CalibrationTarget {
    fn drop(&mut self) {
        self.task.abort();
    }
}
