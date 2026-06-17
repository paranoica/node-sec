//! Compiles the decision gRPC contract (proto/decision.proto) into Rust via tonic-build.

fn main() -> Result<(), Box<dyn std::error::Error>> {
    tonic_build::configure()
        .build_server(true)
        .build_client(false)
        .compile_protos(&["../../proto/decision.proto"], &["../../proto"])?;
    println!("cargo:rerun-if-changed=../../proto/decision.proto");
    Ok(())
}
