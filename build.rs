use std::{env, fs, path::PathBuf};

fn main() {
    let out_dir = PathBuf::from(env::var_os("OUT_DIR").expect("OUT_DIR is set by Cargo"));
    let destination = out_dir.join("embedded-tree-sitter");

    if let Some(source) = env::var_os("LAZYVIM_EMBED_TREE_SITTER") {
        let source = PathBuf::from(source);
        if source.exists() {
            fs::copy(&source, &destination).expect("failed to copy embedded tree-sitter binary");
            println!("cargo:rerun-if-changed={}", source.display());
            return;
        }
    }

    fs::write(&destination, []).expect("failed to write empty embedded tree-sitter placeholder");
    println!("cargo:rerun-if-env-changed=LAZYVIM_EMBED_TREE_SITTER");
}
