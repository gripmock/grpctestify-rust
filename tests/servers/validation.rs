// Validation test server implementation

use std::net::SocketAddr;
use tonic::{Request, Response, Status};

use crate::servers::{TestServerConfig, TestServerHandle};

// Include generated proto code
pub mod validation_proto {
    tonic::include_proto!("validation");
}

use validation_proto::{
    EmailRequest, EmailResponse, IPRequest, IPResponse, TimestampRequest, TimestampResponse,
    URLRequest, URLResponse, UUIDRequest, UUIDResponse,
    validation_service_server::{ValidationService, ValidationServiceServer},
};

/// Validation service implementation
#[derive(Debug, Default)]
pub struct ValidationServiceImpl;

#[tonic::async_trait]
impl ValidationService for ValidationServiceImpl {
    async fn validate_email(
        &self,
        request: Request<EmailRequest>,
    ) -> Result<Response<EmailResponse>, Status> {
        let inner = request.into_inner();
        let valid = inner.email.contains('@') && inner.email.contains('.');

        Ok(Response::new(EmailResponse {
            valid,
            normalized: inner.email.to_lowercase(),
            domain: inner.email.split('@').nth(1).unwrap_or("").to_string(),
        }))
    }

    async fn validate_uuid(
        &self,
        request: Request<UUIDRequest>,
    ) -> Result<Response<UUIDResponse>, Status> {
        let inner = request.into_inner();
        let valid = uuid::Uuid::parse_str(&inner.uuid).is_ok();

        Ok(Response::new(UUIDResponse {
            valid,
            version: if valid {
                "4".to_string()
            } else {
                String::new()
            },
            variant: if valid {
                "RFC4122".to_string()
            } else {
                String::new()
            },
        }))
    }

    async fn validate_url(
        &self,
        request: Request<URLRequest>,
    ) -> Result<Response<URLResponse>, Status> {
        let inner = request.into_inner();
        let valid = inner.url.starts_with("http://") || inner.url.starts_with("https://");

        Ok(Response::new(URLResponse {
            valid,
            scheme: if valid {
                inner.url.split("://").next().unwrap_or("").to_string()
            } else {
                String::new()
            },
            host: String::new(),
            path: String::new(),
        }))
    }

    async fn validate_ip(
        &self,
        request: Request<IPRequest>,
    ) -> Result<Response<IPResponse>, Status> {
        let inner = request.into_inner();
        let valid = inner.ip.parse::<std::net::IpAddr>().is_ok();
        let version = if inner.ip.contains(':') { "v6" } else { "v4" };

        Ok(Response::new(IPResponse {
            valid,
            version: version.to_string(),
            is_private: false,
        }))
    }

    async fn validate_timestamp(
        &self,
        request: Request<TimestampRequest>,
    ) -> Result<Response<TimestampResponse>, Status> {
        let inner = request.into_inner();

        // Try to parse ISO 8601 format
        let valid = inner.timestamp.contains('T') || inner.timestamp.parse::<i64>().is_ok();

        Ok(Response::new(TimestampResponse {
            valid,
            unix_timestamp: if valid { 1703462400 } else { 0 },
            iso_format: inner.timestamp,
        }))
    }
}

/// Start validation test server
pub async fn start_validation_server(
    config: TestServerConfig,
) -> Result<TestServerHandle, Box<dyn std::error::Error>> {
    let addr = format!("{}:{}", config.host, config.port + 2).parse::<SocketAddr>()?;
    let validation_service = ValidationServiceImpl::default();

    let server = Server::builder()
        .add_service(ValidationServiceServer::new(validation_service))
        .serve(addr)
        .await?;

    Ok(TestServerHandle {
        handle: tokio::spawn(server),
        address: addr,
    })
}
