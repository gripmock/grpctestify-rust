use grpctestify::grpc::client::{CompressionMode, GrpcClient, GrpcClientConfig, ProtoConfig};
use std::path::PathBuf;

#[tokio::test]
async fn test_local_proto_files_are_rejected_in_native_mode() {
    // 1. Pick a random port that nothing is listening on.
    // We can rely on the OS to likely not have anything on 59123.
    let address = "http://localhost:59123";

    // 2. Configure client with local proto files.
    // We assume the test runs from the workspace root.
    let proto_path = PathBuf::from("tests/e2e/examples/helloworld/helloworld.proto");
    let import_path = PathBuf::from("tests/e2e/examples/helloworld");
    
    // Verify file exists to avoid spurious test failures
    assert!(proto_path.exists(), "proto file not found at {:?}", proto_path);

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
    };

    // 3. In native mode, local proto files are intentionally not supported to avoid
    // runtime dependency on protoc.
    let result = GrpcClient::new(config).await;
    assert!(result.is_err(), "Client initialization unexpectedly succeeded");
    let err = result.err().unwrap().to_string();
    assert!(
        err.contains("PROTO files are not supported in native mode"),
        "Unexpected error: {}",
        err
    );
}
