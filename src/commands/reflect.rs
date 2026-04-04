// Reflect command - list gRPC services and methods

use anyhow::{Context, Result};

use crate::cli::args::ReflectArgs;
use crate::config;
use crate::grpc::client::{GrpcClient, GrpcClientConfig};

pub async fn handle_reflect(args: &ReflectArgs) -> Result<()> {
    // Determine address
    let address = if let Some(addr) = &args.address {
        addr.clone()
    } else {
        std::env::var(config::ENV_GRPCTESTIFY_ADDRESS).unwrap_or_else(|_| config::default_address())
    };

    // Build client config
    let config = GrpcClientConfig {
        address,
        timeout_seconds: 30,
        tls_config: None,
        proto_config: None,
        metadata: None,
        target_service: None,
        compression: Default::default(),
    };

    println!("Connecting to {}...", config.address);

    let client = GrpcClient::new(config)
        .await
        .context("Failed to connect to gRPC server")?;

    let pool = client.descriptor_pool();

    println!("\nAvailable services:");

    let mut count = 0;
    for service in pool.services() {
        println!("  {}", service.full_name());
        for method in service.methods() {
            println!("    - {}", method.name());
            count += 1;
        }
    }

    if count == 0 {
        println!("  No services found in descriptor pool");
    } else {
        println!("\nTotal: {} methods", count);
    }

    Ok(())
}
