use crate::common::{codes, here};
use oas_forge::diagnostics::{Code, Diagnostics};
use oas_forge::template::registry::{FORMAT_KEY, FORMAT_VERSION, FRAGMENTS_KEY, TEMPLATES_KEY};
use oas_forge::template::{Blueprint, Fragment, Registry};
use serde_json::json;

fn fragment(params: &[&str], body: &str) -> Fragment {
    Fragment {
        params: params.iter().map(|p| p.to_string()).collect(),
        body: body.into(),
    }
}

#[test]
fn new_registry_is_empty() {
    let registry = Registry::new();
    assert!(!registry.has_templates());
    assert_eq!(registry.fragments().count(), 0);
    assert_eq!(registry.blueprints().count(), 0);
    assert!(registry.export().is_empty());
}

#[test]
fn later_definitions_replace_earlier_ones() {
    let mut registry = Registry::new();
    registry.add_fragment("F", fragment(&["a"], "old"));
    registry.add_fragment("F", fragment(&["b"], "new"));
    registry.add_blueprint(
        "Page",
        Blueprint {
            params: vec!["T".into()],
            schema: json!({ "items": "$T" }),
        },
    );

    assert_eq!(registry.fragment("F"), Some(&fragment(&["b"], "new")));
    assert_eq!(registry.blueprint("Page").unwrap().params, ["T"]);
    assert!(registry.blueprint("Missing").is_none());
    assert!(registry.has_templates());
}

#[test]
fn iteration_is_sorted_by_name() {
    let mut registry = Registry::new();
    for name in ["b", "a", "c"] {
        registry.add_fragment(name, fragment(&[], ""));
    }
    let names: Vec<_> = registry.fragments().map(|(n, _)| n.clone()).collect();
    assert_eq!(names, ["a", "b", "c"]);
}

#[test]
fn export_and_import_round_trip() {
    let mut registry = Registry::new();
    registry.add_fragment(
        "Problem",
        fragment(&["status"], "description: Problem {{status}}"),
    );
    registry.add_blueprint(
        "Page",
        Blueprint {
            params: vec!["T".into()],
            schema: json!({ "type": "object", "items": { "$ref": "$T" } }),
        },
    );

    let exported = registry.export();
    assert_eq!(exported[FORMAT_KEY], json!(FORMAT_VERSION));
    assert_eq!(exported[TEMPLATES_KEY]["Page"]["params"], json!(["T"]));
    assert_eq!(
        exported[FRAGMENTS_KEY]["Problem"]["body"],
        "description: Problem {{status}}"
    );

    let mut document = json!({ "components": serde_json::Value::Object(exported) });
    document["components"]["schemas"] = json!({ "Kept": {} });
    let mut imported = Registry::new();
    let mut diagnostics = Diagnostics::new();
    imported.import(&mut document, &here(), &mut diagnostics);

    assert!(diagnostics.is_empty());
    assert_eq!(imported, registry);
    assert_eq!(
        document,
        json!({ "components": { "schemas": { "Kept": {} } } }),
        "transport keys are removed"
    );
}

#[test]
fn import_accepts_0_1_documents_without_format_marker() {
    let mut document = json!({ "components": {
        TEMPLATES_KEY: { "Page": { "params": ["T"], "body": "items: $T\n" } },
        FRAGMENTS_KEY: { "Err": { "body": "description: x" } },
    }});
    let mut registry = Registry::new();
    let mut diagnostics = Diagnostics::new();
    registry.import(&mut document, &here(), &mut diagnostics);
    assert_eq!(
        registry.blueprint("Page").unwrap().schema,
        json!({ "items": "$T" })
    );
    assert_eq!(
        registry.fragment("Err"),
        Some(&fragment(&[], "description: x"))
    );
    assert!(diagnostics.is_empty());
}

#[test]
fn import_reports_unknown_formats_and_invalid_bodies() {
    let mut document = json!({ "components": {
        FORMAT_KEY: 99,
        TEMPLATES_KEY: { "Bad": { "params": [], "body": "a: [unclosed" }, "NoBody": { "params": [] } },
        FRAGMENTS_KEY: "not a map",
    }});
    let mut registry = Registry::new();
    let mut diagnostics = Diagnostics::new();
    registry.import(&mut document, &here(), &mut diagnostics);
    assert_eq!(
        codes(&diagnostics),
        [Code::UnknownTemplateFormat, Code::InvalidFragment]
    );
    assert!(!registry.has_templates());
}

#[test]
fn import_ignores_documents_without_components() {
    let mut document = json!({ "paths": {} });
    let mut registry = Registry::new();
    registry.import(&mut document, &here(), &mut Diagnostics::new());
    assert_eq!(document, json!({ "paths": {} }));
}
