//! `src/output/openapi30.rs`: the OpenAPI 3.0.3 export.

use crate::common::{codes, spec, spec_of};
use oas_forge::diagnostics::Code;
use oas_forge::error::Error;
use oas_forge::output::OutputKind;
use serde_json::json;

const ROOT_32: &str = r#"
openapi: 3.2.0
$self: https://example.com/api
jsonSchemaDialect: https://spec.openapis.org/oas/3.1/dialect/base
info:
  title: T
  summary: Short
  version: '1'
  license: { name: MIT, identifier: MIT }
tags:
  - { name: A, summary: S, kind: nav }
components:
  pathItems: { P: {} }
  securitySchemes:
    tls: { type: mutualTLS }
    key: { type: apiKey, in: header, name: X }
  responses:
    Gone: { summary: G, description: Gone }
"#;

#[test]
fn newer_fields_are_removed_and_reported_once() {
    let (spec, _project) = spec_of(&[
        ("root.yaml", ROOT_32),
        (
            "api.rs",
            "/// @route GET /a\n/// responses:\n///   '200': { summary: S, description: OK }\nfn a() {}\n/// @openapi\nstruct S { n: Option<u8> }",
        ),
    ]);
    let (document, diagnostics) = spec.render_with_diagnostics(OutputKind::OpenApi30).unwrap();
    assert_eq!(document["openapi"], "3.0.3");
    for pointer in [
        "/$self",
        "/jsonSchemaDialect",
        "/info/summary",
        "/info/license/identifier",
        "/tags/0/summary",
        "/tags/0/kind",
        "/components/pathItems",
        "/components/securitySchemes/tls",
        "/components/responses/Gone/summary",
        "/paths/~1a/get/responses/200/summary",
    ] {
        assert!(document.pointer(pointer).is_none(), "{pointer}");
    }
    assert!(
        document
            .pointer("/components/securitySchemes/key")
            .is_some()
    );
    assert_eq!(
        document.pointer("/components/schemas/S/properties/n/nullable"),
        Some(&json!(true))
    );
    assert_eq!(codes(&diagnostics), [Code::UnsupportedFeature]);
    assert!(
        spec::validate(&document).is_empty(),
        "{:?}",
        spec::validate(&document)
    );
    // Unconfigured exports do not add findings to the run.
    assert!(spec.diagnostics().is_empty(), "{:?}", spec.diagnostics());
}

#[test]
fn paths_are_added_when_missing_and_a_root_is_required() {
    let (spec, _project) = spec_of(&[(
        "root.yaml",
        "openapi: 3.1.0\ninfo: { title: T, version: '1' }\ncomponents: {}",
    )]);
    assert_eq!(
        spec.render(OutputKind::OpenApi30).unwrap()["paths"],
        json!({})
    );
    let (spec, _project) = spec_of(&[("lib.rs", "/// @openapi\nstruct S;")]);
    assert!(matches!(
        spec.render(OutputKind::OpenApi30),
        Err(Error::NoRootFound)
    ));
    assert!(matches!(
        spec.render(OutputKind::Swagger20),
        Err(Error::NoRootFound)
    ));
}

#[test]
fn configured_exports_report_into_the_run() {
    let project = crate::common::Project::new();
    project
        .file(
            "src/root.yaml",
            "openapi: 3.1.0\ninfo: { title: T, version: '1' }",
        )
        .file("src/api.rs", "/// @webhook hook\nfn a() {}");
    let spec = oas_forge::Generator::new()
        .input(project.path("src"))
        .output_openapi_30(project.path("out/api30.yaml"))
        .build()
        .unwrap();
    assert_eq!(codes(spec.diagnostics()), [Code::UnsupportedFeature]);
    spec.write().unwrap();
    assert_eq!(project.read_yaml("out/api30.yaml")["openapi"], "3.0.3");
}

#[test]
fn a_license_identifier_that_repeats_the_name_is_dropped_silently() {
    for (license, findings) in [
        ("{ name: MIT, identifier: MIT }", 0),
        ("{ name: MIT License, identifier: MIT }", 1),
    ] {
        let (spec, _project) = spec_of(&[(
            "root.yaml",
            &format!("openapi: 3.1.0\ninfo: {{ title: T, version: '1', license: {license} }}"),
        )]);
        let (document, diagnostics) = spec.render_with_diagnostics(OutputKind::OpenApi30).unwrap();
        assert!(document.pointer("/info/license/identifier").is_none());
        assert_eq!(diagnostics.len(), findings, "{license}: {diagnostics:?}");
    }
}

/// A nullable alternative with a `type` array, and one that allows only
/// null, were written as `type: [integer, string]` and `anyOf: []`, which
/// OpenAPI 3.0 rejects.
#[test]
fn nullable_alternatives_export_valid_schemas() {
    let (spec, _project) = spec_of(&[(
        "root.yaml",
        "openapi: 3.1.0\ninfo: { title: T, version: '1' }\ncomponents:\n  schemas:\n    A: { anyOf: [ { type: [integer, string] }, { type: 'null' } ] }\n    E: { anyOf: [ { type: 'null' } ] }",
    )]);
    let (document, _) = spec.render_with_diagnostics(OutputKind::OpenApi30).unwrap();
    let errors = spec::validate(&document);
    assert!(errors.is_empty(), "{errors:?}");
    assert_eq!(
        document["components"]["schemas"],
        json!({
            "A": { "anyOf": [{ "type": "integer" }, { "type": "string" }], "nullable": true },
            "E": { "enum": [null], "nullable": true }
        })
    );
}
