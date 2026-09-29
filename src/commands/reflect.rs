use anyhow::{Context, Result};
use std::collections::BTreeMap;

use crate::cli::args::ReflectArgs;
use crate::config;
use crate::grpc::client::{GrpcClient, GrpcClientConfig};
use crate::grpc::{TlsConfig, WireProtocol};

fn resolve_tls_config(
    plaintext: bool,
    insecure: bool,
    tls_ca: Option<String>,
    tls_cert: Option<String>,
    tls_key: Option<String>,
    address: &str,
) -> Result<Option<TlsConfig>> {
    if plaintext && address.starts_with("https://") {
        anyhow::bail!("--plaintext cannot be used with an https:// address");
    }

    Ok(if plaintext {
        None
    } else {
        Some(super::tls_config_from_flags(
            tls_ca,
            tls_cert,
            tls_key,
            insecure || !address.starts_with("https://"),
        ))
    })
}

const MAX_DESCRIBE_DEPTH: usize = 8;

pub(crate) fn field_type_label(field: &prost_reflect::FieldDescriptor) -> String {
    use prost_reflect::Kind;
    if field.is_map() {
        let Kind::Message(entry) = field.kind() else {
            return "map".to_string();
        };
        let key = entry.map_entry_key_field();
        let value = entry.map_entry_value_field();
        return format!(
            "map<{}, {}>",
            field_type_label(&key),
            field_type_label(&value)
        );
    }
    match field.kind() {
        Kind::Double => "double".to_string(),
        Kind::Float => "float".to_string(),
        Kind::Int32 | Kind::Sint32 | Kind::Sfixed32 => "int32".to_string(),
        Kind::Int64 | Kind::Sint64 | Kind::Sfixed64 => "int64".to_string(),
        Kind::Uint32 | Kind::Fixed32 => "uint32".to_string(),
        Kind::Uint64 | Kind::Fixed64 => "uint64".to_string(),
        Kind::Bool => "bool".to_string(),
        Kind::String => "string".to_string(),
        Kind::Bytes => "bytes".to_string(),
        Kind::Message(m) => m.name().to_string(),
        Kind::Enum(e) => e.name().to_string(),
    }
}

pub(crate) fn describe_message_tree(
    desc: &prost_reflect::MessageDescriptor,
    indent: usize,
    visiting: &mut Vec<String>,
    out: &mut String,
) {
    use crate::report::style::dim_style;
    use prost_reflect::Kind;

    let pad = "  ".repeat(indent);
    for field in desc.fields() {
        let label = field_type_label(&field);
        let suffix = if field.is_map() {
            String::new()
        } else if field.is_list() {
            "[]".to_string()
        } else {
            String::new()
        };
        out.push_str(&format!(
            "{pad}{} {}{suffix}\n",
            field.json_name(),
            dim_style().apply_to(&label)
        ));

        match field.kind() {
            Kind::Enum(e) => {
                let values: Vec<String> = e.values().map(|v| v.name().to_string()).collect();
                out.push_str(&format!(
                    "{pad}  {} {}\n",
                    dim_style().apply_to("values:"),
                    values.join(", ")
                ));
            }
            Kind::Message(m) if !field.is_map() => {
                let full_name = m.full_name().to_string();
                if indent >= MAX_DESCRIBE_DEPTH || visiting.contains(&full_name) {
                    out.push_str(&format!(
                        "{pad}  {}\n",
                        dim_style().apply_to("... (max depth or recursive type)")
                    ));
                } else {
                    visiting.push(full_name);
                    describe_message_tree(&m, indent + 1, visiting, out);
                    visiting.pop();
                }
            }
            _ => {}
        }
    }
}

fn describe_message_schema(
    desc: &prost_reflect::MessageDescriptor,
    depth: usize,
    visiting: &mut Vec<String>,
) -> serde_json::Value {
    let mut props = serde_json::Map::new();
    for field in desc.fields() {
        props.insert(
            field.json_name().to_string(),
            field_schema(&field, depth, visiting),
        );
    }
    serde_json::json!({ "type": "object", "properties": props })
}

fn field_schema(
    field: &prost_reflect::FieldDescriptor,
    depth: usize,
    visiting: &mut Vec<String>,
) -> serde_json::Value {
    use prost_reflect::Kind;

    if field.is_map() {
        let Kind::Message(entry) = field.kind() else {
            return serde_json::json!({ "type": "object" });
        };
        let value_field = entry.map_entry_value_field();
        return serde_json::json!({
            "type": "object",
            "additionalProperties": field_schema(&value_field, depth, visiting),
        });
    }

    let base = match field.kind() {
        Kind::Double | Kind::Float => serde_json::json!({ "type": "number" }),
        Kind::Int32
        | Kind::Sint32
        | Kind::Sfixed32
        | Kind::Uint32
        | Kind::Fixed32
        | Kind::Int64
        | Kind::Sint64
        | Kind::Sfixed64
        | Kind::Uint64
        | Kind::Fixed64 => serde_json::json!({ "type": "integer" }),
        Kind::Bool => serde_json::json!({ "type": "boolean" }),
        Kind::String => serde_json::json!({ "type": "string" }),
        Kind::Bytes => serde_json::json!({ "type": "string", "format": "base64" }),
        Kind::Enum(e) => {
            let values: Vec<String> = e.values().map(|v| v.name().to_string()).collect();
            serde_json::json!({ "type": "string", "enum": values })
        }
        Kind::Message(m) => {
            let full_name = m.full_name().to_string();
            if depth == 0 || visiting.contains(&full_name) {
                serde_json::json!({ "type": "object", "description": format!("{full_name} (max depth or recursive type)") })
            } else {
                visiting.push(full_name);
                let schema = describe_message_schema(&m, depth - 1, visiting);
                visiting.pop();
                schema
            }
        }
    };

    if field.is_list() {
        serde_json::json!({ "type": "array", "items": base })
    } else {
        base
    }
}

fn matches_filter(filter: Option<&str>, service_name: &str, method_name: &str) -> bool {
    let Some(f) = filter.filter(|f| !f.is_empty()) else {
        return true;
    };
    let f = f.to_lowercase();
    service_name.to_lowercase().contains(&f) || method_name.to_lowercase().contains(&f)
}

pub async fn handle_reflect(args: &ReflectArgs) -> Result<()> {
    let address = if let Some(addr) = &args.address {
        addr.clone()
    } else {
        std::env::var(config::ENV_GRPCTESTIFY_ADDRESS).unwrap_or_else(|_| config::default_address())
    };

    let tls_config = resolve_tls_config(
        args.plaintext,
        args.insecure,
        args.tls_ca.clone(),
        args.tls_cert.clone(),
        args.tls_key.clone(),
        &address,
    )?;

    let config = GrpcClientConfig {
        address,
        timeout_seconds: 30,
        tls_config,
        proto_config: None,
        metadata: None,
        target_service: None,
        compression: Default::default(),
        connection_id: 0,
        protocol: args
            .protocol
            .parse::<WireProtocol>()
            .unwrap_or(crate::grpc::WireProtocol::Grpc),
        version: env!("CARGO_PKG_VERSION").to_string(),
    };

    let display_address = config.address.clone();
    eprintln!("Connecting to {}...", display_address);

    let client = GrpcClient::new(config)
        .await
        .context("Failed to connect to gRPC server")?;

    let pool = client.descriptor_pool();

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
            let mut input_visiting = vec![method.input().full_name().to_string()];
            let mut output_visiting = vec![method.output().full_name().to_string()];
            let output = serde_json::json!({
                "service": parts[0],
                "method": parts[1],
                "rpc_mode": mode,
                "input_type": method.input().full_name(),
                "output_type": method.output().full_name(),
                "input_schema": describe_message_schema(&method.input(), MAX_DESCRIBE_DEPTH, &mut input_visiting),
                "output_schema": describe_message_schema(&method.output(), MAX_DESCRIBE_DEPTH, &mut output_visiting),
            });
            println!("{}", serde_json::to_string_pretty(&output)?);
        } else {
            use crate::report::style::{bold_style, dim_style};
            println!();
            println!(
                "🔎 {}",
                bold_style().apply_to(format!("{}/{}", parts[0], parts[1]))
            );
            println!("   {}  {}", dim_style().apply_to("Mode  "), mode);
            println!(
                "   {}  {}",
                dim_style().apply_to("Input "),
                method.input().full_name()
            );
            let mut input_tree = String::new();
            let mut input_visiting = vec![method.input().full_name().to_string()];
            describe_message_tree(&method.input(), 2, &mut input_visiting, &mut input_tree);
            print!("{input_tree}");
            println!(
                "   {}  {}",
                dim_style().apply_to("Output"),
                method.output().full_name()
            );
            let mut output_tree = String::new();
            let mut output_visiting = vec![method.output().full_name().to_string()];
            describe_message_tree(&method.output(), 2, &mut output_visiting, &mut output_tree);
            print!("{output_tree}");
        }
        return Ok(());
    }

    if args.list_methods {
        let mut all = BTreeMap::new();
        for service in pool.services() {
            for method in service.methods() {
                if !matches_filter(args.filter.as_deref(), service.full_name(), method.name()) {
                    continue;
                }
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
            use crate::report::style::{bold_style, dim_style};
            if all.is_empty() {
                let msg = if args.filter.is_some() {
                    "No methods match the given filter"
                } else {
                    "No methods found"
                };
                println!("  {}", dim_style().apply_to(msg));
            }
            for (key, entry) in &all {
                println!("  {}", bold_style().apply_to(key));
                println!(
                    "     {} {}",
                    dim_style().apply_to("• mode  "),
                    entry["rpc_mode"].as_str().unwrap_or("")
                );
                println!(
                    "     {} {}",
                    dim_style().apply_to("• input "),
                    entry["input"].as_str().unwrap_or("")
                );
                println!(
                    "     {} {}",
                    dim_style().apply_to("• output"),
                    entry["output"].as_str().unwrap_or("")
                );
                println!();
            }
        }
        return Ok(());
    }

    if let Some(symbol) = args.symbol.as_deref() {
        let output = client.describe(Some(symbol))?;
        println!("\n{}", output);
        return Ok(());
    }

    let filter = args.filter.as_deref();
    if args.format == "json" {
        let services: Vec<_> = pool
            .services()
            .filter(|s| {
                matches_filter(filter, s.full_name(), "")
                    || s.methods()
                        .any(|m| matches_filter(filter, s.full_name(), m.name()))
            })
            .map(|s| s.full_name().to_string())
            .collect();
        println!("{}", serde_json::to_string_pretty(&services)?);
    } else {
        use crate::report::style::{bold_style, dim_style};
        println!();
        println!(
            "📦 {} {}",
            bold_style().apply_to("Services"),
            dim_style().apply_to(format!("· {}", display_address))
        );
        println!();
        let mut count = 0;
        for service in pool.services() {
            let service_matches = matches_filter(filter, service.full_name(), "");
            let methods: Vec<_> = service
                .methods()
                .filter(|m| {
                    service_matches || matches_filter(filter, service.full_name(), m.name())
                })
                .collect();
            if !service_matches && methods.is_empty() {
                continue;
            }
            println!("  {}", bold_style().apply_to(service.full_name()));
            for method in &methods {
                println!("    {} {}", dim_style().apply_to("•"), method.name());
                count += 1;
            }
        }
        if count == 0 {
            let msg = if filter.is_some() {
                "No services/methods match the given filter"
            } else {
                "No services found"
            };
            println!("  {}", dim_style().apply_to(msg));
        } else {
            println!();
            println!("📊 Total: {} methods", count);
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bare_address_defaults_to_skip_verify() {
        let cfg = resolve_tls_config(false, false, None, None, None, "localhost:4770")
            .unwrap()
            .unwrap();
        assert!(cfg.insecure_skip_verify);
    }

    #[test]
    fn https_address_defaults_to_verified() {
        let cfg = resolve_tls_config(false, false, None, None, None, "https://localhost:4770")
            .unwrap()
            .unwrap();
        assert!(!cfg.insecure_skip_verify);
    }

    #[test]
    fn insecure_forces_skip_verify_even_for_https_address() {
        let cfg = resolve_tls_config(false, true, None, None, None, "https://localhost:4770")
            .unwrap()
            .unwrap();
        assert!(
            cfg.insecure_skip_verify,
            "--insecure must override the https:// default"
        );
    }

    #[test]
    fn plaintext_yields_no_tls_config() {
        assert!(
            resolve_tls_config(true, false, None, None, None, "localhost:4770")
                .unwrap()
                .is_none()
        );
    }

    #[test]
    fn plaintext_with_https_address_is_rejected() {
        let err = resolve_tls_config(true, false, None, None, None, "https://localhost:4770")
            .unwrap_err();
        assert!(err.to_string().contains("--plaintext"));
    }

    fn health_response_message() -> prost_reflect::MessageDescriptor {
        let pool =
            prost_reflect::DescriptorPool::decode(tonic_health::pb::FILE_DESCRIPTOR_SET).unwrap();
        pool.get_message_by_name("grpc.health.v1.HealthCheckResponse")
            .unwrap()
    }

    #[test]
    fn describe_message_schema_marks_enum_field_with_its_values() {
        let msg = health_response_message();
        let mut visiting = vec![msg.full_name().to_string()];
        let schema = describe_message_schema(&msg, MAX_DESCRIBE_DEPTH, &mut visiting);
        let status_enum = schema["properties"]["status"]["enum"]
            .as_array()
            .expect("status must be an enum schema");
        let values: Vec<&str> = status_enum.iter().filter_map(|v| v.as_str()).collect();
        assert_eq!(
            values,
            ["UNKNOWN", "SERVING", "NOT_SERVING", "SERVICE_UNKNOWN"]
        );
    }

    #[test]
    fn describe_message_tree_lists_enum_values_in_text_form() {
        let msg = health_response_message();
        let mut visiting = vec![msg.full_name().to_string()];
        let mut out = String::new();
        describe_message_tree(&msg, 0, &mut visiting, &mut out);
        assert!(out.contains("status"));
        assert!(out.contains("ServingStatus"));
        assert!(out.contains("SERVING"));
    }

    fn self_referential_node_message() -> prost_reflect::MessageDescriptor {
        use prost_types::field_descriptor_proto::{Label, Type};
        use prost_types::{DescriptorProto, FieldDescriptorProto, FileDescriptorProto};

        let file = FileDescriptorProto {
            name: Some("test_node.proto".to_string()),
            package: Some("test".to_string()),
            syntax: Some("proto3".to_string()),
            message_type: vec![DescriptorProto {
                name: Some("Node".to_string()),
                field: vec![
                    FieldDescriptorProto {
                        name: Some("name".to_string()),
                        json_name: Some("name".to_string()),
                        number: Some(1),
                        label: Some(Label::Optional as i32),
                        r#type: Some(Type::String as i32),
                        ..Default::default()
                    },
                    FieldDescriptorProto {
                        name: Some("children".to_string()),
                        json_name: Some("children".to_string()),
                        number: Some(2),
                        label: Some(Label::Repeated as i32),
                        r#type: Some(Type::Message as i32),
                        type_name: Some(".test.Node".to_string()),
                        ..Default::default()
                    },
                ],
                ..Default::default()
            }],
            ..Default::default()
        };

        let pool = prost_reflect::DescriptorPool::from_file_descriptor_set(
            prost_types::FileDescriptorSet { file: vec![file] },
        )
        .unwrap();
        pool.get_message_by_name("test.Node").unwrap()
    }

    #[test]
    fn describe_message_schema_stops_at_a_cycle_instead_of_recursing_forever() {
        let msg = self_referential_node_message();
        let mut visiting = vec![msg.full_name().to_string()];
        let schema = describe_message_schema(&msg, MAX_DESCRIBE_DEPTH, &mut visiting);
        assert_eq!(schema["type"], "object");
        let children_items = &schema["properties"]["children"]["items"];
        assert!(
            children_items.get("properties").is_none(),
            "cycle guard failed to stop recursion: {children_items}"
        );
    }

    fn depth_chain_message() -> prost_reflect::MessageDescriptor {
        use prost_types::field_descriptor_proto::{Label, Type};
        use prost_types::{DescriptorProto, FieldDescriptorProto, FileDescriptorProto};

        let message = |name: &str, next_type: Option<&str>| DescriptorProto {
            name: Some(name.to_string()),
            field: next_type
                .map(|t| {
                    vec![FieldDescriptorProto {
                        name: Some("next".to_string()),
                        json_name: Some("next".to_string()),
                        number: Some(1),
                        label: Some(Label::Optional as i32),
                        r#type: Some(Type::Message as i32),
                        type_name: Some(t.to_string()),
                        ..Default::default()
                    }]
                })
                .unwrap_or_default(),
            ..Default::default()
        };

        let file = FileDescriptorProto {
            name: Some("test_chain.proto".to_string()),
            package: Some("test".to_string()),
            syntax: Some("proto3".to_string()),
            message_type: vec![
                message("Chain1", None),
                message("Chain2", Some(".test.Chain1")),
                message("Chain3", Some(".test.Chain2")),
            ],
            ..Default::default()
        };

        let pool = prost_reflect::DescriptorPool::from_file_descriptor_set(
            prost_types::FileDescriptorSet { file: vec![file] },
        )
        .unwrap();
        pool.get_message_by_name("test.Chain3").unwrap()
    }

    #[test]
    fn describe_message_schema_stops_at_max_depth() {
        let msg = depth_chain_message();
        let mut visiting = Vec::new();
        let schema = describe_message_schema(&msg, 1, &mut visiting);
        let chain2 = &schema["properties"]["next"];
        assert!(
            chain2["properties"].is_object(),
            "Chain2 (depth 1) should have expanded: {chain2}"
        );
        let chain1 = &chain2["properties"]["next"];
        assert!(
            chain1.get("properties").is_none(),
            "depth cap failed to stop expansion into Chain1: {chain1}"
        );
    }

    #[test]
    fn describe_message_tree_stops_at_a_cycle_instead_of_recursing_forever() {
        let msg = self_referential_node_message();
        let mut visiting = vec![msg.full_name().to_string()];
        let mut out = String::new();
        describe_message_tree(&msg, 0, &mut visiting, &mut out);
        assert!(out.contains("children"));
        assert!(out.contains("max depth or recursive type"));
    }
}
