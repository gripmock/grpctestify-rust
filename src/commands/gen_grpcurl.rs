use anyhow::{Context, Result};
use serde_json::Value;
use std::fs;
use std::path::Path;

use crate::cli::args::{GenArgs, GenGrpcurlArgs, GenSource};
use crate::execution::runner_helpers::{build_proto_config, build_tls_config, full_service_name};
use crate::grpc::grpcurl_invocation::ParsedGrpcurl;
use crate::grpc::{CompressionMode, GrpcClientConfig, TransportRef, WireProtocol};
use crate::parser::GctfDocumentBuilder;

pub async fn handle_gen(args: &GenArgs) -> Result<()> {
    let rendered = match &args.source {
        GenSource::Grpcurl(grpcurl) => handle_gen_grpcurl(grpcurl, args.output.as_deref()).await?,
    };

    if let Some(path) = &args.output {
        fs::write(path, rendered).with_context(|| format!("Failed to write {}", path.display()))?;
    } else {
        println!("{}", rendered);
    }

    Ok(())
}

async fn handle_gen_grpcurl(args: &GenGrpcurlArgs, output: Option<&Path>) -> Result<String> {
    let parsed = ParsedGrpcurl::parse(&args.grpcurl_args)?;
    let mut options = parsed.options.clone();
    options.remove("plaintext");

    let mut builder = GctfDocumentBuilder::new()
        .address(parsed.address.clone())
        .endpoint(parsed.symbol.clone())
        .options(options)
        .request_headers(parsed.headers.clone())
        .request(parsed.request_body.clone());

    if !parsed.tls.is_empty() {
        builder = builder.tls(parsed.tls.clone());
    }
    if !parsed.proto.is_empty() {
        builder = builder.proto(parsed.proto.clone());
    }

    if args.execute {
        match execute_call(&builder, &parsed, output).await {
            Ok(responses) => {
                for response in responses {
                    builder = builder.response(response);
                }
            }
            Err(message) => builder = builder.error(message),
        }
    }

    Ok(builder.render())
}

async fn execute_call(
    builder: &GctfDocumentBuilder,
    parsed: &ParsedGrpcurl,
    output: Option<&Path>,
) -> Result<Vec<Value>, String> {
    let document = builder.clone().build();
    let document_path = output.unwrap_or_else(|| Path::new("gen.gctf"));

    let (package, service, method) = document
        .parse_endpoint()
        .ok_or_else(|| format!("Invalid endpoint '{}'", parsed.symbol))?;
    let full_service = full_service_name(&package, &service);

    let timeout_seconds = match parsed.options.get("max-time") {
        Some(raw) => match raw.parse::<f64>() {
            Ok(secs) if secs > 0.0 && secs.is_finite() => secs.ceil() as u64,
            _ => return Err(format!("Invalid -max-time value '{raw}'")),
        },
        None => 30,
    };

    let config = GrpcClientConfig {
        address: parsed.address.clone(),
        timeout_seconds,
        tls_config: build_tls_config(&document, document_path),
        proto_config: build_proto_config(&document, document_path),
        metadata: (!parsed.headers.is_empty()).then(|| parsed.headers.clone()),
        target_service: Some(full_service.clone()),
        compression: if parsed.options.get("compression").map(String::as_str) == Some("gzip") {
            CompressionMode::Gzip
        } else {
            CompressionMode::default()
        },
        connection_id: 0,
        protocol: parsed
            .options
            .get("protocol")
            .and_then(|p| p.parse::<WireProtocol>().ok())
            .unwrap_or_default(),
        version: env!("CARGO_PKG_VERSION").to_string(),
    };

    let mut transport = TransportRef::new(&config)
        .await
        .map_err(|e| e.to_string())?;
    let result = transport
        .execute(
            &config,
            &full_service,
            &method,
            parsed.request_body.clone(),
            None,
        )
        .await;

    match result.error {
        Some(error) => Err(error.to_string()),
        None => Ok(result.messages),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parser::GctfDocumentBuilder;

    fn render_from_grpcurl(args: &[&str]) -> String {
        let grpcurl_args: Vec<String> = args.iter().map(|s| (*s).to_string()).collect();
        let parsed = ParsedGrpcurl::parse(&grpcurl_args).expect("parse grpcurl args");
        let mut options = parsed.options.clone();
        options.remove("plaintext");

        let mut builder = GctfDocumentBuilder::new()
            .address(parsed.address.clone())
            .endpoint(parsed.symbol.clone())
            .options(options)
            .request_headers(parsed.headers.clone())
            .request(parsed.request_body.clone());
        if !parsed.tls.is_empty() {
            builder = builder.tls(parsed.tls.clone());
        }
        if !parsed.proto.is_empty() {
            builder = builder.proto(parsed.proto.clone());
        }
        builder.render()
    }

    #[test]
    fn gen_grpcurl_tls_round_trips_through_build_tls_config() {
        let rendered = render_from_grpcurl(&[
            "-cacert",
            "/certs/ca.pem",
            "-cert",
            "/certs/client.pem",
            "-key",
            "/certs/client.key",
            "-servername",
            "api.example.com",
            "-insecure",
            "api.example.com:443",
            "svc.Service/Method",
        ]);

        let doc = crate::parser::parse_gctf_from_str(&rendered, "generated.gctf")
            .expect("parse generated gctf");
        let tls = crate::execution::runner_helpers::build_tls_config(
            &doc,
            std::path::Path::new("generated.gctf"),
        )
        .expect("TLS config must survive the round-trip");

        assert_eq!(tls.ca_cert_path.as_deref(), Some("/certs/ca.pem"));
        assert_eq!(tls.client_cert_path.as_deref(), Some("/certs/client.pem"));
        assert_eq!(tls.client_key_path.as_deref(), Some("/certs/client.key"));
        assert_eq!(tls.server_name.as_deref(), Some("api.example.com"));
        assert!(tls.insecure_skip_verify);
    }
}
