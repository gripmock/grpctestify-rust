// Scaffold command - emit a ready-to-edit .gctf skeleton for a gRPC method,
// pre-filling the REQUEST body from the request message descriptor.

use anyhow::{Context, Result, bail};
use prost_reflect::{DescriptorPool, MethodDescriptor};
use std::path::{Path, PathBuf};

use crate::cli::args::ScaffoldArgs;
use crate::config;
use crate::grpc::{GrpcClient, GrpcClientConfig, TlsConfig, WireProtocol};

/// PROTO section to embed so the generated draft can resolve descriptors at run
/// time exactly the way it was scaffolded.
struct ProtoRef {
    files: Vec<String>,
    import_paths: Vec<String>,
    descriptor: Option<String>,
}

pub async fn handle_scaffold(args: &ScaffoldArgs) -> Result<()> {
    let (service, method_name) = split_endpoint(&args.endpoint)?;

    let (pool, proto_ref) = load_descriptor_pool(args, service).await?;

    let svc = pool
        .get_service_by_name(service)
        .ok_or_else(|| anyhow::anyhow!("Service '{service}' not found in descriptors"))?;
    let method = svc
        .methods()
        .find(|m| m.name() == method_name)
        .ok_or_else(|| {
            anyhow::anyhow!("Method '{method_name}' not found in service '{service}'")
        })?;

    let address = args
        .address
        .clone()
        .unwrap_or_else(|| config::default_address_for(Some(&args.protocol)));

    let content = render_scaffold(
        &args.endpoint,
        &address,
        &args.protocol,
        proto_ref.as_ref(),
        &method,
    );

    // Never emit a broken skeleton: parse + validate before writing.
    let doc = crate::parser::parse_gctf_from_str(&content, "<scaffold>")
        .context("generated scaffold failed to parse")?;
    crate::parser::validate_document_chain(&doc).context("generated scaffold failed validation")?;

    emit(&content, args.output.as_deref(), args.force)
}

fn split_endpoint(endpoint: &str) -> Result<(&str, &str)> {
    match endpoint.split_once('/') {
        Some((svc, method)) if !svc.is_empty() && !method.is_empty() => Ok((svc, method)),
        _ => bail!("Invalid endpoint '{endpoint}'. Expected format: package.Service/Method"),
    }
}

async fn load_descriptor_pool(
    args: &ScaffoldArgs,
    service: &str,
) -> Result<(DescriptorPool, Option<ProtoRef>)> {
    match (args.reflect, &args.proto, &args.descriptor) {
        (true, _, _) => Ok((load_via_reflection(args, service).await?, None)),
        (false, Some(proto), _) => {
            let (pool, proto_ref) = load_from_proto(proto)?;
            Ok((pool, Some(proto_ref)))
        }
        (false, None, Some(descriptor)) => {
            let (pool, proto_ref) = load_from_descriptor(descriptor)?;
            Ok((pool, Some(proto_ref)))
        }
        (false, None, None) => bail!(
            "No descriptor source. Pass --proto <file|dir>, --descriptor <file>, or --reflect"
        ),
    }
}

fn load_from_proto(proto: &Path) -> Result<(DescriptorPool, ProtoRef)> {
    let (files, import_paths) = if proto.is_dir() {
        let files = collect_proto_files(proto)?;
        if files.is_empty() {
            bail!("No .proto files found in directory: {}", proto.display());
        }
        (files, vec![proto.to_path_buf()])
    } else {
        let parent = proto
            .parent()
            .filter(|p| !p.as_os_str().is_empty())
            .map(Path::to_path_buf)
            .unwrap_or_else(|| PathBuf::from("."));
        (vec![proto.to_path_buf()], vec![parent])
    };

    let fds = protox::compile(&files, &import_paths)
        .map_err(|e| anyhow::anyhow!("Failed to compile proto files: {e}"))?;
    let pool = DescriptorPool::from_file_descriptor_set(fds)
        .map_err(|e| anyhow::anyhow!("Failed to build descriptor pool: {e}"))?;

    let proto_ref = ProtoRef {
        files: files.iter().map(|p| path_to_string(p)).collect(),
        import_paths: import_paths.iter().map(|p| path_to_string(p)).collect(),
        descriptor: None,
    };
    Ok((pool, proto_ref))
}

fn load_from_descriptor(descriptor: &Path) -> Result<(DescriptorPool, ProtoRef)> {
    use prost::Message;

    let bytes = std::fs::read(descriptor)
        .with_context(|| format!("Failed to read descriptor file: {}", descriptor.display()))?;
    let set = prost_types::FileDescriptorSet::decode(bytes.as_slice())
        .with_context(|| format!("Failed to decode descriptor set: {}", descriptor.display()))?;
    let pool = DescriptorPool::from_file_descriptor_set(set)
        .map_err(|e| anyhow::anyhow!("Failed to build descriptor pool: {e}"))?;

    let proto_ref = ProtoRef {
        files: Vec::new(),
        import_paths: Vec::new(),
        descriptor: Some(path_to_string(descriptor)),
    };
    Ok((pool, proto_ref))
}

async fn load_via_reflection(args: &ScaffoldArgs, service: &str) -> Result<DescriptorPool> {
    let address = args
        .address
        .clone()
        .unwrap_or_else(|| config::default_address_for(Some(&args.protocol)));

    if args.plaintext && address.starts_with("https://") {
        bail!("--plaintext cannot be used with an https:// address");
    }

    let tls_config = if args.plaintext {
        None
    } else {
        Some(TlsConfig {
            ca_cert_path: None,
            client_cert_path: None,
            client_key_path: None,
            server_name: None,
            insecure_skip_verify: args.insecure || !address.starts_with("https://"),
        })
    };

    let config = GrpcClientConfig {
        address,
        timeout_seconds: 30,
        tls_config,
        proto_config: None,
        metadata: None,
        target_service: Some(service.to_string()),
        compression: Default::default(),
        connection_id: 0,
        protocol: args.protocol.parse::<WireProtocol>().unwrap_or_default(),
        version: env!("CARGO_PKG_VERSION").to_string(),
    };

    eprintln!("Connecting to {} for reflection...", config.address);
    let client = GrpcClient::new(config)
        .await
        .context("Failed to load descriptors via reflection")?;
    Ok(client.descriptor_pool().clone())
}

fn render_scaffold(
    endpoint: &str,
    address: &str,
    protocol: &str,
    proto_ref: Option<&ProtoRef>,
    method: &MethodDescriptor,
) -> String {
    let mut out = String::new();

    out.push_str(&format!("/// TEST: Scaffold for {endpoint}\n"));
    out.push_str("/// EXPECT: PASS\n");

    out.push_str("--- ADDRESS ---\n");
    out.push_str(address);
    out.push_str("\n\n");

    out.push_str("--- ENDPOINT ---\n");
    out.push_str(endpoint);
    out.push_str("\n\n");

    if let Some(proto) = proto_ref {
        out.push_str("--- PROTO ---\n");
        if !proto.files.is_empty() {
            out.push_str(&format!("files: {}\n", proto.files.join(", ")));
        }
        if !proto.import_paths.is_empty() {
            out.push_str(&format!(
                "import_paths: {}\n",
                proto.import_paths.join(", ")
            ));
        }
        if let Some(descriptor) = &proto.descriptor {
            out.push_str(&format!("descriptor: {descriptor}\n"));
        }
        out.push('\n');
    }

    // Protocol is a run-time (CLI) concern; record it as a hint when non-default.
    if protocol != "grpc" {
        out.push_str("--- OPTIONS ---\n");
        out.push_str(&format!(
            "# protocol: {protocol} (run with `--protocol {protocol}`)\n\n"
        ));
    }

    let request = crate::serve::api::generate_json_template(&method.input());
    let request_json = serde_json::to_string_pretty(&request).unwrap_or_else(|_| "{}".to_string());
    out.push_str("--- REQUEST ---\n");
    out.push_str(&request_json);
    out.push_str("\n\n");

    out.push_str("--- ASSERTS ---\n");
    let output = method.output();
    let field_names: Vec<String> = output.fields().map(|f| f.json_name().to_string()).collect();
    out.push_str(&format!("# Response type: {}\n", output.full_name()));
    if field_names.is_empty() {
        out.push_str("# (no response fields) — add assertions once the shape is known.\n");
    } else {
        out.push_str(&format!("# Fields: {}\n", field_names.join(", ")));
        out.push_str("# Replace the placeholder below with real expectations.\n");
        out.push_str(&format!(".{} != null\n", field_names[0]));
    }

    out
}

fn emit(content: &str, output: Option<&Path>, force: bool) -> Result<()> {
    match output {
        None => {
            print!("{content}");
            Ok(())
        }
        Some(path) => {
            if path.exists() && !force {
                bail!(
                    "Refusing to overwrite existing file: {} (use --force)",
                    path.display()
                );
            }
            write_atomic(path, content)
                .with_context(|| format!("Failed to write {}", path.display()))?;
            eprintln!("Wrote scaffold to {}", path.display());
            Ok(())
        }
    }
}

/// Write `content` to `path` atomically: temp file in the same directory then
/// rename over the target, so a crash never leaves a half-written .gctf file.
fn write_atomic(path: &Path, content: &str) -> std::io::Result<()> {
    let parent = match path.parent() {
        Some(p) if !p.as_os_str().is_empty() => p,
        _ => Path::new("."),
    };
    let file_name = path
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("out.gctf");
    let tmp_path = parent.join(format!(".{}.{}.tmp", file_name, std::process::id()));
    std::fs::write(&tmp_path, content)?;
    if let Err(e) = std::fs::rename(&tmp_path, path) {
        let _ = std::fs::remove_file(&tmp_path);
        return Err(e);
    }
    Ok(())
}

fn collect_proto_files(dir: &Path) -> Result<Vec<PathBuf>> {
    let mut files = Vec::new();
    let mut stack = vec![dir.to_path_buf()];
    while let Some(current) = stack.pop() {
        for entry in std::fs::read_dir(&current)
            .with_context(|| format!("Failed to read directory: {}", current.display()))?
        {
            let path = entry?.path();
            if path.is_dir() {
                stack.push(path);
            } else if path.extension().is_some_and(|e| e == "proto") {
                files.push(path);
            }
        }
    }
    files.sort();
    Ok(files)
}

fn path_to_string(path: &Path) -> String {
    path.to_string_lossy().to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use prost_types::field_descriptor_proto::{Label, Type};
    use prost_types::{
        DescriptorProto, EnumDescriptorProto, EnumValueDescriptorProto, FieldDescriptorProto,
        FileDescriptorProto, FileDescriptorSet, MethodDescriptorProto, ServiceDescriptorProto,
    };

    fn field(name: &str, number: i32, ty: Type, type_name: Option<&str>) -> FieldDescriptorProto {
        FieldDescriptorProto {
            name: Some(name.to_string()),
            number: Some(number),
            label: Some(Label::Optional as i32),
            r#type: Some(ty as i32),
            type_name: type_name.map(|t| t.to_string()),
            json_name: Some(name.to_string()),
            ..Default::default()
        }
    }

    /// Build an in-memory pool with a message that exercises scalars, a nested
    /// message, a repeated field, and an enum, plus a service to scaffold.
    fn sample_pool() -> DescriptorPool {
        let inner = DescriptorProto {
            name: Some("Inner".to_string()),
            field: vec![field("note", 1, Type::String, None)],
            ..Default::default()
        };
        let color = EnumDescriptorProto {
            name: Some("Color".to_string()),
            value: vec![
                EnumValueDescriptorProto {
                    name: Some("RED".to_string()),
                    number: Some(0),
                    ..Default::default()
                },
                EnumValueDescriptorProto {
                    name: Some("GREEN".to_string()),
                    number: Some(1),
                    ..Default::default()
                },
            ],
            ..Default::default()
        };
        let mut tags = field("tags", 5, Type::String, None);
        tags.label = Some(Label::Repeated as i32);

        let req = DescriptorProto {
            name: Some("Req".to_string()),
            field: vec![
                field("name", 1, Type::String, None),
                field("count", 2, Type::Int32, None),
                field("active", 3, Type::Bool, None),
                field("inner", 4, Type::Message, Some(".demo.Inner")),
                tags,
                field("color", 6, Type::Enum, Some(".demo.Color")),
            ],
            ..Default::default()
        };
        let resp = DescriptorProto {
            name: Some("Resp".to_string()),
            field: vec![field("result", 1, Type::String, None)],
            ..Default::default()
        };
        let service = ServiceDescriptorProto {
            name: Some("Svc".to_string()),
            method: vec![MethodDescriptorProto {
                name: Some("Do".to_string()),
                input_type: Some(".demo.Req".to_string()),
                output_type: Some(".demo.Resp".to_string()),
                ..Default::default()
            }],
            ..Default::default()
        };
        let file = FileDescriptorProto {
            name: Some("demo.proto".to_string()),
            package: Some("demo".to_string()),
            syntax: Some("proto3".to_string()),
            message_type: vec![inner, req, resp],
            enum_type: vec![color],
            service: vec![service],
            ..Default::default()
        };
        DescriptorPool::from_file_descriptor_set(FileDescriptorSet { file: vec![file] }).unwrap()
    }

    fn sample_method(pool: &DescriptorPool) -> MethodDescriptor {
        pool.get_service_by_name("demo.Svc")
            .unwrap()
            .methods()
            .find(|m| m.name() == "Do")
            .unwrap()
    }

    #[test]
    fn request_template_has_all_fields_with_correct_shapes() {
        let pool = sample_pool();
        let req = pool.get_message_by_name("demo.Req").unwrap();
        let tpl = crate::serve::api::generate_json_template(&req);
        let obj = tpl.as_object().expect("object");

        assert!(obj["name"].is_string());
        assert!(obj["count"].is_number());
        assert!(obj["active"].is_boolean());
        assert!(obj["inner"].is_object(), "nested message → object");
        assert!(obj["inner"]["note"].is_string());
        assert!(obj["tags"].is_array(), "repeated → array");
        assert_eq!(
            obj["tags"].as_array().unwrap().len(),
            1,
            "one example element"
        );
        // Enum → first value name.
        assert_eq!(obj["color"], serde_json::json!("RED"));
    }

    #[test]
    fn scaffold_output_parses_and_validates() {
        let pool = sample_pool();
        let method = sample_method(&pool);
        let content = render_scaffold("demo.Svc/Do", "localhost:4770", "grpc", None, &method);

        let doc =
            crate::parser::parse_gctf_from_str(&content, "<test>").expect("scaffold should parse");
        crate::parser::validate_document_chain(&doc).expect("scaffold should validate");

        assert!(content.contains("--- ENDPOINT ---"));
        assert!(content.contains("demo.Svc/Do"));
        assert!(content.contains("--- REQUEST ---"));
        assert!(content.contains("--- ASSERTS ---"));
        assert!(content.contains("demo.Resp"));
        assert!(content.contains(".result != null"));
        // Default protocol → no OPTIONS section.
        assert!(!content.contains("--- OPTIONS ---"));
    }

    #[test]
    fn scaffold_with_proto_ref_and_protocol_validates() {
        let pool = sample_pool();
        let method = sample_method(&pool);
        let proto_ref = ProtoRef {
            files: vec!["demo.proto".to_string()],
            import_paths: vec![".".to_string()],
            descriptor: None,
        };
        let content = render_scaffold(
            "demo.Svc/Do",
            "localhost:4769",
            "grpc-web",
            Some(&proto_ref),
            &method,
        );

        assert!(content.contains("--- PROTO ---"));
        assert!(content.contains("files: demo.proto"));
        assert!(content.contains("--- OPTIONS ---"));
        assert!(content.contains("protocol: grpc-web"));

        let doc =
            crate::parser::parse_gctf_from_str(&content, "<test>").expect("scaffold should parse");
        crate::parser::validate_document_chain(&doc).expect("scaffold should validate");
    }

    #[test]
    #[cfg_attr(miri, ignore)]
    fn emit_refuses_overwrite_without_force() {
        let dir = std::env::temp_dir().join(format!("scaffold_test_{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("out.gctf");
        std::fs::write(&path, "existing").unwrap();

        let err = emit("new content", Some(&path), false).unwrap_err();
        assert!(err.to_string().contains("Refusing to overwrite"));
        // Original untouched.
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "existing");

        // With --force it overwrites.
        emit("new content", Some(&path), true).unwrap();
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "new content");

        let _ = std::fs::remove_dir_all(&dir);
    }
}
