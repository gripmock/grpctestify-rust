// Reflect command - list gRPC services and methods

use anyhow::{Context, Result};
use std::collections::BTreeMap;

use crate::cli::args::ReflectArgs;
use crate::config;
use crate::grpc::client::{GrpcClient, GrpcClientConfig};
use crate::grpc::tls::{TlsConfig, WireProtocol};

pub async fn handle_reflect(args: &ReflectArgs) -> Result<()> {
    let address = if let Some(addr) = &args.address {
        addr.clone()
    } else {
        std::env::var(config::ENV_GRPCTESTIFY_ADDRESS).unwrap_or_else(|_| config::default_address())
    };

    if args.plaintext && address.starts_with("https://") {
        anyhow::bail!("--plaintext cannot be used with an https:// address");
    }

    let tls_config = if args.plaintext {
        None
    } else {
        Some(TlsConfig {
            ca_cert_path: args.tls_ca.clone(),
            client_cert_path: args.tls_cert.clone(),
            client_key_path: args.tls_key.clone(),
            server_name: None,
            insecure_skip_verify: !args.plaintext && !address.starts_with("https://"),
        })
    };

    let config = GrpcClientConfig {
        address,
        timeout_seconds: 30,
        tls_config,
        proto_config: None,
        metadata: None,
        target_service: None,
        compression: Default::default(),
        connection_id: 0,
        protocol: args.protocol.parse::<WireProtocol>().unwrap_or(crate::grpc::WireProtocol::Grpc),
        user_agent: None,
    };

    eprintln!("Connecting to {}...", config.address);

    let client = GrpcClient::new(config)
        .await
        .context("Failed to connect to gRPC server")?;

    let pool = client.descriptor_pool();

    // --describe: show full message fields for a method
    if let Some(ref desc) = args.describe {
        let parts: Vec<&str> = desc.split('/').collect();
        if parts.len() != 2 {
            anyhow::bail!("Use format: Service/Method");
        }
        let svc = pool
            .get_service_by_name(parts[0])
            .ok_or_else(|| anyhow::anyhow!("Service not found: {}", parts[0]))?;
        let method = svc
            .methods()
            .find(|m| m.name() == parts[1])
            .ok_or_else(|| anyhow::anyhow!("Method not found: {}", parts[1]))?;

        let is_cs = method.is_client_streaming();
        let is_ss = method.is_server_streaming();
        let mode = match (is_cs, is_ss) {
            (false, false) => "unary",
            (false, true) => "server streaming",
            (true, false) => "client streaming",
            (true, true) => "bidirectional streaming",
        };

        if args.format == "json" {
            let output = serde_json::json!({
                "service": parts[0],
                "method": parts[1],
                "rpc_mode": mode,
                "input_type": method.input().full_name(),
                "output_type": method.output().full_name(),
            });
            println!("{}", serde_json::to_string_pretty(&output)?);
        } else {
            println!("\nMethod: {}/{}", parts[0], parts[1]);
            println!("  Mode:        {}", mode);
            println!("  Input:       {}", method.input().full_name());
            println!("  Output:      {}", method.output().full_name());
        }
        return Ok(());
    }

    // --list-methods: show all methods with full signatures
    if args.list_methods {
        let mut all = BTreeMap::new();
        for service in pool.services() {
            for method in service.methods() {
                let is_cs = method.is_client_streaming();
                let is_ss = method.is_server_streaming();
                let mode = match (is_cs, is_ss) {
                    (false, false) => "unary",
                    (false, true) => "server_stream",
                    (true, false) => "client_stream",
                    (true, true) => "bidi",
                };
                let key = format!("{}/{}", service.full_name(), method.name());
                let entry = serde_json::json!({
                    "service": service.full_name(),
                    "method": method.name(),
                    "rpc_mode": mode,
                    "input": method.input().full_name(),
                    "output": method.output().full_name(),
                });
                all.insert(key, entry);
            }
        }

        if args.format == "json" {
            let output: Vec<_> = all.values().collect();
            println!("{}", serde_json::to_string_pretty(&output)?);
        } else {
            for (key, entry) in &all {
                println!("{}", key);
                println!("  mode:   {}", entry["rpc_mode"].as_str().unwrap_or(""));
                println!("  input:  {}", entry["input"].as_str().unwrap_or(""));
                println!("  output: {}", entry["output"].as_str().unwrap_or(""));
                println!();
            }
        }
        return Ok(());
    }

    // --symbol: describe a specific symbol
    if let Some(symbol) = args.symbol.as_deref() {
        let output = client.describe(Some(symbol))?;
        println!("\n{}", output);
        return Ok(());
    }

    // Default: list services
    if args.format == "json" {
        let services: Vec<_> = pool.services().map(|s| s.full_name().to_string()).collect();
        println!("{}", serde_json::to_string_pretty(&services)?);
    } else {
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
            println!("  No services found");
        } else {
            println!("\nTotal: {} methods", count);
        }
    }

    Ok(())
}
