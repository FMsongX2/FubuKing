// Modified by Atlas from upstream OpenAI Codex (Apache-2.0). See CONTEXT.md.
use std::path::PathBuf;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("cargo:rustc-check-cfg=cfg(atlas_engine_bazel)");
    println!("cargo:rerun-if-changed=src/grpc");

    let mut config = tonic_prost_build::Config::new();
    config.protoc_executable(protoc_bin_vendored::protoc_bin_path()?);
    let proto_files = glob::glob("src/grpc/*.proto")?.collect::<Result<Vec<_>, _>>()?;

    tonic_prost_build::configure()
        .build_client(/*enable*/ true)
        .build_server(/*enable*/ true)
        .compile_with_config(config, &proto_files, &[PathBuf::from("src/grpc")])?;

    Ok(())
}
