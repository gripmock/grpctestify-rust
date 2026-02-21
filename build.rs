#[cfg(feature = "proto-build")]
use std::env;
#[cfg(feature = "proto-build")]
use std::path::PathBuf;

#[cfg(feature = "proto-build")]
fn compile_protos() -> Result<(), Box<dyn std::error::Error>> {
    // Use vendored protoc binary (no system protoc required)
    unsafe {
        env::set_var("PROTOC", protoc_bin_vendored::protoc_bin_path().unwrap());
    }

    let out_dir = PathBuf::from(env::var("OUT_DIR").unwrap());
    tonic_build::configure()
        .file_descriptor_set_path(out_dir.join("helloworld_descriptor.bin"))
        .compile_protos(&["tests/server/helloworld.proto"], &["tests/server"])?;
    Ok(())
}

#[cfg(not(feature = "proto-build"))]
fn compile_protos() -> Result<(), Box<dyn std::error::Error>> {
    // Skip proto compilation when proto-build feature is not enabled
    Ok(())
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    compile_protos()
}
