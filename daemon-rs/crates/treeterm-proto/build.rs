use std::path::PathBuf;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    // Navigate from crates/treeterm-proto/ up to repo root's src/proto/
    let proto_dir = manifest_dir.join("../../../src/proto");
    let proto_file = proto_dir.join("treeterm.proto");

    tonic_build::configure()
        .build_server(true)
        .build_client(false)
        .compile_protos(&[&proto_file], &[&proto_dir])?;

    println!("cargo:rerun-if-changed={}", proto_file.display());
    Ok(())
}
