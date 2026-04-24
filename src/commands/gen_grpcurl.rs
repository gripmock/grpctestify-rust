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
