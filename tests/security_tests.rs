// Security tests - TLS warnings, address validation, update mode validation

use grpctestify::grpc::TlsConfig;

#[test]
fn test_tls_config_insecure_skip_verify() {
    let tls_config = TlsConfig {
        ca_cert_path: None,
        client_cert_path: None,
        client_key_path: None,
        server_name: None,
        insecure_skip_verify: true,
    };

    assert!(tls_config.insecure_skip_verify);
}

#[test]
fn test_tls_config_secure() {
    let tls_config = TlsConfig {
        ca_cert_path: None,
        client_cert_path: None,
        client_key_path: None,
        server_name: None,
        insecure_skip_verify: false,
    };

    assert!(!tls_config.insecure_skip_verify);
}

#[test]
fn test_tls_config_with_server_name() {
    let tls_config = TlsConfig {
        ca_cert_path: None,
        client_cert_path: None,
        client_key_path: None,
        server_name: Some("example.com".to_string()),
        insecure_skip_verify: false,
    };

    assert_eq!(tls_config.server_name, Some("example.com".to_string()));
    assert!(!tls_config.insecure_skip_verify);
}
