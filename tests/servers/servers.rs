// Test gRPC servers for integration testing

pub mod auth;
pub mod echo;
pub mod validation;

use std::net::SocketAddr;
use tokio::task::JoinHandle;
use tonic::transport::Server;

/// Test server configuration
#[derive(Debug, Clone)]
pub struct TestServerConfig {
    pub host: String,
    pub port: u16,
}

impl Default for TestServerConfig {
    fn default() -> Self {
        Self {
            host: "127.0.0.1".to_string(),
            port: 50051,
        }
    }
}

/// Running test server handle
pub struct TestServerHandle {
    pub handle: JoinHandle<Result<(), tonic::transport::Error>>,
    pub address: SocketAddr,
}

impl TestServerHandle {
    /// Stop the test server
    pub fn stop(self) {
        self.handle.abort();
    }
}

/// Start all test servers
pub async fn start_all_test_servers(
    config: TestServerConfig,
) -> Result<Vec<TestServerHandle>, Box<dyn std::error::Error>> {
    let mut handles = Vec::new();

    // Start echo server
    let echo_handle = echo::start_echo_server(config.clone()).await?;
    handles.push(echo_handle);

    // Start auth server
    let auth_handle = auth::start_auth_server(config.clone()).await?;
    handles.push(auth_handle);

    // Start validation server
    let validation_handle = validation::start_validation_server(config).await?;
    handles.push(validation_handle);

    Ok(handles)
}

/// Start a single test server on given port
pub async fn start_test_server(
    server_type: &str,
    port: u16,
) -> Result<TestServerHandle, Box<dyn std::error::Error>> {
    let config = TestServerConfig {
        host: "127.0.0.1".to_string(),
        port,
    };

    match server_type {
        "echo" => echo::start_echo_server(config).await,
        "auth" => auth::start_auth_server(config).await,
        "validation" => validation::start_validation_server(config).await,
        _ => Err(format!("Unknown server type: {}", server_type).into()),
    }
}
