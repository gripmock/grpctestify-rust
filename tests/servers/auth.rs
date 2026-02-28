// Auth test server implementation

use std::net::SocketAddr;
use tonic::transport::Server;
use tonic::{Request, Response, Status};

use crate::servers::{TestServerConfig, TestServerHandle};

// Include generated proto code
pub mod auth_proto {
    tonic::include_proto!("auth");
}

use auth_proto::{
    LoginRequest, LoginResponse, LogoutRequest, LogoutResponse, RefreshTokenRequest,
    RefreshTokenResponse, ValidateTokenRequest, ValidateTokenResponse,
    auth_service_server::{AuthService, AuthServiceServer},
};

/// Auth service implementation
#[derive(Debug, Default)]
pub struct AuthServiceImpl;

#[tonic::async_trait]
impl AuthService for AuthServiceImpl {
    async fn login(
        &self,
        request: Request<LoginRequest>,
    ) -> Result<Response<LoginResponse>, Status> {
        let inner = request.into_inner();

        // Simple validation for testing
        if inner.username.is_empty() || inner.password.is_empty() {
            return Err(Status::invalid_argument("Username and password required"));
        }

        Ok(Response::new(LoginResponse {
            access_token: format!("eyJhbGciOiJIUzI1NiIsInR5cCI6IkpXVCJ9.{}", inner.username),
            refresh_token: "550e8400-e29b-41d4-a716-446655440000".to_string(),
            expires_in: 3600,
            token_type: "Bearer".to_string(),
        }))
    }

    async fn validate_token(
        &self,
        request: Request<ValidateTokenRequest>,
    ) -> Result<Response<ValidateTokenResponse>, Status> {
        let inner = request.into_inner();

        // Simple validation for testing
        let valid = inner.access_token.starts_with("eyJ");

        Ok(Response::new(ValidateTokenResponse {
            valid,
            user_id: if valid {
                "user-123".to_string()
            } else {
                String::new()
            },
            expires_at: if valid { 1735689600 } else { 0 },
        }))
    }

    async fn refresh_token(
        &self,
        request: Request<RefreshTokenRequest>,
    ) -> Result<Response<RefreshTokenResponse>, Status> {
        let _inner = request.into_inner();

        Ok(Response::new(RefreshTokenResponse {
            access_token: "eyJhbGciOiJIUzI1NiIsInR5cCI6IkpXVCJ9.refreshed".to_string(),
            expires_in: 3600,
        }))
    }

    async fn logout(
        &self,
        _request: Request<LogoutRequest>,
    ) -> Result<Response<LogoutResponse>, Status> {
        Ok(Response::new(LogoutResponse { success: true }))
    }
}

/// Start auth test server
pub async fn start_auth_server(
    config: TestServerConfig,
) -> Result<TestServerHandle, Box<dyn std::error::Error>> {
    let addr = format!("{}:{}", config.host, config.port + 1).parse::<SocketAddr>()?;
    let auth_service = AuthServiceImpl;

    let server = tokio::spawn(async move {
        Server::builder()
            .add_service(AuthServiceServer::new(auth_service))
            .serve(addr)
            .await
    });

    Ok(TestServerHandle {
        handle: server,
        address: addr,
    })
}
