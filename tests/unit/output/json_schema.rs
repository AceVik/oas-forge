//! `src/output/json_schema.rs`: the JSON Schema bundle.

use crate::common::{codes, spec, spec_of};
use oas_forge::output::OutputKind;
use serde_json::json;

#[test]
fn component_schemas_become_defs_with_rewritten_references() {
    let (spec, _project) = spec_of(&[(
        "lib.rs",
        "/// @openapi\nstruct User { manager: Option<User>, role: Role }\n/// @openapi\n#[serde(tag = \"t\")]\nenum Role { Admin { level: u8 } }",
    )]);
    let (bundle, diagnostics) = spec
        .render_with_diagnostics(OutputKind::JsonSchema)
        .unwrap();
    assert!(diagnostics.is_empty());
    assert_eq!(
        bundle["$schema"],
        "https://json-schema.org/draft/2020-12/schema"
    );
    let keys: Vec<_> = bundle.as_object().unwrap().keys().cloned().collect();
    assert_eq!(keys, ["$schema", "$defs"]);
    let names: Vec<_> = bundle["$defs"]
        .as_object()
        .unwrap()
        .keys()
        .cloned()
        .collect();
    assert_eq!(names, ["Role", "RoleAdmin", "User"], "sorted");
    assert_eq!(
        bundle["$defs"]["User"]["properties"]["manager"],
        json!({ "anyOf": [{ "$ref": "#/$defs/User" }, { "type": "null" }] })
    );
    assert_eq!(
        bundle["$defs"]["Role"]["discriminator"]["mapping"]["Admin"],
        "#/$defs/RoleAdmin"
    );
    assert!(
        spec::validate(&bundle).is_empty(),
        "{:?}",
        spec::validate(&bundle)
    );
}

#[test]
fn the_bundle_keeps_the_canonical_dialect_for_older_roots() {
    let (spec, _project) = spec_of(&[
        (
            "root.yaml",
            "openapi: 3.0.3\ninfo: { title: T, version: '1' }\npaths: {}",
        ),
        ("lib.rs", "/// @openapi\nstruct S { a: Option<String> }"),
    ]);
    assert_eq!(
        spec.document()["components"]["schemas"]["S"]["properties"]["a"],
        json!({ "type": "string", "nullable": true })
    );
    let bundle = spec.render(OutputKind::JsonSchema).unwrap();
    assert_eq!(
        bundle["$defs"]["S"]["properties"]["a"],
        json!({ "type": ["string", "null"] })
    );
}

#[test]
fn documents_without_schemas_give_an_empty_bundle() {
    let (spec, _project) = spec_of(&[("api.yaml", "paths: {}")]);
    assert_eq!(
        spec.render(OutputKind::JsonSchema).unwrap(),
        json!({ "$schema": "https://json-schema.org/draft/2020-12/schema", "$defs": {} })
    );
    assert!(codes(spec.diagnostics()).is_empty());
}
