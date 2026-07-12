// Health command — checks gRPC service health using grpc.health.v1.Health/Check

use crate::cli::args::HealthArgs;
use crate::grpc::{GrpcClient, GrpcClientConfig, TlsConfig, WireProtocol};
use anyhow::Result;

pub async fn handle_health(args: &HealthArgs) -> Result<()> {
    let start = std::time::Instant::now();

    let tls_config = if args.insecure {
        Some(TlsConfig {
            ca_cert_path: None,
            client_cert_path: None,
            client_key_path: None,
            server_name: None,
            insecure_skip_verify: true,
        })
    } else {
        None
    };

    let config = GrpcClientConfig {
        address: args.address.clone(),
        timeout_seconds: args.timeout,
        tls_config,
        proto_config: None,
        metadata: None,
        target_service: Some("grpc.health.v1".to_string()),
        compression: crate::grpc::CompressionMode::None,
        connection_id: 0,
        protocol: args
            .protocol
            .parse::<WireProtocol>()
            .unwrap_or(crate::grpc::WireProtocol::Grpc),
    };

    let mut client = GrpcClient::new(config).await?;
    let service_name = if args.service.is_empty() {
        ""
    } else {
        &args.service
    };

    let request = serde_json::json!({"service": service_name});
    let response = client
        .call("grpc.health.v1.Health", "Check", vec![request])
        .await?;

    let elapsed = start.elapsed().as_millis() as u64;
    let status = response
        .messages
        .first()
        .and_then(|m| m.get("status").and_then(|s| s.as_str()))
        .unwrap_or("UNKNOWN");

    match args.format.as_str() {
        "json" => {
            let output = serde_json::json!({
                "status": status,
                "duration_ms": elapsed,
                "service": service_name,
            });
            println!("{}", serde_json::to_string_pretty(&output)?);
        }
        _ => {
            let icon = match status {
                "SERVING" => "✓",
                _ => "✗",
            };
            println!(
                "{} Service: {} ({}) [{} ms]",
                icon, status, service_name, elapsed
            );
        }
    }

    if status != "SERVING" {
        std::process::exit(1);
    }

    Ok(())
}
