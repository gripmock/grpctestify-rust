use grpctestify::grpc::client::{CompressionMode, GrpcClient, GrpcClientConfig, ProtoConfig};
use std::path::PathBuf;

#[tokio::test]
async fn test_local_proto_files_are_rejected_in_native_mode() {
    let address = "http://localhost:59123";
    let proto_path = PathBuf::from("tests/e2e/examples/helloworld/helloworld.proto");
    let import_path = PathBuf::from("tests/e2e/examples/helloworld");

    assert!(
        proto_path.exists(),
        "proto file not found at {:?}",
        proto_path
    );

    let config = GrpcClientConfig {
        address: address.to_string(),
        timeout_seconds: 5,
        tls_config: None,
        proto_config: Some(ProtoConfig {
            files: vec![proto_path.to_string_lossy().to_string()],
            import_paths: vec![import_path.to_string_lossy().to_string()],
            descriptor: None,
        }),
        metadata: None,
        target_service: None,
        compression: CompressionMode::None,
        connection_id: 0,
        protocol: Default::default(),
    };

    let result = GrpcClient::new(config).await;
    assert!(
        result.is_err(),
        "Client initialization unexpectedly succeeded"
    );
    let err = result.err().unwrap().to_string();
    assert!(
        err.contains("PROTO files are not supported in native mode"),
        "Unexpected error: {}",
        err
    );
}
