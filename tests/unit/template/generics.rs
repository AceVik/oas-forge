use crate::common::{codes, here};
use oas_forge::diagnostics::{Code, Diagnostics};
use oas_forge::template::generics::{GenericArg, GenericRef, Instantiator};
use oas_forge::template::{Blueprint, Registry};
use serde_json::{Value, json};
use std::collections::BTreeMap;

fn registry(blueprints: &[(&str, &[&str], Value)]) -> Registry {
    let mut registry = Registry::new();
    for (name, params, schema) in blueprints {
        registry.add_blueprint(
            *name,
            Blueprint {
                params: params.iter().map(|p| p.to_string()).collect(),
                schema: schema.clone(),
            },
        );
    }
    registry
}

/// Instantiates the generics in `value`; returns the result, the concrete
/// schemas and the findings.
fn run(reg: &Registry, mut value: Value) -> (Value, BTreeMap<String, Value>, Diagnostics) {
    let mut diagnostics = Diagnostics::new();
    let mut instantiator = Instantiator::new(reg);
    instantiator.apply(&mut value, &here(), &mut diagnostics);
    let concrete = instantiator
        .into_concrete()
        .into_iter()
        .map(|(name, (schema, _))| (name, schema))
        .collect();
    (value, concrete, diagnostics)
}

mod generic_ref {
    use super::*;

    fn name(n: &str) -> GenericArg {
        GenericArg::Name(n.to_string())
    }

    #[test]
    fn parses_names_and_nested_arguments() {
        assert_eq!(
            GenericRef::parse("$Page<User>"),
            Some(GenericRef {
                name: "Page".into(),
                args: vec![name("User")]
            })
        );
        assert_eq!(
            GenericRef::parse(" $Map< String , $Pair<A, B> > "),
            Some(GenericRef {
                name: "Map".into(),
                args: vec![
                    name("String"),
                    GenericArg::Generic(GenericRef {
                        name: "Pair".into(),
                        args: vec![name("A"), name("B")],
                    }),
                ],
            })
        );
        assert_eq!(GenericRef::parse("$Page<>").unwrap().args, vec![]);
    }

    #[test]
    fn rejects_text_that_is_not_one_generic_reference() {
        for text in [
            "Page<User>",
            "$Page",
            "$Foo<Bar",
            "$A<B<C>",
            "$Page<User> tail",
            "$<User>",
            "$Page<,>",
        ] {
            assert_eq!(GenericRef::parse(text), None, "{text}");
        }
    }

    #[test]
    fn concrete_names_flatten_arguments() {
        let name = |text: &str| GenericRef::parse(text).unwrap().concrete_name();
        assert_eq!(name("$Page<User>"), "Page_User");
        assert_eq!(name("$Wrapper<$Inner<Item>>"), "Wrapper_Inner_Item");
        assert_eq!(name("$Map<String, $Pair<A, B>>"), "Map_String_Pair_A_B");
        assert_eq!(name("$Page<>"), "Page_Generic");
    }
}

mod instantiation {
    use super::*;

    #[test]
    fn replaces_generic_reference_with_concrete_name() {
        let reg = registry(&[("Page", &["T"], json!({ "items": { "$ref": "$T" } }))]);

        let (value, concrete, diagnostics) = run(&reg, json!({ "$ref": "$Page<User>" }));

        assert_eq!(value, json!({ "$ref": "$Page_User" }));
        assert_eq!(
            concrete["Page_User"],
            json!({ "items": { "$ref": "$User" } })
        );
        assert!(diagnostics.is_empty());
    }

    #[test]
    fn leaves_everything_else_untouched() {
        let reg = registry(&[("Page", &["T"], json!({}))]);
        let input = json!({
            "a": "$User",
            "b": "see $Page<User>",
            "c": "Page<User>",
            "d": ["x", 1, null, true],
            "e": "$Page",
        });
        let (value, concrete, _) = run(&reg, input.clone());
        assert_eq!(value, input);
        assert!(concrete.is_empty());
    }

    #[test]
    fn each_instantiation_is_created_once() {
        let reg = registry(&[("Page", &["T"], json!({ "items": "$T" }))]);
        let (value, concrete, _) = run(&reg, json!(["$Page<User>", "$Page<User>", "$Page<Order>"]));
        assert_eq!(value, json!(["$Page_User", "$Page_User", "$Page_Order"]));
        assert_eq!(concrete.len(), 2);
    }

    #[test]
    fn nested_generics_are_resolved_inside_out() {
        let reg = registry(&[
            ("Wrapper", &["T"], json!({ "wrap": "$T" })),
            ("Inner", &["U"], json!({ "in": "$U" })),
        ]);

        let (value, concrete, _) = run(&reg, json!("$Wrapper<$Inner<Item>>"));

        assert_eq!(value, json!("$Wrapper_Inner_Item"));
        assert_eq!(concrete["Inner_Item"], json!({ "in": "$Item" }));
        assert_eq!(
            concrete["Wrapper_Inner_Item"],
            json!({ "wrap": "$Inner_Item" })
        );
    }

    #[test]
    fn generics_in_instantiated_bodies_are_resolved() {
        let reg = registry(&[
            (
                "Envelope",
                &["T"],
                json!({ "data": { "$ref": "$Page<$T>" } }),
            ),
            ("Page", &["T"], json!({ "items": "$T" })),
        ]);
        let (_, concrete, _) = run(&reg, json!("$Envelope<User>"));
        assert_eq!(
            concrete["Envelope_User"],
            json!({ "data": { "$ref": "$Page_User" } })
        );
        assert_eq!(concrete["Page_User"], json!({ "items": "$User" }));
    }

    #[test]
    fn parameters_are_substituted_by_position_in_strings_and_keys() {
        let reg = registry(&[(
            "Map",
            &["K", "V"],
            json!({ "key": "$K", "value": ["$V", "$V"], "$K": "named after $K" }),
        )]);
        let (_, concrete, _) = run(&reg, json!("$Map<String, User>"));
        assert_eq!(
            concrete["Map_String_User"],
            json!({ "key": "$String", "value": ["$User", "$User"], "$String": "named after $String" })
        );
    }

    #[test]
    fn parameters_are_replaced_as_whole_identifiers_in_one_pass() {
        // Regression: `$T` was replaced inside `$Tag`, and `$TT` became `$UserT`.
        let reg = registry(&[(
            "Pair",
            &["T", "TT"],
            json!({ "a": "$T", "b": "$TT", "tag": "$Tag", "raw": "$T_x", "size": "$Tä" }),
        )]);
        let (_, concrete, _) = run(&reg, json!("$Pair<TT, User>"));
        assert_eq!(
            concrete["Pair_TT_User"],
            json!({ "a": "$TT", "b": "$User", "tag": "$Tag", "raw": "$T_x", "size": "$Tä" })
        );
    }

    #[test]
    fn argument_count_mismatch_substitutes_what_is_available() {
        let reg = registry(&[("Map", &["K", "V"], json!({ "key": "$K", "value": "$V" }))]);

        let (_, concrete, diagnostics) = run(&reg, json!("$Map<String>"));

        assert_eq!(
            concrete["Map_String"],
            json!({ "key": "$String", "value": "$V" })
        );
        assert_eq!(codes(&diagnostics), [Code::BlueprintArity]);
    }

    #[test]
    fn unknown_blueprint_becomes_the_concrete_name_and_warns() {
        let (value, concrete, diagnostics) = run(&Registry::new(), json!("$Missing<User>"));
        assert_eq!(value, json!("$Missing_User"));
        assert!(concrete.is_empty());
        assert_eq!(codes(&diagnostics), [Code::UnknownBlueprint]);
        assert!(
            diagnostics
                .iter()
                .next()
                .unwrap()
                .message()
                .contains("Missing")
        );
    }

    #[test]
    fn unknown_inner_blueprint_is_named_concretely() {
        let reg = registry(&[("Page", &["T"], json!({ "item": "$T" }))]);
        let (value, concrete, diagnostics) = run(&reg, json!("$Page<$Missing<User>>"));
        assert_eq!(value, json!("$Page_Missing_User"));
        assert_eq!(
            concrete["Page_Missing_User"],
            json!({ "item": "$Missing_User" })
        );
        assert_eq!(codes(&diagnostics), [Code::UnknownBlueprint]);
    }

    #[test]
    fn self_referencing_blueprints_terminate() {
        let reg = registry(&[(
            "Tree",
            &["T"],
            json!({ "value": "$T", "children": { "items": "$Tree<$T>" } }),
        )]);
        let (_, concrete, diagnostics) = run(&reg, json!("$Tree<User>"));
        assert_eq!(
            concrete["Tree_User"],
            json!({ "value": "$User", "children": { "items": "$Tree_User" } })
        );
        assert!(diagnostics.is_empty());
    }

    #[test]
    fn ever_growing_instantiation_hits_the_depth_limit() {
        let reg = registry(&[
            ("Nest", &["T"], json!({ "inner": "$Nest<$Wrap<$T>>" })),
            ("Wrap", &["T"], json!({ "x": "$T" })),
        ]);
        let (_, _, diagnostics) = run(&reg, json!("$Nest<User>"));
        assert!(
            codes(&diagnostics).contains(&Code::InstantiationDepth),
            "{diagnostics:#?}"
        );
        assert!(diagnostics.has_errors());
    }
}

mod std_containers {
    use super::*;

    #[test]
    fn vec_shorthand_becomes_an_inline_array() {
        let (value, concrete, _) = run(&Registry::new(), json!({ "schema": "$Vec<Tag>" }));
        assert_eq!(
            value,
            json!({ "schema": { "type": "array", "items": { "$ref": "$Tag" } } })
        );
        assert!(concrete.is_empty());
    }

    #[test]
    fn shorthand_as_ref_replaces_the_schema_and_keeps_siblings() {
        let (value, _, _) = run(
            &Registry::new(),
            json!({ "$ref": "$Vec<String>", "description": "Names" }),
        );
        assert_eq!(
            value,
            json!({ "type": "array", "items": { "type": "string" }, "description": "Names" })
        );
    }

    #[test]
    fn containers_of_generics_instantiate_the_items() {
        let reg = registry(&[("Page", &["T"], json!({ "items": "$T" }))]);
        let (value, concrete, _) = run(&reg, json!("$Vec<$Page<User>>"));
        assert_eq!(
            value,
            json!({ "type": "array", "items": { "$ref": "$Page_User" } })
        );
        assert_eq!(concrete["Page_User"], json!({ "items": "$User" }));
    }

    #[test]
    fn a_blueprint_named_like_a_container_wins() {
        let reg = registry(&[("Vec", &["T"], json!({ "custom": "$T" }))]);
        let (value, concrete, _) = run(&reg, json!({ "$ref": "$Vec<Tag>" }));
        assert_eq!(value, json!({ "$ref": "$Vec_Tag" }));
        assert_eq!(concrete["Vec_Tag"], json!({ "custom": "$Tag" }));
    }
}
