//! Golden tests: regenerating the `examples/` workspace must reproduce the
//! checked-in output files. Comparison is structural, so key order is irrelevant.

use crate::common::{Project, yaml};
use oas_forge::Generator;
use std::path::PathBuf;

fn example(rel: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("examples")
        .join(rel)
}

fn checked_in(rel: &str) -> serde_json::Value {
    yaml(&std::fs::read_to_string(example(rel)).expect("checked-in example output"))
}

#[test]
fn shared_models_fragments_match_checked_in_file() {
    let out = Project::new();

    Generator::new()
        .input(example("shared-models/src"))
        .output_fragments(out.path("shared_openapi_fragments.yaml"))
        .generate()
        .unwrap();

    assert_eq!(
        out.read_yaml("shared_openapi_fragments.yaml"),
        checked_in("shared-models/shared_openapi_fragments.yaml")
    );
}

#[test]
fn microservice_spec_matches_checked_in_file() {
    let out = Project::new();

    Generator::new()
        .input(example("microservice/src"))
        .include(example("shared-models/shared_openapi_fragments.yaml"))
        .output(out.path("openapi.yaml"))
        .generate()
        .unwrap();

    assert_eq!(
        out.read_yaml("openapi.yaml"),
        checked_in("microservice/openapi.yaml")
    );
}
