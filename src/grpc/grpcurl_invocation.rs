use anyhow::Result;
use serde_json::Value;
use std::collections::HashMap;

#[derive(Debug, Clone, PartialEq)]
pub struct ParsedGrpcurl {
    pub address: String,
    pub symbol: String,
    pub request_body: Value,
    pub headers: HashMap<String, String>,
    pub tls: HashMap<String, String>,
    pub options: HashMap<String, String>,
    pub proto: HashMap<String, String>,
}

impl ParsedGrpcurl {
    pub fn parse(args: &[String]) -> Result<Self> {
        let mut headers = HashMap::new();
        let mut tls = HashMap::new();
        let mut options = HashMap::new();
        let mut proto = HashMap::new();
        let mut request_body: Option<Value> = None;
        let mut positionals: Vec<String> = Vec::new();

        let mut i = 0usize;
        while i < args.len() {
            let token = args[i].as_str();
            match token {
                "-plaintext" => {
                    options.insert("plaintext".to_string(), "true".to_string());
                }
                "-gzip" => {
                    options.insert("compression".to_string(), "gzip".to_string());
                }
                "-insecure" => {
                    tls.insert("insecure-skip-verify".to_string(), "true".to_string());
                }
                "-H" | "-rpc-header" | "-reflect-header" => {
                    let value = next_value(args, i, token)?;
                    let (k, v) = parse_header(value)?;
                    headers.insert(k, v);
                    i += 1;
                }
                "-d" => {
                    let value = next_value(args, i, "-d")?;
                    request_body = Some(parse_json_or_string(value));
                    i += 1;
                }
                "-cacert" => {
                    let value = next_value(args, i, token)?;
                    tls.insert("ca-cert".to_string(), value.to_string());
                    i += 1;
                }
                "-cert" => {
                    let value = next_value(args, i, token)?;
                    tls.insert("client-cert".to_string(), value.to_string());
                    i += 1;
                }
                "-key" => {
                    let value = next_value(args, i, token)?;
                    tls.insert("client-key".to_string(), value.to_string());
                    i += 1;
                }
                "-servername" => {
                    let value = next_value(args, i, token)?;
                    tls.insert("server-name".to_string(), value.to_string());
                    i += 1;
                }
                "-import-path" => {
                    let value = next_value(args, i, token)?;
                    push_csv_option(&mut proto, "import_paths", value);
                    i += 1;
                }
                "-proto" => {
                    let value = next_value(args, i, token)?;
                    push_csv_option(&mut proto, "files", value);
                    i += 1;
                }
                "-protoset" => {
                    let value = next_value(args, i, token)?;
                    proto.insert("descriptor".to_string(), value.to_string());
                    i += 1;
                }
                s if s.starts_with('-') => {
                    if let Some((k, v)) = parse_equals_flag(s) {
                        options.insert(k, v);
                    } else if flag_takes_value(s) {
                        let value = next_value(args, i, s)?;
                        options.insert(normalize_flag_name(s), value.to_string());
                        i += 1;
                    } else {
                        options.insert(normalize_flag_name(s), "true".to_string());
                    }
                }
                _ => positionals.push(args[i].clone()),
            }
            i += 1;
        }

        if positionals.first().is_some_and(|v| v == "grpcurl") {
            positionals.remove(0);
        }

        if positionals.len() < 2 {
            anyhow::bail!("Expected grpcurl address and service/method symbol");
        }

        let address = positionals[positionals.len() - 2].clone();
        let symbol = positionals[positionals.len() - 1].clone();

        Ok(Self {
            address,
            symbol,
            request_body: request_body.unwrap_or_else(|| Value::Object(serde_json::Map::new())),
            headers,
            tls,
            options,
            proto,
        })
    }
}

fn next_value<'a>(args: &'a [String], idx: usize, flag: &str) -> Result<&'a str> {
    args.get(idx + 1)
        .map(|v| v.as_str())
        .ok_or_else(|| anyhow::anyhow!("Missing value for {}", flag))
}

fn parse_header(value: &str) -> Result<(String, String)> {
    let idx = value
        .find(':')
        .ok_or_else(|| anyhow::anyhow!("Invalid header '{}': expected 'Key: Value'", value))?;

    let key = value[..idx].trim();
    let val = value[idx + 1..].trim();
    if key.is_empty() {
        anyhow::bail!("Invalid header '{}': empty key", value);
    }
    Ok((key.to_string(), val.to_string()))
}

fn parse_equals_flag(flag: &str) -> Option<(String, String)> {
    let body = flag.trim_start_matches('-');
    let (k, v) = body.split_once('=')?;
    if k.is_empty() {
        return None;
    }
    Some((k.to_string(), v.to_string()))
}

fn normalize_flag_name(flag: &str) -> String {
    flag.trim_start_matches('-').to_string()
}

fn flag_takes_value(flag: &str) -> bool {
    matches!(
        flag,
        "-max-time"
            | "-max-msg-sz"
            | "-max-call-recv-msg-size"
            | "-max-call-send-msg-size"
            | "-authority"
            | "-format"
            | "-msg-template"
            | "-connect-timeout"
    )
}

fn push_csv_option(map: &mut HashMap<String, String>, key: &str, value: &str) {
    map.entry(key.to_string())
        .and_modify(|existing| {
            existing.push(',');
            existing.push_str(value);
        })
        .or_insert_with(|| value.to_string());
}

fn parse_json_or_string(value: &str) -> Value {
    serde_json::from_str(value).unwrap_or_else(|_| Value::String(value.to_string()))
}

pub fn parse_response_payload(stdout: &str) -> Vec<Value> {
    if stdout.trim().is_empty() {
        return vec![Value::Object(serde_json::Map::new())];
    }

    if let Ok(value) = serde_json::from_str::<Value>(stdout) {
        return vec![value];
    }

    let mut values = Vec::new();
    for line in stdout.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        values.push(parse_json_or_string(trimmed));
    }

    if values.is_empty() {
        vec![Value::Object(serde_json::Map::new())]
    } else {
        values
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn strings(values: &[&str]) -> Vec<String> {
        values.iter().map(|v| (*v).to_string()).collect()
    }

    #[test]
    fn parse_grpcurl_basic() {
        let args = strings(&[
            "-plaintext",
            "-H",
            "x-api-key: wrong-key",
            "-d",
            r#"{"action":"delete"}"#,
            "localhost:4770",
            "auth.AuthService/CheckAccess",
        ]);

        let parsed = ParsedGrpcurl::parse(&args).expect("parse grpcurl args");
        assert_eq!(parsed.address, "localhost:4770");
        assert_eq!(parsed.symbol, "auth.AuthService/CheckAccess");
        assert_eq!(parsed.request_body, json!({"action": "delete"}));
        assert_eq!(
            parsed.headers.get("x-api-key").map(String::as_str),
            Some("wrong-key")
        );
        assert_eq!(
            parsed.options.get("plaintext").map(String::as_str),
            Some("true")
        );
    }

    #[test]
    fn parse_grpcurl_supports_permutation_order() {
        let a = strings(&[
            "-plaintext",
            "-H",
            "x-api-key: wrong-key",
            "-d",
            r#"{"action":"delete"}"#,
            "localhost:4770",
            "auth.AuthService/CheckAccess",
        ]);
        let b = strings(&[
            "localhost:4770",
            "-d",
            r#"{"action":"delete"}"#,
            "-H",
            "x-api-key: wrong-key",
            "auth.AuthService/CheckAccess",
            "-plaintext",
        ]);
        let c = strings(&[
            "grpcurl",
            "-H",
            "x-api-key: wrong-key",
            "-plaintext",
            "localhost:4770",
            "auth.AuthService/CheckAccess",
            "-d",
            r#"{"action":"delete"}"#,
        ]);

        let pa = ParsedGrpcurl::parse(&a).unwrap();
        let pb = ParsedGrpcurl::parse(&b).unwrap();
        let pc = ParsedGrpcurl::parse(&c).unwrap();

        assert_eq!(pa, pb);
        assert_eq!(pa, pc);
    }

    #[test]
    fn parse_grpcurl_handles_proto_flags_repeated() {
        let args = strings(&[
            "-import-path",
            "proto",
            "-import-path",
            "third_party",
            "-proto",
            "a.proto",
            "-proto",
            "b.proto",
            "-protoset",
            "desc.pb",
            "localhost:4770",
            "auth.AuthService/CheckAccess",
        ]);

        let parsed = ParsedGrpcurl::parse(&args).unwrap();
        assert_eq!(
            parsed.proto.get("import_paths").map(String::as_str),
            Some("proto,third_party")
        );
        assert_eq!(
            parsed.proto.get("files").map(String::as_str),
            Some("a.proto,b.proto")
        );
        assert_eq!(
            parsed.proto.get("descriptor").map(String::as_str),
            Some("desc.pb")
        );
    }

    #[test]
    fn parse_grpcurl_supports_equals_style_unknown_flags() {
        let args = strings(&[
            "-emit-defaults=true",
            "-allow-unknown-fields=false",
            "localhost:4770",
            "auth.AuthService/CheckAccess",
        ]);

        let parsed = ParsedGrpcurl::parse(&args).unwrap();
        assert_eq!(
            parsed.options.get("emit-defaults").map(String::as_str),
            Some("true")
        );
        assert_eq!(
            parsed
                .options
                .get("allow-unknown-fields")
                .map(String::as_str),
            Some("false")
        );
    }

    #[test]
    fn parse_grpcurl_errors_on_missing_positionals() {
        let args = strings(&["-plaintext"]);
        let err = ParsedGrpcurl::parse(&args).expect_err("missing address/symbol must fail");
        assert!(
            err.to_string()
                .contains("Expected grpcurl address and service/method symbol")
        );
    }

    #[test]
    fn parse_grpcurl_errors_on_invalid_header() {
        let args = strings(&[
            "-H",
            "broken_header",
            "localhost:4770",
            "auth.AuthService/CheckAccess",
        ]);

        let err = ParsedGrpcurl::parse(&args).expect_err("invalid header should fail");
        assert!(err.to_string().contains("Invalid header"));
    }

    #[test]
    fn parse_grpcurl_consumes_known_option_values() {
        let args = strings(&[
            "-max-time",
            "5",
            "-format",
            "json",
            "localhost:4770",
            "auth.AuthService/CheckAccess",
        ]);
        let parsed = ParsedGrpcurl::parse(&args).unwrap();
        assert_eq!(
            parsed.options.get("max-time").map(String::as_str),
            Some("5")
        );
        assert_eq!(
            parsed.options.get("format").map(String::as_str),
            Some("json")
        );
    }

    #[test]
    fn parse_response_payload_json_lines() {
        let payload = "{\"a\":1}\n{\"b\":2}\n";
        let parsed = parse_response_payload(payload);
        assert_eq!(parsed, vec![json!({"a": 1}), json!({"b": 2})]);
    }

    #[test]
    fn parse_response_payload_empty_defaults_to_empty_object() {
        let parsed = parse_response_payload("\n\n");
        assert_eq!(parsed, vec![json!({})]);
    }

    #[test]
    fn parse_response_payload_single_json_object() {
        let parsed = parse_response_payload(r#"{"ok":true,"count":2}"#);
        assert_eq!(parsed, vec![json!({"ok": true, "count": 2})]);
    }
}
