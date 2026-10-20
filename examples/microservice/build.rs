//! Writes `openapi.yaml` from this crate's sources and the fragments of
//! `shared-models`. Its build script has run before this one (it declares
//! `links`), so `DEP_SHARED_MODELS_FRAGMENTS` names the finished file.

fn main() {
    let fragments = std::env::var("DEP_SHARED_MODELS_FRAGMENTS")
        .expect("set by the shared-models build script");
    oas_forge::Generator::new()
        .input("src")
        .include(fragments)
        .output("openapi.yaml")
        .emit_cargo_directives(true)
        .generate()
        .expect("Failed to generate OpenAPI spec");
}
