use std::env;

fn use_vendored_protoc() -> Result<(), Box<dyn std::error::Error>> {
    // Use vendored protoc binary (no system protoc required)
    unsafe {
        env::set_var("PROTOC", protoc_bin_vendored::protoc_bin_path()?);
    }
    Ok(())
}

#[cfg(feature = "proto-build")]
fn compile_main_protos() -> Result<(), Box<dyn std::error::Error>> {
    use_vendored_protoc()?;

    let out_dir = std::path::PathBuf::from(env::var("OUT_DIR").unwrap());
    tonic_prost_build::configure()
        .file_descriptor_set_path(out_dir.join("helloworld_descriptor.bin"))
        .compile_protos(&["tests/server/helloworld.proto"], &["tests/server"])?;
    Ok(())
}

#[cfg(not(feature = "proto-build"))]
fn compile_main_protos() -> Result<(), Box<dyn std::error::Error>> {
    // Skip proto compilation when proto-build feature is not enabled
    Ok(())
}

fn compile_test_server_protos() -> Result<(), Box<dyn std::error::Error>> {
    // Test server protos are only needed when test-servers feature is enabled.
    if env::var_os("CARGO_FEATURE_TEST_SERVERS").is_none() {
        return Ok(());
    }

    use_vendored_protoc()?;

    let test_proto_dir = std::path::Path::new("tests/servers/proto");

    if !test_proto_dir.exists() {
        return Ok(());
    }

    let out_dir = std::path::PathBuf::from(env::var("OUT_DIR").unwrap());

    // Find all proto files in test server directory
    let proto_files = std::fs::read_dir(test_proto_dir)?
        .filter_map(|e| e.ok())
        .filter(|e| e.path().extension().is_some_and(|ext| ext == "proto"))
        .map(|e| e.path())
        .collect::<Vec<_>>();

    if proto_files.is_empty() {
        return Ok(());
    }

    // Print rerun-if-changed for all proto files
    for proto in &proto_files {
        println!("cargo:rerun-if-changed={}", proto.display());
    }

    // Compile test server protos
    tonic_prost_build::configure()
        .file_descriptor_set_path(out_dir.join("test_servers_descriptor.bin"))
        .compile_protos(&proto_files, &[test_proto_dir.to_path_buf()])?;

    Ok(())
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Compile main protos (optional, feature-gated)
    compile_main_protos()?;

    // Compile test server protos (always if they exist)
    compile_test_server_protos()?;

    Ok(())
}
