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
                    // -d @ means "read from stdin" — in import context, skip body
                    if value == "@" {
                        request_body = Some(Value::Object(serde_json::Map::new()));
                    } else {
                        request_body = Some(parse_json_or_string(value));
                    }
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

    // ── Parameterized format tests ─────────────────────────

    fn p(args: &[&str]) -> ParsedGrpcurl {
        ParsedGrpcurl::parse(&args.iter().map(|s| (*s).to_string()).collect::<Vec<_>>()).unwrap()
    }

    struct TC {
        args: &'static [&'static str],
        check: fn(&ParsedGrpcurl),
    }

    macro_rules! t {
        ($args:expr, $check:expr) => {
            TC {
                args: $args,
                check: $check,
            }
        };
    }

    #[test]
    fn fmt_all_variants() {
        let cases = vec![
            t!(&["-plaintext", "localhost:50051", "foo.Bar/Baz"], |r| {
                assert_eq!(r.address, "localhost:50051");
                assert_eq!(r.symbol, "foo.Bar/Baz");
                assert_eq!(r.options.get("plaintext").map(String::as_str), Some("true"));
            }),
            t!(
                &["-plaintext", "-d", "{}", "localhost:50051", "foo.Bar/Baz"],
                |r| {
                    assert_eq!(r.request_body, json!({}));
                }
            ),
            t!(
                &[
                    "-d",
                    r#"{"id":1}"#,
                    "-plaintext",
                    "localhost:50051",
                    "foo.Bar/Baz"
                ],
                |r| {
                    assert_eq!(r.request_body, json!({"id": 1}));
                }
            ),
            t!(
                &[
                    "-plaintext",
                    "localhost:50051",
                    "-d",
                    r#"{"id":1}"#,
                    "foo.Bar/Baz"
                ],
                |r| {
                    assert_eq!(r.request_body, json!({"id": 1}));
                }
            ),
            t!(
                &[
                    "-H",
                    "authorization: Bearer abc",
                    "-plaintext",
                    "-d",
                    r#"{"id":1}"#,
                    "localhost:50051",
                    "foo.Bar/Baz"
                ],
                |r| {
                    assert_eq!(
                        r.headers.get("authorization").map(String::as_str),
                        Some("Bearer abc")
                    );
                }
            ),
            t!(
                &[
                    "-H",
                    "x-a:1",
                    "-H",
                    "x-b:2",
                    "-plaintext",
                    "-d",
                    r#"{"a":1,"b":2}"#,
                    "localhost:50051",
                    "foo.Bar/Baz"
                ],
                |r| {
                    assert_eq!(r.headers.get("x-a").map(String::as_str), Some("1"));
                    assert_eq!(r.headers.get("x-b").map(String::as_str), Some("2"));
                    assert_eq!(r.request_body, json!({"a": 1, "b": 2}));
                }
            ),
            t!(
                &[
                    "-plaintext",
                    "-d",
                    r#"{"b":2,"a":1}"#,
                    "localhost:50051",
                    "foo.Bar/Baz"
                ],
                |r| {
                    let s = serde_json::to_string(&r.request_body).unwrap();
                    assert!(s.contains(r#""b""#), "field order: {}", s);
                }
            ),
            t!(
                &[
                    "-plaintext",
                    "-d",
                    r#"{"nested":{"x":1,"y":[1,2,3]}}"#,
                    "localhost:50051",
                    "foo.Bar/Baz"
                ],
                |r| {
                    assert_eq!(r.request_body["nested"]["x"], json!(1));
                    assert_eq!(r.request_body["nested"]["y"], json!([1, 2, 3]));
                }
            ),
            t!(
                &[
                    "-plaintext",
                    "-d",
                    r#"{"text":"Привет 🌍"}"#,
                    "localhost:50051",
                    "foo.Bar/Baz"
                ],
                |r| {
                    assert_eq!(r.request_body["text"], "Привет 🌍");
                }
            ),
            t!(
                &["-plaintext", "-d", "@", "localhost:50051", "foo.Bar/Baz"],
                |r| {
                    assert_eq!(r.request_body, json!({}), "-d @ → empty body");
                }
            ),
            t!(
                &[
                    "-emit-defaults",
                    "-plaintext",
                    "localhost:50051",
                    "foo.Bar/Baz"
                ],
                |r| {
                    assert_eq!(
                        r.options.get("emit-defaults").map(String::as_str),
                        Some("true")
                    );
                }
            ),
            t!(
                &[
                    "-allow-unknown-fields",
                    "-plaintext",
                    "-d",
                    r#"{"unknown":1}"#,
                    "localhost:50051",
                    "foo.Bar/Baz"
                ],
                |r| {
                    assert_eq!(
                        r.options.get("allow-unknown-fields").map(String::as_str),
                        Some("true")
                    );
                }
            ),
            t!(
                &[
                    "-proto",
                    "api.proto",
                    "-import-path",
                    "./proto",
                    "-plaintext",
                    "-d",
                    "{}",
                    "localhost:50051",
                    "foo.Bar/Baz"
                ],
                |r| {
                    assert_eq!(r.proto.get("files").map(String::as_str), Some("api.proto"));
                    assert_eq!(
                        r.proto.get("import_paths").map(String::as_str),
                        Some("./proto")
                    );
                }
            ),
            t!(
                &[
                    "-protoset",
                    "api.protoset",
                    "-d",
                    r#"{"id":123}"#,
                    "localhost:50051",
                    "foo.Bar/Baz"
                ],
                |r| {
                    assert_eq!(
                        r.proto.get("descriptor").map(String::as_str),
                        Some("api.protoset")
                    );
                    assert_eq!(r.request_body["id"], json!(123));
                }
            ),
            t!(
                &["-cacert", "ca.pem", "api.example.com:443", "foo.Bar/Baz"],
                |r| {
                    assert_eq!(r.tls.get("ca-cert").map(String::as_str), Some("ca.pem"));
                    assert_eq!(r.address, "api.example.com:443");
                }
            ),
            t!(
                &[
                    "-cert",
                    "client.pem",
                    "-key",
                    "client.key",
                    "api.example.com:443",
                    "foo.Bar/Baz"
                ],
                |r| {
                    assert_eq!(
                        r.tls.get("client-cert").map(String::as_str),
                        Some("client.pem")
                    );
                    assert_eq!(
                        r.tls.get("client-key").map(String::as_str),
                        Some("client.key")
                    );
                }
            ),
            t!(
                &[
                    "-authority",
                    "example.com",
                    "-plaintext",
                    "localhost:50051",
                    "foo.Bar/Baz"
                ],
                |r| {
                    assert_eq!(
                        r.options.get("authority").map(String::as_str),
                        Some("example.com")
                    );
                }
            ),
            t!(
                &[
                    "-servername",
                    "example.com",
                    "api.example.com:443",
                    "foo.Bar/Baz"
                ],
                |r| {
                    assert_eq!(
                        r.tls.get("server-name").map(String::as_str),
                        Some("example.com")
                    );
                }
            ),
            t!(
                &[
                    "-max-time",
                    "5",
                    "-plaintext",
                    "localhost:50051",
                    "foo.Bar/Baz"
                ],
                |r| {
                    assert_eq!(r.options.get("max-time").map(String::as_str), Some("5"));
                }
            ),
            t!(
                &["-vv", "-plaintext", "localhost:50051", "foo.Bar/Baz"],
                |r| {
                    assert_eq!(r.options.get("vv").map(String::as_str), Some("true"));
                }
            ),
            t!(
                &["grpcurl", "-plaintext", "localhost:50051", "foo.Bar/Baz"],
                |r| {
                    assert_eq!(r.address, "localhost:50051");
                    assert_eq!(r.symbol, "foo.Bar/Baz");
                }
            ),
        ];

        for tc in cases {
            let result = p(tc.args);
            (tc.check)(&result);
        }
    }
}
