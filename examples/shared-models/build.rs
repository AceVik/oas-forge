//! Writes the OpenAPI fragments of the shared models to `OUT_DIR` and tells
//! the build scripts of dependent crates where they are: with
//! `links = "shared_models"`, Cargo runs this script first and passes the path
//! as `DEP_SHARED_MODELS_FRAGMENTS`.

use std::path::PathBuf;

fn main() {
    let out_dir = PathBuf::from(std::env::var_os("OUT_DIR").expect("set by Cargo"));
    let fragments = out_dir.join("openapi_fragments.yaml");
    oas_forge::Generator::new()
        .input("src")
        .output_fragments(&fragments)
        .emit_cargo_directives(true)
        .generate()
        .expect("Failed to generate OpenAPI fragments");
    println!("cargo::metadata=fragments={}", fragments.display());
}
