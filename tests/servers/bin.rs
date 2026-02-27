// Test server binary - starts all reference gRPC servers
// Usage: cargo run --bin test-server --features test-servers -- --port 50051

use std::env;
use tokio::signal;

mod servers;

use servers::{TestServerConfig, start_all_test_servers};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<String> = env::args().collect();

    // Parse port from arguments
    let port = args
        .iter()
        .position(|arg| arg == "--port")
        .and_then(|i| args.get(i + 1))
        .and_then(|p| p.parse::<u16>().ok())
        .unwrap_or(50051);

    let config = TestServerConfig {
        host: "127.0.0.1".to_string(),
        port,
    };

    println!("🚀 Starting test servers...");
    println!("   Echo server:       {}:{}", config.host, config.port);
    println!("   Auth server:       {}:{}", config.host, config.port + 1);
    println!("   Validation server: {}:{}", config.host, config.port + 2);
    println!();

    // Start all servers
    let handles = start_all_test_servers(config.clone()).await?;

    println!("✅ All servers started");
    println!("   Press Ctrl+C to stop");
    println!();

    // Wait for Ctrl+C
    signal::ctrl_c().await?;

    println!();
    println!("🛑 Shutting down servers...");

    // Stop all servers
    for handle in handles {
        handle.stop();
    }

    println!("✅ All servers stopped");

    Ok(())
}
