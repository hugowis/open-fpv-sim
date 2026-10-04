fn main() -> Result<(), Box<dyn std::error::Error>> {
    std::env::set_var("PROTOC", protoc_bin_vendored::protoc_bin_path()?);
    tonic_build::configure().compile_protos(&["../../proto/ofs/v1/sim.proto"], &["../../proto"])?;
    println!("cargo:rerun-if-changed=../../proto/ofs/v1/sim.proto");
    Ok(())
}
