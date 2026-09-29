#[derive(Default, Clone, Copy)]
pub struct TlsPaths<'a> {
    pub ca: Option<&'a str>,
    pub cert: Option<&'a str>,
    pub key: Option<&'a str>,
}

#[derive(Default, Clone, Copy)]
pub struct CallSpec<'a> {
    pub endpoint: &'a str,
    pub address: Option<&'a str>,
    pub protocol: Option<&'a str>,
    pub body: Option<&'a str>,
    pub headers: &'a [(String, String)],
    pub tls: TlsPaths<'a>,
    pub protoset: Option<&'a str>,
    pub insecure: bool,
    pub plaintext: bool,
    pub max_time: Option<u64>,
}

fn quote(value: &str) -> String {
    format!("'{}'", value.replace('\'', "'\\''"))
}

pub fn grpctestify_call(spec: CallSpec<'_>) -> String {
    let CallSpec {
        endpoint,
        address,
        protocol,
        body,
        headers,
        tls,
        insecure,
        plaintext,
        max_time,
        ..
    } = spec;
    let mut line = String::from("grpctestify call -e ");
    line.push_str(&quote(endpoint));
    if let Some(address) = address.filter(|a| !a.trim().is_empty()) {
        line.push_str(" --address ");
        line.push_str(&quote(address));
    }
    if let Some(body) = body.filter(|b| !b.trim().is_empty()) {
        line.push_str(" -d ");
        line.push_str(&quote(body));
    }
    if let Some(protocol) = protocol.filter(|p| !p.is_empty() && *p != "grpc") {
        line.push_str(&format!(" --protocol {protocol}"));
    }
    if insecure {
        line.push_str(" --insecure");
    } else if plaintext {
        line.push_str(" --plaintext");
    }
    for (name, value) in headers {
        if name.trim().is_empty() {
            continue;
        }
        line.push_str(" -H ");
        line.push_str(&quote(&format!("{name}: {value}")));
    }
    for (flag, path) in [
        ("--tls-ca", tls.ca),
        ("--tls-cert", tls.cert),
        ("--tls-key", tls.key),
    ] {
        if let Some(path) = path.filter(|p| !p.trim().is_empty()) {
            line.push_str(&format!(" {flag} "));
            line.push_str(&quote(path));
        }
    }
    if let Some(seconds) = max_time {
        line.push_str(&format!(" --max-time {seconds}"));
    }
    line
}

pub fn grpctestify_call_file(path: &str, doc_index: usize) -> String {
    let mut line = format!("grpctestify call {}", quote(path));
    if doc_index > 1 {
        line.push_str(&format!(" --doc-index {doc_index}"));
    }
    line
}

pub fn grpcurl_line(spec: CallSpec<'_>) -> String {
    let CallSpec {
        endpoint,
        address,
        body,
        headers,
        tls,
        protoset,
        insecure,
        plaintext,
        max_time,
        ..
    } = spec;
    let address = address.unwrap_or("localhost:4770");
    let mut line = String::from("grpcurl");
    if plaintext {
        line.push_str(" -plaintext");
    } else if insecure {
        line.push_str(" -insecure");
    }
    for (flag, path) in [("-cacert", tls.ca), ("-cert", tls.cert), ("-key", tls.key)] {
        if let Some(path) = path.filter(|p| !p.trim().is_empty()) {
            line.push_str(&format!(" {flag} "));
            line.push_str(&quote(path));
        }
    }
    if let Some(protoset) = protoset.filter(|p| !p.trim().is_empty()) {
        line.push_str(" -protoset ");
        line.push_str(&quote(protoset));
    }
    for (name, value) in headers {
        if name.trim().is_empty() {
            continue;
        }
        line.push_str(" -H ");
        line.push_str(&quote(&format!("{name}: {value}")));
    }
    if let Some(body) = body.filter(|b| !b.trim().is_empty()) {
        line.push_str(" -d ");
        line.push_str(&quote(body));
    }
    if let Some(seconds) = max_time {
        line.push_str(&format!(" -max-time {seconds}"));
    }
    line.push(' ');
    line.push_str(address);
    line.push(' ');
    line.push_str(endpoint);
    line
}

pub fn curl_line(
    method: &str,
    url: &str,
    headers: &[(String, String)],
    body: Option<&str>,
    max_time: Option<u64>,
) -> String {
    let mut line = String::from("curl -L");
    if !method.eq_ignore_ascii_case("GET") {
        line.push_str(" -X ");
        line.push_str(method);
    }
    line.push(' ');
    line.push_str(&quote(url));
    for (name, value) in headers {
        line.push_str(" -H ");
        line.push_str(&quote(&format!("{name}: {value}")));
    }
    if let Some(body) = body.filter(|b| !b.trim().is_empty()) {
        line.push_str(" -d ");
        line.push_str(&quote(body));
    }
    if let Some(seconds) = max_time {
        line.push_str(&format!(" --max-time {seconds}"));
    }
    line
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_call_line_carries_what_call_can_take() {
        let line = grpctestify_call(CallSpec {
            endpoint: "pkg.Svc/M",
            address: Some("localhost:4770"),
            protocol: Some("grpc"),
            body: Some("{\"a\":1}"),
            ..Default::default()
        });
        assert_eq!(
            line,
            "grpctestify call -e 'pkg.Svc/M' --address 'localhost:4770' -d '{\"a\":1}'"
        );
    }

    #[test]
    fn the_transport_is_named_only_when_it_is_not_the_default() {
        assert!(
            !grpctestify_call(CallSpec {
                endpoint: "a/B",
                protocol: Some("grpc"),
                ..Default::default()
            })
            .contains("--protocol")
        );
        assert!(
            grpctestify_call(CallSpec {
                endpoint: "a/B",
                protocol: Some("grpc-web"),
                ..Default::default()
            })
            .contains("--protocol grpc-web")
        );
    }

    #[test]
    fn headers_are_carried_not_counted() {
        let headers = [
            ("authorization".to_string(), "Bearer x y".to_string()),
            ("x-tenant".to_string(), "acme".to_string()),
        ];
        let line = grpctestify_call(CallSpec {
            endpoint: "a/B",
            headers: &headers,
            ..Default::default()
        });
        assert!(line.contains("-H 'authorization: Bearer x y'"), "{line}");
        assert!(line.contains("-H 'x-tenant: acme'"), "{line}");
        assert!(!line.contains("not flags"), "{line}");
    }

    #[test]
    fn a_call_line_for_a_file_names_the_file() {
        assert_eq!(
            grpctestify_call_file("auth/login.gctf", 1),
            "grpctestify call 'auth/login.gctf'"
        );
        assert_eq!(
            grpctestify_call_file("chain.gctf", 3),
            "grpctestify call 'chain.gctf' --doc-index 3"
        );
    }

    #[test]
    fn a_nameless_header_is_left_out() {
        let headers = [(" ".to_string(), "nothing".to_string())];
        assert!(
            !grpctestify_call(CallSpec {
                endpoint: "a/B",
                headers: &headers,
                ..Default::default()
            })
            .contains("-H")
        );
    }

    #[test]
    fn a_grpcurl_line_names_the_target_last_as_grpcurl_wants_it() {
        assert_eq!(
            grpcurl_line(CallSpec {
                endpoint: "pkg.Svc/M",
                address: Some("localhost:4770"),
                body: Some("{\"a\":1}"),
                plaintext: true,
                ..Default::default()
            }),
            "grpcurl -plaintext -d '{\"a\":1}' localhost:4770 pkg.Svc/M"
        );
        assert!(
            !grpcurl_line(CallSpec {
                endpoint: "a/B",
                address: Some("h:1"),
                ..Default::default()
            })
            .contains("-plaintext")
        );

        let full = grpcurl_line(CallSpec {
            endpoint: "a/B",
            address: Some("h:1"),
            headers: &[("authorization".to_string(), "Bearer t".to_string())],
            protoset: Some("/tmp/schema.bin"),
            plaintext: true,
            ..Default::default()
        });
        assert!(full.contains("-protoset '/tmp/schema.bin'"), "{full}");
        assert!(full.contains("-H 'authorization: Bearer t'"), "{full}");
    }

    #[test]
    fn a_quote_inside_a_body_cannot_break_out_of_the_shell() {
        let line = grpctestify_call(CallSpec {
            endpoint: "a/B",
            body: Some("{\"s\":\"it's\"}"),
            ..Default::default()
        });
        assert!(line.contains("'{\"s\":\"it'\\''s\"}'"), "{line}");
    }
    #[test]
    fn a_curl_line_is_spelled_the_way_the_workbench_spells_it() {
        assert_eq!(
            curl_line(
                "POST",
                "https://api.example.com/v1/users",
                &[("content-type".to_string(), "application/json".to_string())],
                Some("{\"name\":\"Ada\"}"),
                None,
            ),
            "curl -L -X POST 'https://api.example.com/v1/users' -H 'content-type: application/json' -d '{\"name\":\"Ada\"}'",
        );
        assert_eq!(
            curl_line("GET", "https://x.test/f", &[], None, None),
            "curl -L 'https://x.test/f'"
        );
    }
}
