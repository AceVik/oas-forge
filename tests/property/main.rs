//! Property-based tests: invariants that must hold for arbitrary input.
//!
//! Doc comments are free text written by users, so no stage of the pipeline
//! may panic on unexpected characters, unbalanced brackets or stray quotes.
//! Raise the number of cases with `PROPTEST_CASES=10000 cargo test --test property`.

#[path = "../common/mod.rs"]
mod common;

use common::{here, yaml};
use oas_forge::attrs::RenameRule;
use oas_forge::diagnostics::Diagnostics;
use oas_forge::dsl::{self, RouteSource};
use oas_forge::extract::extract_str;
use oas_forge::generator::generate_from_sources;
use oas_forge::merge::{Policy, merge};
use oas_forge::output::Format;
use oas_forge::schema::types::{TypeExpr, TypeMapper};
use oas_forge::schema::{dialect, nullable};
use oas_forge::source::{SourceFile, SourceKind};
use oas_forge::template::generics::{GenericRef, Instantiator};
use oas_forge::template::lexical::{self, Variables};
use oas_forge::template::refs::{self, KnownSchemas};
use oas_forge::template::{Blueprint, Fragment, Registry, extend};
use proptest::prelude::*;
use proptest::test_runner::FileFailurePersistence;
use serde_json::{Value, json};
use std::collections::HashSet;
use std::path::Path;

/// Text that is likely to hit parser edge cases: DSL sigils, brackets,
/// quotes, separators and multi-byte characters.
fn tricky_text() -> impl Strategy<Value = String> {
    let atoms = prop::sample::select(vec![
        "$",
        "$Page",
        "<",
        ">",
        "$Page<User>",
        "$Vec<",
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
        "---",
        "@insert",
        "@extend",
        "@openapi",
        "@route",
        "{{x}}",
        "\\$",
        "ä",
        "ß",
        "€",
        "–",
        "𝄞",
        "\u{3000}",
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
        "@openapi",
        "@openapi path",
        "@openapi document",
        "responses:",
        "x-extra:",
        "",
    ]);
    (directive, tricky_text()).prop_map(|(d, rest)| format!("{d} {rest}"))
}

fn json_value() -> impl Strategy<Value = Value> {
    let leaf = prop_oneof![
        any::<bool>().prop_map(Value::from),
        any::<i32>().prop_map(Value::from),
        Just(Value::Null),
        // Short words include YAML 1.1 booleans such as `yes` and `on`.
        "[a-z]{0,6}".prop_map(Value::from),
        tricky_text().prop_map(Value::from),
    ];
    leaf.prop_recursive(3, 24, 4, |inner| {
        prop_oneof![
            prop::collection::vec(inner.clone(), 0..4).prop_map(|items| {
                // Documents de-duplicate sequences, so generate unique items only.
                let mut seen = HashSet::new();
                Value::Array(
                    items
                        .into_iter()
                        .filter(|v| seen.insert(v.to_string()))
                        .collect(),
                )
            }),
            prop::collection::btree_map("[a-z$]{1,6}", inner, 0..4)
                .prop_map(|m| Value::Object(m.into_iter().collect())),
        ]
    })
}

/// `value` with every `<` in strings and keys replaced, so it names no generic.
fn without_angles(value: Value) -> Value {
    match value {
        Value::String(s) => Value::String(s.replace('<', "(")),
        Value::Array(items) => Value::Array(items.into_iter().map(without_angles).collect()),
        Value::Object(map) => Value::Object(
            map.into_iter()
                .map(|(k, v)| (k.replace('<', "("), without_angles(v)))
                .collect(),
        ),
        other => other,
    }
}

fn registry() -> Registry {
    let mut registry = Registry::new();
    registry.add_blueprint(
        "Page",
        Blueprint {
            params: vec!["T".into()],
            schema: json!({ "items": "$T" }),
        },
    );
    registry.add_fragment(
        "Frag",
        Fragment {
            params: vec!["x".into()],
            body: "a: {{x}}".into(),
        },
    );
    registry
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
    fn type_expressions_never_panic(text in tricky_text()) {
        if let Some(expr) = TypeExpr::parse(&text) {
            let _ = expr.to_string();
            let _ = TypeMapper::new().map(&expr);
        }
        if let Some(generic) = GenericRef::parse(&text) {
            prop_assert!(!generic.concrete_name().contains('<'));
        }
    }

    #[test]
    fn instantiation_never_panics(value in json_value(), text in tricky_text()) {
        let registry = registry();
        let mut value = json!({ "a": value, "b": { "$ref": text.clone() }, "c": text });
        let mut instantiator = Instantiator::new(&registry);
        instantiator.apply(&mut value, &here(), &mut Diagnostics::new());
        let _ = instantiator.into_concrete();
    }

    #[test]
    fn values_without_generics_are_not_instantiated(value in json_value().prop_map(without_angles)) {
        let registry = registry();
        let mut copy = value.clone();
        Instantiator::new(&registry).apply(&mut copy, &here(), &mut Diagnostics::new());
        prop_assert_eq!(copy, value);
    }

    #[test]
    fn references_without_known_schemas_are_kept(value in json_value()) {
        // Only `{"$ref": "$<built-in type>"}` objects are rewritten.
        prop_assume!(!value.to_string().contains("\"$ref\""));
        let mut copy = value.clone();
        let (known, registry) = (KnownSchemas::default(), Registry::new());
        let targets = refs::Targets { known: &known, registry: &registry, types: TypeMapper::builtin() };
        refs::resolve(&mut copy, targets, &here(), &mut Diagnostics::new());
        prop_assert_eq!(copy, value);
    }

    #[test]
    fn lexical_expansion_never_panics(text in tricky_text()) {
        let mut variables = Variables::new();
        variables.insert("x".into(), "va'lue".into());
        let _ = lexical::expand(&text, &registry(), &variables, &here(), &mut Diagnostics::new());
    }

    #[test]
    fn plain_text_is_not_expanded(text in "[^@{\\\\\r]*") {
        let out = lexical::expand(&text, &registry(), &Variables::new(), &here(), &mut Diagnostics::new());
        prop_assert_eq!(out, text);
    }

    #[test]
    fn extend_never_panics(value in json_value()) {
        let mut value = json!({ "x-oas-forge-extend-1": "Frag(1)", "nested": value });
        extend::apply(&mut value, &registry(), &here(), &mut Diagnostics::new());
        prop_assert!(value.get("x-oas-forge-extend-1").is_none());
    }

    #[test]
    fn route_dsl_never_panics(lines in prop::collection::vec(dsl_line(), 0..8)) {
        let mut all = vec!["@route GET /x".to_string()];
        all.extend(lines);
        let compiled = dsl::compile(
            RouteSource::new(&all, &here()),
            &mut Diagnostics::new(),
        );
        if let Some(compiled) = compiled {
            prop_assert!(compiled.document["paths"][&compiled.path][&compiled.method].is_object());
        }
    }

    #[test]
    fn extraction_never_panics_on_doc_comments(docs in prop::collection::vec(tricky_text(), 0..6)) {
        let attrs: Vec<syn::Attribute> = docs.iter().map(|d| syn::parse_quote!(#[doc = #d])).collect();
        let file: syn::File = syn::parse_quote! {
            #(#attrs)*
            /// @openapi
            mod api {
                #(#attrs)*
                /// @openapi
                struct Model { field: String }

                #(#attrs)*
                /// @openapi
                #[serde(tag = "t")]
                enum Choice { A { x: u8 }, B(String) }

                #(#attrs)*
                fn handler() {}
            }
        };
        let text = quote::ToTokens::to_token_stream(&file).to_string();
        let mut diagnostics = Diagnostics::new();
        extract_str(Path::new("prop.rs"), &text, TypeMapper::builtin(), &mut diagnostics).expect("generated Rust is valid");
    }

    #[test]
    fn the_pipeline_never_panics(docs in prop::collection::vec(tricky_text(), 0..4)) {
        let attrs: Vec<syn::Attribute> = docs.iter().map(|d| syn::parse_quote!(#[doc = #d])).collect();
        let file: syn::File = syn::parse_quote! {
            #(#attrs)*
            fn handler() {}
            #(#attrs)*
            /// @route GET /x
            fn route() {}
        };
        let text = quote::ToTokens::to_token_stream(&file).to_string();
        let _ = generate_from_sources(vec![SourceFile::new("p.rs", SourceKind::Rust, text)], &Variables::new());
    }

    #[test]
    fn merging_into_an_empty_document_reproduces_the_source(
        doc in prop::collection::btree_map("[a-z]{1,6}", json_value(), 0..4)
    ) {
        let doc = Value::Object(doc.into_iter().collect());
        for policy in [Policy::Document, Policy::Operation, Policy::Extend, Policy::Override] {
            let mut target = json!({});
            prop_assert!(merge(&mut target, doc.clone(), policy).is_empty());
            prop_assert_eq!(&target, &doc);
        }
    }

    #[test]
    fn merging_a_document_with_itself_changes_nothing(value in json_value()) {
        for policy in [Policy::Document, Policy::Operation, Policy::Extend, Policy::Override] {
            let mut target = value.clone();
            prop_assert!(merge(&mut target, value.clone(), policy).is_empty());
            prop_assert_eq!(&target, &value);
        }
    }

    #[test]
    fn long_and_multi_line_strings_round_trip(
        words in prop::collection::vec(prop_oneof![
            "[a-z]{1,12}".prop_map(String::from),
            Just(" ".to_string()), Just("  ".to_string()), Just("\n".to_string()),
            Just("\n\n".to_string()), Just("\t".to_string()), Just("   \n".to_string()),
            Just("- ".to_string()), Just("# ".to_string()), Just(": ".to_string()),
        ], 0..60)
    ) {
        let value = json!({ "description": words.concat(), "nested": { "list": [words.concat()] } });
        let text = Format::Yaml.render(&value);
        prop_assert_eq!(yaml(&text), value, "{}", text);
    }

    #[test]
    fn rendered_documents_parse_back_unchanged(value in json_value()) {
        prop_assert_eq!(yaml(&Format::Yaml.render(&value)), value.clone());
        prop_assert_eq!(serde_json::from_str::<Value>(&Format::Json.render(&value)).unwrap(), value);
    }

    #[test]
    fn nullable_is_idempotent_and_accepts_null(value in json_value()) {
        let once = nullable(value.clone());
        prop_assert_eq!(nullable(once.clone()), once.clone());
        if value.is_object() && value != json!({}) {
            let text = once.to_string();
            prop_assert!(text.contains("null"), "{}", text);
        }
    }

    #[test]
    fn lowering_to_openapi_30_never_panics(value in json_value()) {
        let mut document = json!({
            "openapi": "3.0.3",
            "components": { "schemas": { "A": nullable(value.clone()), "B": value } },
            "paths": { "/x": { "get": { "responses": { "200": { "content": { "a/b": { "schema": { "type": ["string", "null"] } } } } } } } },
        });
        dialect::lower(&mut document, dialect::Dialect::OpenApi30, &mut Diagnostics::new());
        let lowered = &document["paths"]["/x"]["get"]["responses"]["200"]["content"]["a/b"]["schema"];
        prop_assert_eq!(lowered, &json!({ "type": "string", "nullable": true }));
    }

    #[test]
    fn lowering_casing_rules_are_idempotent(ident in "[A-Za-z][A-Za-z0-9_]{0,12}") {
        for rule in [RenameRule::Lower, RenameRule::Upper, RenameRule::Snake, RenameRule::Kebab] {
            let once = rule.apply_to_field(&ident);
            prop_assert_eq!(rule.apply_to_field(&once), once.clone(), "{}", rule);
            let once = rule.apply_to_variant(&ident);
            prop_assert_eq!(rule.apply_to_variant(&once), once.clone(), "{}", rule);
        }
    }

    #[test]
    fn screaming_rules_contain_no_lowercase(ident in "[A-Za-z][A-Za-z0-9_]{0,12}") {
        for rule in [RenameRule::ScreamingSnake, RenameRule::ScreamingKebab, RenameRule::Upper] {
            for out in [rule.apply_to_field(&ident), rule.apply_to_variant(&ident)] {
                prop_assert!(!out.chars().any(char::is_lowercase), "{} -> {}", rule, out);
            }
        }
    }

    #[test]
    fn camel_and_pascal_case_drop_underscores_of_fields(ident in "[a-z][a-z0-9_]{0,12}") {
        for rule in [RenameRule::Camel, RenameRule::Pascal] {
            prop_assert!(!rule.apply_to_field(&ident).contains('_'), "{}", rule);
        }
    }

    #[test]
    fn renaming_never_panics(ident in any::<String>()) {
        for (_, rule) in RenameRule::ALL {
            let _ = rule.apply_to_field(&ident);
            let _ = rule.apply_to_variant(&ident);
        }
    }
}
