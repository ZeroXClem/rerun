use std::path::PathBuf;

fn main() -> Result<(), std::io::Error> {
    if std::env::var("CI").is_ok() {
        // No need to run this on CI (which means setting up `protoc` etc) since the code is committed
        // anyway.
        return Ok(());
    }
    if std::env::var("IS_IN_RERUN_WORKSPACE") != Ok("yes".to_owned()) {
        // Only run if we are in the rerun workspace, not on users machines (if we ever publish the example).
        return Ok(());
    }

    prost_build::compile_protos(
        &[
            "proto/a_r_capture_metadata.proto",
            "proto/annotation_data.proto",
            "proto/object.proto",
        ],
        &["proto"],
    )?;

    let out_dir = std::env::var("OUT_DIR").unwrap();
    let src_path = PathBuf::from(out_dir).join("objectron.proto.rs");
    let dst_path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("src/objectron.rs");

    // `include!()` will break LSP & Github navigation, so create an actual source file to make the
    // UX reasonable.

    let bytes = [
        b"// This file was autogenerated by `build.rs`. Do not edit.\n\n".to_vec(),
        b"#![allow(clippy::all, clippy::doc_markdown)]\n".to_vec(),
        std::fs::read(src_path)?,
    ]
    .into_iter()
    .flatten()
    .collect::<Vec<_>>();

    // `cargo` has an implicit `rerun-if-changed=src/**` clause, which will act against us in this
    // instance.
    // Make sure to _not_ rewrite identical data, so as to avoid being stuck in an infinite build
    // loop when using tools like e.g. `bacon` that watch the filesystem for any changes to the
    // project's files.
    write_file_if_necessary(dst_path, &bytes)
}

/// Only touch the file if the contents has actually changed
fn write_file_if_necessary(
    dst_path: impl AsRef<std::path::Path>,
    content: &[u8],
) -> std::io::Result<()> {
    if let Ok(cur_bytes) = std::fs::read(&dst_path) {
        if cur_bytes == content {
            return Ok(());
        }
    }

    std::fs::write(dst_path, content)
}