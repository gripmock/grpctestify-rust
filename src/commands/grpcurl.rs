use anyhow::Result;
use serde::Serialize;
use std::path::Path;

use crate::cli::args::{GrpcurlArgs, HasFormat};
use crate::execution::runner_helpers;
use crate::grpc::CompressionMode;
use crate::parser;

#[derive(Debug, Clone, Serialize)]
struct GrpcurlOutput {
    file: String,
    doc_index: usize,
    address: String,
    endpoint: String,
    request_index: usize,
    command: String,
}

fn shell_quote(value: &str) -> String {
    if value.is_empty() {
        return "''".to_string();
    }
    let escaped = value.replace('\'', "'\\''");
    format!("'{}'", escaped)
}

fn grpcurl_address_parts(raw: &str) -> (String, bool) {
    if let Some(rest) = raw.strip_prefix("https://") {
        return (rest.to_string(), false);
    }
    if let Some(rest) = raw.strip_prefix("http://") {
        return (rest.to_string(), true);
    }
    (raw.to_string(), true)
}

fn path_for_invocation(resolved: &Path, cwd: &Path) -> String {
    let normalize = |p: &Path| p.to_string_lossy().replace('\\', "/");

    if let Ok(relative) = resolved.strip_prefix(cwd)
        && !relative.as_os_str().is_empty()
    {
        return normalize(relative);
    }
    normalize(resolved)
}

fn build_grpcurl_command(
    doc: &parser::GctfDocument,
    gctf_file: &Path,
    cwd: &Path,
    doc_index: usize,
    request_index: usize,
) -> Result<GrpcurlOutput> {
    let endpoint = doc
        .get_endpoint()
        .ok_or_else(|| anyhow::anyhow!("Missing ENDPOINT section"))?;

    let address_raw = runner_helpers::effective_address(doc);
    let (address, plaintext_from_address) = grpcurl_address_parts(&address_raw);

    let tls_config = runner_helpers::build_tls_config(doc, gctf_file);
    let plaintext = plaintext_from_address && tls_config.is_none();

    let mut parts = vec!["grpcurl".to_string()];
    if plaintext {
        parts.push("-plaintext".to_string());
    }

    let options = doc.get_options().unwrap_or_default();
    if runner_helpers::parse_compression_option(&options) == CompressionMode::Gzip {
        parts.push("-gzip".to_string());
    }

    if let Some(tls) = tls_config {
        if let Some(ca_cert) = tls.ca_cert_path {
            parts.push("-cacert".to_string());
            parts.push(shell_quote(&path_for_invocation(Path::new(&ca_cert), cwd)));
        }
        if let Some(cert) = tls.client_cert_path {
            parts.push("-cert".to_string());
            parts.push(shell_quote(&path_for_invocation(Path::new(&cert), cwd)));
        }
        if let Some(key) = tls.client_key_path {
            parts.push("-key".to_string());
            parts.push(shell_quote(&path_for_invocation(Path::new(&key), cwd)));
        }
        if let Some(server_name) = tls.server_name {
            parts.push("-servername".to_string());
            parts.push(shell_quote(&server_name));
        }
        if tls.insecure_skip_verify {
            parts.push("-insecure".to_string());
        }
    }

    if let Some(proto) = runner_helpers::build_proto_config(doc, gctf_file) {
        if let Some(descriptor) = proto.descriptor {
            parts.push("-protoset".to_string());
            parts.push(shell_quote(&path_for_invocation(
                Path::new(&descriptor),
                cwd,
            )));
        } else {
            for import_path in proto.import_paths {
                parts.push("-import-path".to_string());
                parts.push(shell_quote(&path_for_invocation(
                    Path::new(&import_path),
                    cwd,
                )));
            }

            for proto_file in proto.files {
                parts.push("-proto".to_string());
                parts.push(shell_quote(&path_for_invocation(
                    Path::new(&proto_file),
                    cwd,
                )));
            }
        }
    }

    if let Some(headers) = doc.get_request_headers() {
        let mut sorted: Vec<_> = headers.into_iter().collect();
        sorted.sort_by(|a, b| a.0.cmp(&b.0));
        for (k, v) in sorted {
            parts.push("-H".to_string());
            parts.push(shell_quote(&format!("{}: {}", k, v)));
        }
    }

    let requests = doc.get_requests();
    if requests.is_empty() {
        parts.push("-d".to_string());
        parts.push(shell_quote("{}"));
    } else {
        let idx = request_index.saturating_sub(1);
        let chosen = requests.get(idx).or_else(|| requests.last());
        if let Some(body) = chosen {
            let json = serde_json::to_string(body)?;
            parts.push("-d".to_string());
            parts.push(shell_quote(&json));
        }
    }

    parts.push(shell_quote(&address));
    parts.push(shell_quote(&endpoint));

    Ok(GrpcurlOutput {
        file: gctf_file.to_string_lossy().to_string(),
        doc_index,
        address,
        endpoint,
        request_index,
        command: parts.join(" "),
    })
}

pub async fn handle_grpcurl(args: &GrpcurlArgs) -> Result<()> {
    if args.request_index == 0 {
        anyhow::bail!("--request-index must be >= 1");
    }
    if args.doc_index == Some(0) {
        anyhow::bail!("--doc-index must be >= 1");
    }

    let cwd = std::env::current_dir()?;
    let file_path = if args.file.is_absolute() {
        args.file.clone()
    } else {
        cwd.join(&args.file)
    };

    if !file_path.exists() {
        return Err(anyhow::anyhow!("File not found: {}", file_path.display()));
    }

    let parse_result = parser::parse_with_recovery(&file_path);
    let doc = parse_result.document;

    let mut outputs = Vec::new();
    for (idx, d) in doc.iter_chain().enumerate() {
        let doc_index = idx + 1;
        if let Some(selected) = args.doc_index
            && selected != doc_index
        {
            continue;
        }

        outputs.push(build_grpcurl_command(
            d,
            &file_path,
            &cwd,
            doc_index,
            args.request_index,
        )?);
    }

    if outputs.is_empty() {
        if let Some(selected) = args.doc_index {
            anyhow::bail!(
                "Document index {} is out of range (total documents: {})",
                selected,
                doc.document_count()
            );
        }
        anyhow::bail!("No documents found in file");
    }

    if args.is_json() {
        if outputs.len() == 1 {
            println!("{}", serde_json::to_string_pretty(&outputs[0])?);
        } else {
            println!("{}", serde_json::to_string_pretty(&outputs)?);
        }
    } else if outputs.len() == 1 {
        println!("{}", outputs[0].command);
    } else {
        for (i, out) in outputs.iter().enumerate() {
            println!("# Document {}", i + 1);
            println!("{}", out.command);
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_grpcurl_address_parts() {
        assert_eq!(
            grpcurl_address_parts("https://svc:443"),
            ("svc:443".to_string(), false)
        );
        assert_eq!(
            grpcurl_address_parts("http://svc:80"),
            ("svc:80".to_string(), true)
        );
        assert_eq!(
            grpcurl_address_parts("localhost:50051"),
            ("localhost:50051".to_string(), true)
        );
    }

    #[test]
    fn test_shell_quote() {
        assert_eq!(shell_quote("a b"), "'a b'");
        assert_eq!(shell_quote("a'b"), "'a'\\''b'");
    }
}
