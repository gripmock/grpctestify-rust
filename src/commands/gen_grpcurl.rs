use anyhow::{Context, Result};
use std::fs;
use std::process::Command;

use crate::cli::args::{GenArgs, GenGrpcurlArgs, GenSource};
use crate::grpc::grpcurl_invocation::{ParsedGrpcurl, parse_response_payload};
use crate::parser::GctfDocumentBuilder;

pub async fn handle_gen(args: &GenArgs) -> Result<()> {
    let rendered = match &args.source {
        GenSource::Grpcurl(grpcurl) => handle_gen_grpcurl(grpcurl).await?,
    };

    if let Some(path) = &args.output {
        fs::write(path, rendered).with_context(|| format!("Failed to write {}", path.display()))?;
    } else {
        println!("{}", rendered);
    }

    Ok(())
}

async fn handle_gen_grpcurl(args: &GenGrpcurlArgs) -> Result<String> {
    let parsed = ParsedGrpcurl::parse(&args.grpcurl_args)?;
    let mut options = parsed.options.clone();
    // Native mode defaults to plaintext when TLS is not configured.
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
        let output = Command::new("grpcurl")
            .args(&args.grpcurl_args)
            .output()
            .context("Failed to execute grpcurl")?;

        let stdout = String::from_utf8_lossy(&output.stdout).trim().to_string();
        let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();

        if output.status.success() {
            for response in parse_response_payload(&stdout) {
                builder = builder.response(response);
            }
        } else {
            let message = if stderr.is_empty() {
                "grpcurl execution failed".to_string()
            } else {
                stderr
            };
            builder = builder.error(message);
        }
    }

    Ok(builder.render())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parser::GctfDocumentBuilder;

    /// Build a rendered .gctf from grpcurl args exactly like `handle_gen_grpcurl`
    /// (without the `--execute` side effects).
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

    /// Regression for the gen-grpcurl TLS round-trip drop: the TLS section keys
    /// emitted here must be exactly the ones `build_tls_config` reads back, so a
    /// generated .gctf actually keeps its TLS configuration instead of silently
    /// running plaintext/unverified.
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
