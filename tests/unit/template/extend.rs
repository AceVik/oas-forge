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
fn acyclic_extension_fanout_is_bounded() {
    let mut reg = Registry::new();
    reg.add_fragment(
        "F0",
        Fragment {
            params: vec![],
            body: "{}".into(),
        },
    );
    for depth in 1..6 {
        reg.add_fragment(
            format!("F{depth}"),
            Fragment {
                params: vec![],
                body: format!("@extend F{}\n", depth - 1).repeat(10),
            },
        );
    }
    let (value, diagnostics) = apply(&reg, json!({ marker(1): "F5", "keep": true }));
    assert_eq!(value, json!({ "keep": true }));
    assert_eq!(codes(&diagnostics), [Code::ExpansionLimit]);
    assert!(diagnostics.has_errors());
    assert_eq!(diagnostics.iter().next().unwrap().location(), Some(&here()));
}

#[test]
fn extension_body_substitution_is_bounded_before_allocation() {
    let reg = registry(&[("Big", &["x"], &"{{x}}".repeat(4096))]);
    let (value, diagnostics) = apply(
        &reg,
        json!({ marker(1): format!("Big({})", "x".repeat(8192)) }),
    );
    assert_eq!(value, json!({}));
    assert_eq!(codes(&diagnostics), [Code::ExpansionLimit]);
}

#[test]
fn extension_text_budgets_cover_repeated_and_expanded_bodies() {
    let text = "#".repeat(4 * 1024 * 1024);
    let input = json!({ marker(1): "Big", marker(2): "Big", marker(3): "Big", marker(4): "Big" });
    for (body, variables) in [
        (format!("{text}\n{{}}"), Variables::new()),
        (
            "{{BIG}}\n{}".into(),
            Variables::from([("BIG".into(), text.clone())]),
        ),
    ] {
        let reg = registry(&[("Big", &[], &body)]);
        let (value, diagnostics) = apply_with(&reg, &variables, input.clone());
        assert_eq!(value, json!({}));
        assert_eq!(codes(&diagnostics), [Code::ExpansionLimit]);
    }
}

#[test]
fn an_extension_may_use_the_exact_text_budget() {
    let body = "#".repeat(8 * 1024 * 1024 - 3) + "\n{}";
    let reg = registry(&[("Exact", &[], &body)]);
    let (value, diagnostics) = apply(&reg, json!({marker(1): "Exact"}));
    assert_eq!(value, json!({}));
    assert!(diagnostics.is_empty(), "{diagnostics:?}");
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
    let reg = registry(&[("Tagged", &[], "tags:\n  - b\n  - a")]);
    let (value, _) = apply(&reg, json!({ marker(1): "Tagged", "tags": ["a"] }));
    assert_eq!(value, json!({ "tags": ["a", "b"] }));
}

/// A fragment cannot add an alternative to the local requirements, such as
/// `{}`, which admits anonymous requests.
#[test]
fn local_security_wins() {
    let reg = registry(&[("Open", &[], "security:\n  - {}\ndeprecated: true")]);
    let (value, _) = apply(
        &reg,
        json!({ marker(1): "Open", "security": [{ "basic": [] }] }),
    );
    assert_eq!(
        value,
        json!({ "security": [{ "basic": [] }], "deprecated": true })
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
        json!({ marker(1): "Missing", marker(2): "Broken", marker(3): "Needs", "keep": 1 }),
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
fn extra_arguments_are_reported() {
    let reg = registry(&[("One", &["a"], "x: '{{a}}'")]);
    let (value, diagnostics) = apply(&reg, json!({ marker(1): "One(1, \"2, 3\", 4)" }));
    assert_eq!(value, json!({ "x": "1" }));
    assert_eq!(codes(&diagnostics), [Code::ExtraFragmentArgument]);
}

#[test]
fn markers_apply_in_line_order_then_defaults_innermost_first() {
    let reg = registry(&[
        ("A", &[], "v: a"),
        ("B", &[], "v: b"),
        ("C", &[], "v: c"),
        ("D", &[], "v: d"),
    ]);
    let defaults = |module: u8, n: u8| format!("{EXTEND_KEY}defaults-{module}-{n}");
    // Keys in an order a map may end up in; the first applied wins.
    let cases = [
        (json!({ defaults(1, 1): "A", defaults(2, 1): "B" }), "b"),
        (
            json!({ defaults(2, 1): "B", marker(10): "C", marker(2): "D" }),
            "d",
        ),
        (json!({ marker(10): "C", defaults(1, 1): "A" }), "c"),
    ];
    for (value, expected) in cases {
        let (value, _) = apply(&reg, value);
        assert_eq!(value, json!({ "v": expected }));
    }
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

/// The lexical stage writes the fragment of an `@extend` line as a string
/// under the marker key. A marker with another value is not its own (a
/// source wrote it): it stays, so that the sweep of the finished document
/// reports it instead of this stage dropping it silently.
#[test]
fn a_marker_with_another_value_stays() {
    let (value, diagnostics) = apply(
        &registry(&[("Base", &[], "x-base: 1")]),
        json!({
            marker(1): "Base",
            marker(2): { "a": 1 },
            marker(3): 3,
            "nested": { marker(4): ["Base"] },
        }),
    );
    assert_eq!(
        value,
        json!({
            marker(2): { "a": 1 },
            marker(3): 3,
            "nested": { marker(4): ["Base"] },
            "x-base": 1,
        })
    );
    assert!(diagnostics.is_empty(), "{diagnostics:?}");
}
