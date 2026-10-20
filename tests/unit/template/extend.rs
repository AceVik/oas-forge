use crate::common::{codes, here};
use oas_forge::diagnostics::{Code, Diagnostics};
use oas_forge::template::lexical::EXTEND_KEY;
use oas_forge::template::{Fragment, Registry, Variables, extend};
use serde_json::{Value, json};

fn registry(fragments: &[(&str, &[&str], &str)]) -> Registry {
    let mut registry = Registry::new();
    for (name, params, body) in fragments {
        registry.add_fragment(
            *name,
            Fragment {
                params: params.iter().map(|p| p.to_string()).collect(),
                body: body.to_string(),
            },
        );
    }
    registry
}

fn marker(n: usize) -> String {
    format!("{EXTEND_KEY}{n}")
}

fn apply(reg: &Registry, value: Value) -> (Value, Diagnostics) {
    apply_with(reg, &Variables::new(), value)
}

fn apply_with(reg: &Registry, variables: &Variables, mut value: Value) -> (Value, Diagnostics) {
    let mut diagnostics = Diagnostics::new();
    extend::apply(&mut value, reg, variables, &here(), &mut diagnostics);
    (value, diagnostics)
}

#[test]
fn merges_fragment_into_the_containing_mapping() {
    let reg = registry(&[(
        "Base",
        &[],
        "responses:\n  '404':\n    description: Not Found",
    )]);
    let (value, diagnostics) = apply(
        &reg,
        json!({ "get": { marker(1): "Base", "responses": { "200": { "description": "OK" } } } }),
    );
    assert_eq!(
        value,
        json!({ "get": { "responses": {
            "200": { "description": "OK" },
            "404": { "description": "Not Found" }
        }}})
    );
    assert!(diagnostics.is_empty());
}

#[test]
fn local_values_win_over_the_fragment() {
    // Regression: the fragment used to overwrite local scalars.
    let reg = registry(&[("Base", &[], "summary: From fragment\ndeprecated: true")]);
    let (value, _) = apply(&reg, json!({ marker(1): "Base", "summary": "Local" }));
    assert_eq!(value, json!({ "summary": "Local", "deprecated": true }));
}

#[test]
fn arrays_are_combined() {
    let reg = registry(&[("Secured", &[], "security:\n  - oidc: []\n  - basic: []")]);
    let (value, _) = apply(
        &reg,
        json!({ marker(1): "Secured", "security": [{ "basic": [] }] }),
    );
    assert_eq!(
        value,
        json!({ "security": [{ "basic": [] }, { "oidc": [] }] })
    );
}

#[test]
fn substitutes_arguments_including_quoted_ones() {
    let reg = registry(&[("Secured", &["role"], "security:\n  - oidc: [{{role}}]")]);
    let (value, _) = apply(&reg, json!({ marker(1): "Secured(\"admin\")" }));
    assert_eq!(value, json!({ "security": [{ "oidc": ["admin"] }] }));
}

#[test]
fn multiple_markers_in_one_mapping() {
    let reg = registry(&[("A", &[], "a: 1"), ("B", &[], "b: 2")]);
    let (value, _) = apply(&reg, json!({ marker(1): "A", marker(2): "B()" }));
    assert_eq!(value, json!({ "a": 1, "b": 2 }));
}

#[test]
fn works_inside_sequences_and_nested_fragments() {
    let reg = registry(&[
        ("Typed", &[], "schema: { type: string }"),
        (
            "Outer",
            &[],
            &format!("{}9: Typed\nrequired: true", EXTEND_KEY),
        ),
    ]);
    let (value, _) = apply(
        &reg,
        json!([{ "name": "a", marker(1): "Outer" }, { "name": "b" }]),
    );
    assert_eq!(
        value,
        json!([
            { "name": "a", "schema": { "type": "string" }, "required": true },
            { "name": "b" }
        ])
    );
}

#[test]
fn problems_are_reported_and_the_marker_dropped() {
    let reg = registry(&[
        ("Broken", &[], "a: [unclosed"),
        ("Needs", &["x"], "a: Value {{x}}"),
    ]);
    let (value, diagnostics) = apply(
        &reg,
        json!({ marker(1): "Missing", marker(2): "Broken", marker(3): "Needs", "keep": 1, marker(4): 5 }),
    );
    assert_eq!(value, json!({ "keep": 1, "a": "Value {{x}}" }));
    assert_eq!(
        codes(&diagnostics),
        [
            Code::UnknownFragment,
            Code::InvalidFragment,
            Code::MissingFragmentArgument
        ]
    );
}

#[test]
fn fragment_bodies_go_through_the_text_stage() {
    let reg = registry(&[
        (
            "Versioned",
            &[],
            "x-version: {{VERSION}}\nx-price: \\$Price",
        ),
        ("Paging", &[], "- name: page\n  in: query"),
        ("WithPaging", &[], "parameters:\n  @insert Paging"),
        ("Outer", &["role"], "x-outer: '{{role}}'\n@extend Versioned"),
    ]);
    let variables = Variables::from([("VERSION".to_string(), "1.10".to_string())]);
    let (value, diagnostics) = apply_with(
        &reg,
        &variables,
        json!({ marker(1): "WithPaging", marker(2): "Outer(admin)" }),
    );
    assert!(diagnostics.is_empty(), "{diagnostics:?}");
    let mut value = value;
    oas_forge::template::refs::unescape(&mut value);
    assert_eq!(
        value,
        json!({
            "parameters": [{ "name": "page", "in": "query" }],
            "x-outer": "admin",
            "x-version": "1.10",
            "x-price": "$Price",
        })
    );
}

#[test]
fn cycles_are_reported_once_and_skipped() {
    let reg = registry(&[
        ("A", &[], "a: 1\n@extend B"),
        ("B", &[], "b: 2\n@extend A\n@extend A"),
        ("Self", &[], "s: 1\n@extend Self"),
    ]);
    let (value, diagnostics) = apply(&reg, json!({ marker(1): "A", marker(2): "Self" }));
    assert_eq!(value, json!({ "a": 1, "b": 2, "s": 1 }));
    assert_eq!(
        codes(&diagnostics),
        [Code::FragmentCycle, Code::FragmentCycle]
    );
    assert!(
        diagnostics
            .iter()
            .any(|d| d.message().contains("A → B → A"))
    );
}

#[test]
fn nesting_is_bounded() {
    let fragments: Vec<(String, String)> = (0..10)
        .map(|i| {
            let body = if i < 9 {
                format!("f{i}: 1\n@extend F{}", i + 1)
            } else {
                format!("f{i}: 1")
            };
            (format!("F{i}"), body)
        })
        .collect();
    let list: Vec<(&str, &[&str], &str)> = fragments
        .iter()
        .map(|(name, body)| (name.as_str(), &[][..], body.as_str()))
        .collect();
    let (value, diagnostics) = apply(&registry(&list), json!({ marker(1): "F0" }));
    assert_eq!(value.as_object().unwrap().len(), 8);
    assert_eq!(codes(&diagnostics), [Code::UnknownFragment]);
}
