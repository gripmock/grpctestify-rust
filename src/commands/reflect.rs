// Reflect command - list gRPC services and methods

use anyhow::{Context, Result};

use crate::cli::args::ReflectArgs;
use crate::grpc::client::{GrpcClient, GrpcClientConfig};

pub async fn handle_reflect(args: &ReflectArgs) -> Result<()> {
    // Determine address
    let address = if let Some(addr) = &args.address {
        addr.clone()
    } else {
        std::env::var("GRPCTESTIFY_ADDRESS").unwrap_or_else(|_| "localhost:4770".to_string())
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

    // Get all services from the global descriptor pool
    println!("\nAvailable services:");

    // List all services from the pool
    let mut count = 0;
    for fd in pool.file_descriptor_protos() {
        for service in &fd.service {
            if let Some(name) = &service.name {
                println!("  {}.{}", fd.name.as_ref().unwrap_or(&"".to_string()), name);
                for method in &service.method {
                    if let Some(method_name) = &method.name {
                        println!("    - {}", method_name);
                        count += 1;
                    }
                }
            }
        }
    }

    if count == 0 {
        println!("  No services found in descriptor pool");
    } else {
        println!("\nTotal: {} methods", count);
    }

    Ok(())
}
