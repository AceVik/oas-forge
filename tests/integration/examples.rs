//! Golden tests: regenerating the `examples/` workspace must reproduce the
//! checked-in specification. Comparison is structural, so key order is
//! irrelevant. The build scripts themselves run in CI
//! (`cargo build --manifest-path examples/Cargo.toml`).

use crate::common::{Project, yaml};
use oas_forge::Generator;
use std::path::PathBuf;

fn example(rel: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("examples")
        .join(rel)
}

/// What the `shared-models` build script writes to its `OUT_DIR`.
fn shared_fragments(out: &Project) -> PathBuf {
    let path = out.path("openapi_fragments.yaml");
    Generator::new()
        .input(example("shared-models/src"))
        .output_fragments(&path)
        .generate()
        .unwrap();
    path
}

#[test]
fn shared_models_export_their_templates() {
    let out = Project::new();
    let fragments = yaml(&std::fs::read_to_string(shared_fragments(&out)).unwrap());
    let components = &fragments["components"];
    assert_eq!(
        components["x-oas-forge-templates"]["Paginated"]["params"],
        serde_json::json!(["T"]),
        "{fragments:#}"
    );
    assert_eq!(components["x-oas-forge-format"], 1);
}

#[test]
fn microservice_spec_matches_checked_in_file() {
    let out = Project::new();
    let fragments = shared_fragments(&out);
    Generator::new()
        .input(example("microservice/src"))
        .include(fragments)
        .output(out.path("openapi.yaml"))
        .generate()
        .unwrap();

    let checked_in = std::fs::read_to_string(example("microservice/openapi.yaml"))
        .expect("checked-in example output");
    assert_eq!(out.read_yaml("openapi.yaml"), yaml(&checked_in));
}
