use anyhow::Result;
use futures::stream::StreamExt;
use serde_json::Value;
use std::fs::File;
use std::io::Write;
use std::path::Path;
use std::process::ExitCode;
use std::time::Instant;
use tokio::sync::mpsc;
use tokio_stream::wrappers::ReceiverStream;

use crate::cli::args::CallArgs;
use crate::execution::runner_helpers;
use crate::grpc::{GrpcClient, GrpcClientConfig, client::StreamItem, proxy::ProxyEnv};
use crate::parser;

fn resolve_call_timeout_seconds(_connect_timeout: u64, max_time: u64) -> u64 {
    max_time
}

struct CallOptions<'a> {
    address: Option<String>,
    include_headers: bool,
    verbose: bool,
    very_verbose: bool,
    output_file: &'a mut Option<File>,
    header_file: &'a mut Option<File>,
    silent: bool,
    show_error: bool,
    connect_timeout: u64,
    max_time: u64,
    insecure: bool,
    plaintext: bool,
    tls_ca: Option<String>,
    tls_cert: Option<String>,
    tls_key: Option<String>,
    protocol: crate::grpc::WireProtocol,
}

async fn handle_call_document_inline(doc: &parser::GctfDocument, args: &CallArgs) -> Result<()> {
    let mut output_file: Option<File> = if let Some(ref path) = args.output {
        Some(File::create(path)?)
    } else {
        None
    };

    let mut header_file: Option<File> = if let Some(ref path) = args.dump_header {
        Some(File::create(path)?)
    } else {
        None
    };

    let verbose = args.verbose || args.very_verbose;

    let opts = CallOptions {
        address: args.address.clone(),
        include_headers: args.include,
        verbose,
        very_verbose: args.very_verbose,
        output_file: &mut output_file,
        header_file: &mut header_file,
        silent: args.silent,
        show_error: args.show_error,
        connect_timeout: args.connect_timeout,
        max_time: args.max_time,
        insecure: args.insecure,
        plaintext: args.plaintext,
        tls_ca: args.tls_ca.clone(),
        tls_cert: args.tls_cert.clone(),
        tls_key: args.tls_key.clone(),
        protocol: args
            .protocol
            .parse::<crate::grpc::WireProtocol>()
            .unwrap_or(crate::grpc::WireProtocol::Grpc),
    };

    handle_call_document(doc, Path::new("<inline>"), opts).await
}

fn header_pairs(flags: &[String]) -> Vec<(String, String)> {
    flags
        .iter()
        .filter_map(|h| h.split_once(':'))
        .map(|(k, v)| (k.trim().to_string(), v.trim().to_string()))
        .filter(|(k, _)| !k.is_empty())
        .collect()
}

fn http_endpoint(endpoint: &str) -> Option<(String, String)> {
    let (method, path) = endpoint.trim().split_once(' ')?;
    let (method, path) = (method.trim(), path.trim());
    if method.is_empty() || path.is_empty() || !crate::parser::ast::is_http_method(method) {
        return None;
    }
    Some((method.to_ascii_uppercase(), path.to_string()))
}

fn error_notice(args: &CallArgs, status: u16) -> Option<String> {
    if status < 400 {
        return None;
    }
    if args.show_error || !args.silent || args.fail {
        return Some(format!("* HTTP {status}: the answer is an error"));
    }
    None
}

fn dumped_headers(answer: &apif_http_transport::HttpAnswer) -> String {
    let mut out = format!("HTTP {}\n", answer.status);
    let mut names: Vec<&String> = answer
        .headers
        .keys()
        .filter(|k| !k.starts_with(':'))
        .collect();
    names.sort();
    for name in names {
        out.push_str(&format!("{name}: {}\n", answer.headers[name]));
    }
    out
}

fn flags_an_http_call_cannot_honour(args: &CallArgs) -> Vec<&'static str> {
    let mut named = Vec::new();
    if args.insecure {
        named.push("--insecure");
    }
    if args.plaintext {
        named.push("--plaintext");
    }
    if args.tls_ca.is_some() {
        named.push("--tls-ca");
    }
    if args.tls_cert.is_some() {
        named.push("--tls-cert");
    }
    if args.tls_key.is_some() {
        named.push("--tls-key");
    }
    if args.protocol != "grpc" {
        named.push("--protocol");
    }
    named
}

async fn call_http(method: &str, path: &str, args: &CallArgs) -> Result<ExitCode> {
    let refused = flags_an_http_call_cannot_honour(args);
    if !refused.is_empty() {
        anyhow::bail!(
            "{} {} for gRPC calls only: an HTTP call is secured by https:// in its address",
            refused.join(", "),
            if refused.len() == 1 { "is" } else { "are" }
        );
    }
    let dump_header = match &args.dump_header {
        Some(path) => Some(File::create(path)?),
        None => None,
    };

    let url = apif_http_transport::url_for(args.address.as_deref(), path);
    let answer = apif_http_transport::send(apif_http_transport::HttpCall {
        method: method.to_string(),
        url,
        headers: header_pairs(&args.header),
        body: args.data.clone().filter(|b| !b.trim().is_empty()),
        timeout: std::time::Duration::from_secs(resolve_call_timeout_seconds(
            args.connect_timeout,
            args.max_time,
        )),
        follow_redirects: args.location,
    })
    .await
    .map_err(|e| anyhow::anyhow!("{e}"))?;

    if args.include {
        vinfo(args.silent, &format!("HTTP {}", answer.status));
        let mut names: Vec<&String> = answer
            .headers
            .keys()
            .filter(|k| !k.starts_with(':'))
            .collect();
        names.sort();
        for name in names {
            vinfo(args.silent, &format!("{name}: {}", answer.headers[name]));
        }
    }

    if let Some(mut file) = dump_header {
        write!(file, "{}", dumped_headers(&answer))?;
    }

    let body = match &answer.body {
        Value::String(text) => text.clone(),
        other => serde_json::to_string_pretty(other)?,
    };
    match &args.output {
        Some(path) => std::fs::write(path, format!("{body}\n"))?,
        None => println!("{body}"),
    }

    if let Some(notice) = error_notice(args, answer.status) {
        eprintln!("{notice}");
    }
    if args.fail && answer.status >= 400 {
        return Ok(ExitCode::from(22));
    }
    Ok(ExitCode::SUCCESS)
}

pub async fn handle_call(args: &CallArgs) -> Result<ExitCode> {
    if args.bench {
        let bench_args = crate::cli::args::BenchArgs {
            address: args.address.clone(),
            calibrate: false,
            protocol: args.protocol.clone(),
            test_paths: if args.endpoint.is_some() {
                vec![]
            } else {
                vec![args.file.clone().unwrap_or_default()]
            },
            call: args.endpoint.clone(),
            data: args.data.clone(),
            profile: None,
            mode: None,
            concurrency: args.concurrency,
            requests: args.requests,
            duration: args.duration.clone(),
            ramp_up: None,
            warmup: None,
            max_duration: None,
            max_rps: None,
            load_schedule: None,
            load_start: None,
            load_step: None,
            load_end: None,
            load_step_duration: None,
            load_max_duration: None,
            concurrency_schedule: None,
            concurrency_start: None,
            concurrency_end: None,
            concurrency_step: None,
            concurrency_step_duration: None,
            connections: None,
            connect_timeout: None,
            request_timeout: None,
            keepalive: None,
            cpus: None,
            name: None,
            assert_mode: None,
            no_assert: false,
            sample_rate: None,
            cache: None,
            skip_first: None,
            count_errors_in_latency: None,
            duration_stop: None,
            latency_percentiles: None,
            progress_interval: None,
            format: "console".to_string(),
            output: None,
            report_template: None,
            allure_output_dir: None,
            compact: false,
            tags: vec![],
            skip_tags: vec![],
            exclude: vec![],
            list_profiles: false,
            profile_file: None,
        };
        return crate::commands::bench::handle_bench(&bench_args)
            .await
            .map(|()| ExitCode::SUCCESS);
    }

    if let Some(ref endpoint) = args.endpoint {
        if let Some((method, path)) = http_endpoint(endpoint) {
            return call_http(&method, &path, args).await;
        }
        let body = args.data.as_deref().unwrap_or("{}");
        let request_value: Value = serde_json::from_str(body)
            .map_err(|e| anyhow::anyhow!("Invalid JSON in -d/--data: {}", e))?;
        let mut builder = crate::parser::GctfDocumentBuilder::new()
            .with_file_path("<inline>")
            .endpoint(endpoint)
            .request(request_value);
        let headers = header_pairs(&args.header);
        if !headers.is_empty() {
            let mut kv = crate::parser::OrderedStringMap::default();
            for (name, value) in headers {
                kv.insert(name, value);
            }
            builder = builder.request_headers(kv);
        }
        let doc = builder.build();
        return handle_call_document_inline(&doc, args)
            .await
            .map(|()| ExitCode::SUCCESS);
    }

    if args.doc_index == Some(0) {
        anyhow::bail!("--doc-index must be >= 1");
    }

    let file_path = match &args.file {
        Some(f) => {
            let cwd = std::env::current_dir()?;
            let abs = if f.is_absolute() {
                f.clone()
            } else {
                cwd.join(f)
            };
            if !abs.exists() {
                return Err(anyhow::anyhow!("File not found: {}", abs.display()));
            }
            abs
        }
        None => anyhow::bail!("Either provide a .gctf file or use -e/--endpoint"),
    };

    let parse_result = parser::parse_with_recovery(&file_path);
    let doc = parse_result.document;

    let mut output_file: Option<File> = if let Some(ref path) = args.output {
        Some(File::create(path)?)
    } else {
        None
    };

    let mut header_file: Option<File> = if let Some(ref path) = args.dump_header {
        Some(File::create(path)?)
    } else {
        None
    };

    let verbose = args.verbose || args.very_verbose;

    let mut matched_docs = 0usize;
    for (idx, d) in doc.iter_chain().enumerate() {
        let doc_index = idx + 1;
        if let Some(selected) = args.doc_index
            && selected != doc_index
        {
            continue;
        }
        matched_docs += 1;

        let opts = CallOptions {
            address: args.address.clone(),
            include_headers: args.include,
            verbose,
            very_verbose: args.very_verbose,
            output_file: &mut output_file,
            header_file: &mut header_file,
            silent: args.silent,
            show_error: args.show_error,
            connect_timeout: args.connect_timeout,
            max_time: args.max_time,
            insecure: args.insecure,
            plaintext: args.plaintext,
            tls_ca: args.tls_ca.clone(),
            tls_cert: args.tls_cert.clone(),
            tls_key: args.tls_key.clone(),
            protocol: args
                .protocol
                .parse::<crate::grpc::WireProtocol>()
                .unwrap_or(crate::grpc::WireProtocol::Grpc),
        };

        handle_call_document(d, &file_path, opts).await?;
    }

    if matched_docs == 0 {
        if let Some(selected) = args.doc_index {
            anyhow::bail!(
                "Document index {} is out of range (total documents: {})",
                selected,
                doc.document_count()
            );
        }
        anyhow::bail!("No documents found in file");
    }

    Ok(ExitCode::SUCCESS)
}

fn vinfo(silent: bool, line: &str) {
    if !silent {
        eprintln!("* {}", line);
    }
}

fn vsend(silent: bool, line: &str) {
    if !silent {
        eprintln!("> {}", line);
    }
}

fn vrecv(silent: bool, line: &str) {
    if !silent {
        eprintln!("< {}", line);
    }
}

fn print_send_metadata(silent: bool, entries: &crate::parser::OrderedStringMap) {
    let mut pairs: Vec<_> = entries.iter().collect();
    pairs.sort_by_key(|(k, _)| k.as_str());
    for (k, v) in pairs {
        vsend(silent, &format!("{}: {}", k, v));
    }
}

fn print_recv_metadata(silent: bool, entries: &std::collections::HashMap<String, String>) {
    let mut pairs: Vec<_> = entries.iter().collect();
    pairs.sort_by_key(|(k, _)| k.as_str());
    for (k, v) in pairs {
        vrecv(silent, &format!("{}: {}", k, v));
    }
}

fn dial_address(
    doc: &parser::GctfDocument,
    flag: Option<&str>,
    protocol: crate::grpc::WireProtocol,
) -> String {
    match flag {
        Some(address) => address.to_string(),
        None => runner_helpers::effective_address(doc, Some(protocol)),
    }
}

async fn handle_call_document(
    doc: &parser::GctfDocument,
    gctf_file: &Path,
    opts: CallOptions<'_>,
) -> Result<()> {
    let (package, service, method) = match doc.parse_endpoint() {
        Some(e) => e,
        None => return Err(anyhow::anyhow!("Invalid ENDPOINT format")),
    };
    let full_service = runner_helpers::full_service_name(&package, &service);

    let address = dial_address(doc, opts.address.as_deref(), opts.protocol);
    let cli_tls_present = opts.plaintext
        || opts.tls_ca.is_some()
        || opts.tls_cert.is_some()
        || opts.tls_key.is_some();
    let mut tls_config = if cli_tls_present {
        if opts.plaintext {
            None
        } else {
            Some(crate::grpc::TlsConfig {
                ca_cert_path: opts.tls_ca.clone(),
                client_cert_path: opts.tls_cert.clone(),
                client_key_path: opts.tls_key.clone(),
                server_name: None,
                insecure_skip_verify: false,
            })
        }
    } else {
        runner_helpers::build_tls_config(doc, gctf_file)
    };
    if opts.insecure {
        tls_config = tls_config
            .map(|mut t| {
                t.insecure_skip_verify = true;
                t
            })
            .or(Some(crate::grpc::TlsConfig {
                ca_cert_path: None,
                client_cert_path: None,
                client_key_path: None,
                server_name: None,
                insecure_skip_verify: true,
            }));
    }
    let tls_label = if tls_config.is_some() {
        "TLS"
    } else {
        "plaintext"
    };

    if opts.verbose {
        let proxy = ProxyEnv::from_env();
        if let Some(v) = &proxy.no_proxy {
            vinfo(
                opts.silent,
                &format!("Uses proxy env variable NO_PROXY == '{}'", v),
            );
        }
        if let Some(v) = &proxy.https_proxy {
            vinfo(
                opts.silent,
                &format!("Uses proxy env variable HTTPS_PROXY == '{}'", v),
            );
            vinfo(
                opts.silent,
                "WARNING: gRPC transport does not support HTTP CONNECT proxies; HTTPS_PROXY is ignored",
            );
        }
        if let Some(v) = &proxy.http_proxy {
            vinfo(
                opts.silent,
                &format!("Uses proxy env variable HTTP_PROXY == '{}'", v),
            );
            vinfo(
                opts.silent,
                "WARNING: gRPC transport does not support HTTP CONNECT proxies; HTTP_PROXY is ignored",
            );
        }

        vinfo(
            opts.silent,
            &format!("Trying {} ({})...", address, tls_label),
        );
        vinfo(
            opts.silent,
            &format!("gRPC method: {}/{}", full_service, method),
        );

        if let Some(headers) = doc.get_request_headers()
            && !headers.is_empty()
        {
            vsend(opts.silent, "");
            print_send_metadata(opts.silent, &headers);
        }
        vsend(opts.silent, "");
    }

    let timeout_seconds = resolve_call_timeout_seconds(opts.connect_timeout, opts.max_time);

    let config = GrpcClientConfig {
        address: address.clone(),
        timeout_seconds,
        tls_config,
        proto_config: runner_helpers::build_proto_config(doc, gctf_file),
        metadata: doc.get_request_headers().map(|m| m.into_iter().collect()),
        target_service: Some(full_service.clone()),
        compression: Default::default(),
        connection_id: 0,
        protocol: opts.protocol,
        version: env!("CARGO_PKG_VERSION").to_string(),
    };

    let start = Instant::now();
    let mut client = GrpcClient::new(config).await?;

    let requests: Vec<Value> = doc.get_requests();
    if requests.is_empty() {
        return Err(anyhow::anyhow!("No REQUEST section found"));
    }

    if opts.very_verbose {
        let size: usize = requests
            .iter()
            .filter_map(|r| serde_json::to_vec(r).ok())
            .map(|v| v.len())
            .sum();
        vinfo(
            opts.silent,
            &format!("Estimated request size: {} bytes", size),
        );
    }

    let (tx, rx) = mpsc::channel::<Value>(100);
    let mut tx = Some(tx);
    let request_stream = ReceiverStream::new(rx);

    if let Some(tx_ref) = tx.as_mut() {
        for req in requests {
            tx_ref.send(req).await?;
        }
    }
    drop(tx.take());

    let (headers, mut response_stream) = client
        .call_stream(&full_service, &method, request_stream, None)
        .await?;

    if opts.verbose && !headers.is_empty() {
        print_recv_metadata(opts.silent, &headers);
        vrecv(opts.silent, "");
    }

    if opts.include_headers && !opts.silent && !headers.is_empty() {
        let mut pairs: Vec<_> = headers.iter().collect();
        pairs.sort_by_key(|(k, _)| k.as_str());
        for (k, v) in &pairs {
            println!("< {}: {}", k, v);
        }
        println!();
    }

    if let Some(f) = opts.header_file.as_mut() {
        let mut pairs: Vec<_> = headers.iter().collect();
        pairs.sort_by_key(|(k, _)| k.as_str());
        for (k, v) in &pairs {
            writeln!(f, "{}: {}", k, v)?;
        }
    }

    let mut msg_count = 0usize;
    while let Some(item_res) = response_stream.next().await {
        match item_res {
            Ok(StreamItem::Message(msg)) => {
                msg_count += 1;
                if opts.very_verbose {
                    let size = serde_json::to_vec(&msg).map(|v| v.len()).unwrap_or(0);
                    vinfo(
                        opts.silent,
                        &format!("Estimated response size: {} bytes", size),
                    );
                }
                let output = serde_json::to_string_pretty(&msg)?;
                if let Some(f) = opts.output_file.as_mut() {
                    writeln!(f, "{}", output)?;
                } else if !opts.silent {
                    println!("{}", output);
                }
            }
            Ok(StreamItem::Trailers(trailers)) => {
                if opts.verbose && !trailers.is_empty() {
                    print_recv_metadata(opts.silent, &trailers);
                }

                if opts.include_headers && !opts.silent && !trailers.is_empty() {
                    let mut pairs: Vec<_> = trailers.iter().collect();
                    pairs.sort_by_key(|(k, _)| k.as_str());
                    for (k, v) in &pairs {
                        println!("< {}: {}", k, v);
                    }
                }

                if let Some(f) = opts.header_file.as_mut() {
                    let mut pairs: Vec<_> = trailers.iter().collect();
                    pairs.sort_by_key(|(k, _)| k.as_str());
                    for (k, v) in &pairs {
                        writeln!(f, "{}: {}", k, v)?;
                    }
                }

                let rpc_status = trailers
                    .get("grpc-status")
                    .and_then(|s| s.parse::<u32>().ok())
                    .unwrap_or(0);

                if opts.verbose {
                    if rpc_status == 0 {
                        vinfo(opts.silent, "Connection #0 left intact");
                    } else {
                        let call_msg = trailers
                            .get("grpc-message")
                            .map(|s| s.as_str())
                            .unwrap_or("");
                        vinfo(
                            opts.silent,
                            &format!("gRPC error: code={} message={}", rpc_status, call_msg),
                        );
                    }
                }

                if opts.very_verbose {
                    let elapsed = start.elapsed();
                    vinfo(
                        opts.silent,
                        &format!(
                            "Elapsed: {}, messages received: {}",
                            crate::report::style::format_duration_ms(elapsed.as_millis() as u64),
                            msg_count
                        ),
                    );
                }
            }
            Err(err) => {
                if opts.show_error && !opts.silent {
                    eprintln!("* gRPC error: {}", err);
                }
                return Err(anyhow::anyhow!(err));
            }
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    #[test]
    fn an_endpoint_with_a_method_is_an_http_call() {
        assert_eq!(
            http_endpoint("GET /v1/users"),
            Some(("GET".to_string(), "/v1/users".to_string()))
        );
        assert_eq!(
            http_endpoint("propfind /dav/"),
            Some(("PROPFIND".to_string(), "/dav/".to_string()))
        );
        assert_eq!(http_endpoint("users.UserService/GetUser"), None);
        assert_eq!(http_endpoint("GET"), None);
    }

    fn http_call_args() -> CallArgs {
        let cli = <crate::cli::Cli as clap::Parser>::parse_from([
            "grpctestify",
            "call",
            "-e",
            "GET /v1/users",
        ]);
        match cli.command {
            Some(crate::cli::Commands::Call(args)) => args,
            _ => panic!("expected call"),
        }
    }

    async fn hopping_origin() -> String {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind");
        let origin = format!("http://{}", listener.local_addr().expect("addr"));
        tokio::spawn(async move {
            loop {
                let Ok((mut socket, _)) = listener.accept().await else {
                    break;
                };
                let mut buf = vec![0u8; 4096];
                let n = tokio::io::AsyncReadExt::read(&mut socket, &mut buf)
                    .await
                    .unwrap_or(0);
                let request = String::from_utf8_lossy(&buf[..n]).to_string();
                let response: &[u8] = if request.starts_with("GET /hop") {
                    b"HTTP/1.1 302 Found\r\nlocation: /landed\r\nx-hop: yes\r\ncontent-length: 0\r\nconnection: close\r\n\r\n"
                } else {
                    b"HTTP/1.1 200 OK\r\nx-landed: yes\r\ncontent-length: 14\r\nconnection: close\r\n\r\n{\"here\": true}"
                };
                let _ = tokio::io::AsyncWriteExt::write_all(&mut socket, response).await;
            }
        });
        origin
    }

    #[test]
    fn grpc_only_flags_are_named_when_the_target_is_http() {
        let mut args = http_call_args();
        assert!(flags_an_http_call_cannot_honour(&args).is_empty());

        args.dump_header = Some(PathBuf::from("h.txt"));
        assert!(
            flags_an_http_call_cannot_honour(&args).is_empty(),
            "-D dumps HTTP response headers too"
        );

        args.insecure = true;
        args.plaintext = true;
        args.tls_ca = Some("ca.pem".to_string());
        args.tls_cert = Some("c.pem".to_string());
        args.tls_key = Some("k.pem".to_string());
        args.protocol = "grpc-web".to_string();
        assert_eq!(
            flags_an_http_call_cannot_honour(&args),
            vec![
                "--insecure",
                "--plaintext",
                "--tls-ca",
                "--tls-cert",
                "--tls-key",
                "--protocol",
            ]
        );
    }

    #[tokio::test]
    async fn dump_header_writes_the_status_and_headers_of_an_http_answer() {
        let origin = hopping_origin().await;
        let dir = std::env::temp_dir().join(format!("grpctestify-dump-{}", std::process::id()));
        std::fs::create_dir_all(&dir).expect("temp dir");
        let dumped = dir.join("headers.txt");

        let mut args = http_call_args();
        args.address = Some(origin.clone());
        args.silent = true;
        args.output = Some(dir.join("body.json"));
        args.dump_header = Some(dumped.clone());

        let code = call_http("GET", "/hop", &args).await.expect("answered");
        assert_eq!(code, ExitCode::SUCCESS);
        let written = std::fs::read_to_string(&dumped).expect("dump written");
        assert!(written.starts_with("HTTP 302\n"), "{written}");
        assert!(written.contains("location: /landed"), "{written}");
        assert!(written.contains("x-hop: yes"), "{written}");
        assert!(
            !written.contains(":status"),
            "the pseudo-headers stay out of the dump: {written}"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[tokio::test]
    async fn location_follows_the_redirect_and_dumps_where_it_landed() {
        let origin = hopping_origin().await;
        let dir = std::env::temp_dir().join(format!("grpctestify-location-{}", std::process::id()));
        std::fs::create_dir_all(&dir).expect("temp dir");
        let dumped = dir.join("headers.txt");

        let mut args = http_call_args();
        args.address = Some(origin.clone());
        args.silent = true;
        args.output = Some(dir.join("body.json"));
        args.dump_header = Some(dumped.clone());
        args.location = true;

        let code = call_http("GET", "/hop", &args).await.expect("answered");
        assert_eq!(code, ExitCode::SUCCESS);
        let written = std::fs::read_to_string(&dumped).expect("dump written");
        assert!(written.starts_with("HTTP 200\n"), "{written}");
        assert!(written.contains("x-landed: yes"), "{written}");
        let body = std::fs::read_to_string(dir.join("body.json")).expect("body written");
        assert!(body.contains("\"here\""), "{body}");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn show_error_says_which_status_came_back_without_needing_fail() {
        let mut args = http_call_args();
        args.silent = true;

        assert_eq!(error_notice(&args, 404), None, "silence stays silent");

        args.show_error = true;
        assert_eq!(
            error_notice(&args, 404).as_deref(),
            Some("* HTTP 404: the answer is an error"),
            "-S shows the error on its own"
        );
        assert_eq!(
            error_notice(&args, 204),
            None,
            "a good answer is not an error"
        );

        args.show_error = false;
        args.fail = true;
        assert!(error_notice(&args, 500).is_some(), "-f says why it failed");

        args.fail = false;
        args.silent = false;
        assert!(error_notice(&args, 500).is_some());
    }

    #[tokio::test]
    async fn an_http_call_with_a_grpc_only_flag_is_refused_before_dialling() {
        let mut args = http_call_args();
        args.address = Some("http://127.0.0.1:1".to_string());
        args.insecure = true;
        let err = call_http("GET", "/v1/users", &args)
            .await
            .expect_err("refused");
        let message = err.to_string();
        assert!(message.contains("--insecure"), "{message}");
        assert!(message.contains("gRPC calls only"), "{message}");
    }

    #[tokio::test]
    async fn fail_turns_a_4xx_into_a_non_zero_exit_and_show_error_does_not() {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind");
        let origin = format!("http://{}", listener.local_addr().expect("addr"));
        tokio::spawn(async move {
            loop {
                let Ok((mut socket, _)) = listener.accept().await else {
                    break;
                };
                let mut buf = vec![0u8; 2048];
                let _ = tokio::io::AsyncReadExt::read(&mut socket, &mut buf).await;
                let _ = tokio::io::AsyncWriteExt::write_all(
                    &mut socket,
                    b"HTTP/1.1 404 Not Found\r\ncontent-length: 2\r\nconnection: close\r\n\r\n{}",
                )
                .await;
            }
        });

        let mut args = http_call_args();
        args.address = Some(origin.clone());
        args.silent = true;
        let code = call_http("GET", "/missing", &args).await.expect("answered");
        assert_eq!(code, ExitCode::SUCCESS);

        args.show_error = true;
        let code = call_http("GET", "/missing", &args).await.expect("answered");
        assert_eq!(code, ExitCode::SUCCESS);

        args.fail = true;
        let code = call_http("GET", "/missing", &args).await.expect("answered");
        assert_eq!(code, ExitCode::from(22));
    }

    #[test]
    fn a_header_flag_without_a_colon_names_no_header() {
        let pairs = header_pairs(&[
            "authorization: Bearer t0ken".to_string(),
            "x-empty:".to_string(),
            "nonsense".to_string(),
            ": value".to_string(),
        ]);
        assert_eq!(
            pairs,
            vec![
                ("authorization".to_string(), "Bearer t0ken".to_string()),
                ("x-empty".to_string(), String::new()),
            ]
        );
    }
    use super::{dial_address, resolve_call_timeout_seconds};

    #[test]
    fn max_time_below_connect_timeout_is_honored() {
        assert_eq!(resolve_call_timeout_seconds(30, 1), 1);
        assert_eq!(resolve_call_timeout_seconds(30, 5), 5);
    }

    #[test]
    fn the_address_flag_outranks_the_file() {
        let doc = crate::parser::GctfDocumentBuilder::new()
            .with_file_path("<test>")
            .endpoint("pkg.Svc/M")
            .address("file-host:4770")
            .build();

        assert_eq!(
            dial_address(
                &doc,
                Some("flag-host:9000"),
                crate::grpc::WireProtocol::Grpc
            ),
            "flag-host:9000"
        );
        assert_eq!(
            dial_address(&doc, None, crate::grpc::WireProtocol::Grpc),
            "file-host:4770"
        );
    }

    #[test]
    fn overall_deadline_is_max_time() {
        assert_eq!(resolve_call_timeout_seconds(30, 60), 60);
        assert_eq!(resolve_call_timeout_seconds(10, 10), 10);
        assert_eq!(resolve_call_timeout_seconds(120, 5), 5);
    }
}
