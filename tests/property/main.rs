//! Property-based tests: invariants that must hold for arbitrary input.
//!
//! Doc comments are free text written by users, so no stage of the pipeline
//! may panic on unexpected characters, unbalanced brackets or stray quotes.
//! Raise the number of cases with `PROPTEST_CASES=10000 cargo test --test property`.

#[path = "../common/mod.rs"]
mod common;

use common::{snippet, yaml};
use oas_forge::doc_parser::apply_casing;
use oas_forge::dsl::parse_route_dsl;
use oas_forge::generics::Monomorphizer;
use oas_forge::index::Registry;
use oas_forge::merger::merge_openapi;
use oas_forge::preprocessor::preprocess;
use oas_forge::scanner::substitute_smart_references;
use oas_forge::visitor::OpenApiVisitor;
use proptest::prelude::*;
use proptest::test_runner::FileFailurePersistence;
use serde_json::Value;
use std::collections::HashSet;
use syn::visit::Visit;

/// Text that is likely to hit parser edge cases: DSL sigils, brackets,
/// quotes, separators and multi-byte characters.
fn tricky_text() -> impl Strategy<Value = String> {
    let atoms = prop::sample::select(vec![
        "$",
        "$Page",
        "<",
        ">",
        "$Page<User>",
        "{",
        "}",
        "(",
        ")",
        "[",
        "]",
        "\"",
        "'",
        ":",
        ",",
        " ",
        "  ",
        "\n",
        "-",
        "@insert",
        "@extend",
        "{{x}}",
        "ä",
        "ß",
        "€",
        "–",
        "𝄞",
        "User",
        "tags:",
        "{id: u32 \"Id\"}",
    ]);
    prop::collection::vec(
        prop_oneof![atoms.prop_map(str::to_string), any::<String>()],
        0..12,
    )
    .prop_map(|parts| parts.concat())
}

fn dsl_line() -> impl Strategy<Value = String> {
    let directive = prop::sample::select(vec![
        "@route GET",
        "@route POST /x",
        "@return 200:",
        "@return",
        "@body",
        "@security",
        "@tag",
        "@query-param",
        "@path-param",
        "@header-param",
        "@cookie-param",
        "responses:",
        "",
    ]);
    (directive, tricky_text()).prop_map(|(d, rest)| format!("{d} {rest}"))
}

fn json_value() -> impl Strategy<Value = Value> {
    let leaf = prop_oneof![
        any::<bool>().prop_map(Value::from),
        any::<i32>().prop_map(Value::from),
        "[a-z]{0,6}".prop_map(Value::from),
    ];
    leaf.prop_recursive(3, 24, 4, |inner| {
        prop_oneof![
            prop::collection::vec(inner.clone(), 0..4).prop_map(|items| {
                // The merger deduplicates sequences, so generate unique items only.
                let mut seen = HashSet::new();
                Value::Array(
                    items
                        .into_iter()
                        .filter(|v| seen.insert(v.to_string()))
                        .collect(),
                )
            }),
            prop::collection::btree_map("[a-z]{1,6}", inner, 0..4)
                .prop_map(|m| Value::Object(m.into_iter().collect())),
        ]
    })
}

proptest! {
    // Failing inputs are recorded here and replayed first on every run.
    #![proptest_config(ProptestConfig {
        failure_persistence: Some(Box::new(FileFailurePersistence::Direct(
            "tests/property/regressions.txt",
        ))),
        ..ProptestConfig::default()
    })]

    #[test]
    fn monomorphizer_never_panics(text in tricky_text()) {
        let mut registry = Registry::new();
        registry.insert_blueprint("Page".into(), vec!["T".into()], "items: $T".into());
        Monomorphizer::new(&mut registry).process(&text);
    }

    #[test]
    fn monomorphizer_keeps_text_without_generics(text in any::<String>()) {
        prop_assume!(!text.contains('<'));
        let mut registry = Registry::new();
        prop_assert_eq!(Monomorphizer::new(&mut registry).process(&text), text);
    }

    #[test]
    fn smart_references_keep_text_without_known_schemas(text in tricky_text()) {
        prop_assert_eq!(substitute_smart_references(&text, &HashSet::new()), text);
    }

    #[test]
    fn route_dsl_never_panics(lines in prop::collection::vec(dsl_line(), 0..8)) {
        let mut all = vec!["@route GET /x".to_string()];
        all.extend(lines);
        if let Some(generated) = parse_route_dsl(&all, "op") {
            prop_assert!(serde_yaml_ng::from_str::<Value>(&generated).is_ok());
        }
    }

    #[test]
    fn preprocessor_never_panics(text in tricky_text()) {
        let mut registry = Registry::new();
        registry.insert_fragment("Frag".into(), vec!["x".into()], "a: {{x}}".into());
        preprocess(&text, &registry);
    }

    #[test]
    fn visitor_never_panics_on_doc_comments(docs in prop::collection::vec(tricky_text(), 0..6)) {
        let attrs: Vec<syn::Attribute> = docs
            .iter()
            .map(|d| syn::parse_quote!(#[doc = #d]))
            .collect();
        let file: syn::File = syn::parse_quote! {
            #(#attrs)*
            /// @openapi
            mod api {
                #(#attrs)*
                /// @openapi
                struct Model { field: String }

                #(#attrs)*
                fn handler() {}
            }
        };
        OpenApiVisitor::default().visit_file(&file);
    }

    #[test]
    fn merging_a_single_fragment_reproduces_it(doc in prop::collection::btree_map("[a-z]{1,6}", json_value(), 0..4)) {
        let doc = Value::Object(doc.into_iter().collect());
        prop_assume!(doc.get("openapi").is_none());
        let text = serde_yaml_ng::to_string(&doc).unwrap();

        let merged = merge_openapi(vec![snippet(&text)]).unwrap();
        let merged_text = serde_yaml_ng::to_string(&merged).unwrap();

        prop_assert_eq!(yaml(&merged_text), doc);
    }

    #[test]
    fn lowering_casing_rules_are_idempotent(ident in "[A-Za-z][A-Za-z0-9_]{0,12}") {
        // SCREAMING_* rules are not: like serde, they split every capital again.
        for rule in ["lowercase", "UPPERCASE", "snake_case", "kebab-case"] {
            let once = apply_casing(&ident, rule);
            prop_assert_eq!(apply_casing(&once, rule), once.clone(), "{}", rule);
        }
    }

    #[test]
    fn screaming_rules_contain_no_lowercase(ident in "[A-Za-z][A-Za-z0-9_]{0,12}") {
        for rule in ["SCREAMING_SNAKE_CASE", "SCREAMING-KEBAB-CASE"] {
            let out = apply_casing(&ident, rule);
            prop_assert!(!out.chars().any(|c| c.is_lowercase()), "{} -> {}", rule, out);
        }
    }

    #[test]
    fn camel_and_pascal_case_drop_underscores(ident in "[a-z][a-z0-9_]{0,12}") {
        for rule in ["camelCase", "PascalCase"] {
            prop_assert!(!apply_casing(&ident, rule).contains('_'), "{}", rule);
        }
    }
}
