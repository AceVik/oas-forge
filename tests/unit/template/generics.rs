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
    fn parser_nesting_is_bounded_before_recursing() {
        let nested = |depth| format!("${}User{}", "Page<".repeat(depth), ">".repeat(depth));
        assert!(GenericRef::parse(&nested(128)).is_some());
        for depth in [129, 10_000] {
            assert_eq!(GenericRef::parse(&nested(depth)), None);
        }
        // Siblings do not consume each other's nesting budget.
        let wide = format!("$Tuple<{}>", vec!["Page<User>"; 200].join(","));
        assert_eq!(GenericRef::parse(&wide).unwrap().args.len(), 200);
    }

    #[test]
    fn malformed_and_excessively_nested_references_are_reported() {
        for reference in [
            "$Page<User".to_string(),
            format!("${}User{}", "Page<".repeat(129), ">".repeat(129)),
        ] {
            let input = json!({ "$ref": reference });
            let (value, concrete, diagnostics) = run(&Registry::new(), input.clone());
            assert_eq!(value, input);
            assert!(concrete.is_empty());
            assert_eq!(codes(&diagnostics), [Code::InvalidGenericReference]);
            let finding = diagnostics.iter().next().unwrap();
            assert!(finding.is_error());
            assert_eq!(finding.location(), Some(&here()));
        }
    }

    #[test]
    fn generic_diagnostics_do_not_reject_data_or_external_references() {
        let input = json!({
            "example": { "$ref": "$not<a-reference" },
            "x-data": { "$ref": "$not<a-reference" },
            "properties": { "x": { "$ref": "other.json#/$defs/a<b" } }
        });
        let (value, _, diagnostics) = run(&Registry::new(), input.clone());
        assert_eq!(value, input);
        assert!(diagnostics.is_empty(), "{diagnostics:?}");
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
    fn text_is_the_rust_type_without_sigils() {
        let text = |t: &str| GenericRef::parse(t).unwrap().text();
        assert_eq!(text("$Page<models::User>"), "Page<User>");
        assert_eq!(
            text("$Map<String, $Pair<A, $Vec<B>>>"),
            "Map<String, Pair<A, Vec<B>>>"
        );
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

    /// A blueprint argument is its concrete name in text and keys too, not
    /// only in whole values.
    #[test]
    fn nested_blueprint_arguments_are_bound_by_their_concrete_name() {
        let reg = registry(&[
            ("Wrapper", &["T"], json!({ "x-of": "wraps $T", "$T": true })),
            ("Inner", &["U"], json!({ "in": "$U" })),
        ]);
        let (_, concrete, _) = run(&reg, json!("$Wrapper<$Inner<Item>>"));
        assert_eq!(
            concrete["Wrapper_Inner_Item"],
            json!({ "x-of": "wraps $Inner_Item", "$Inner_Item": true })
        );
    }

    #[test]
    fn nested_blueprints_are_instantiated_even_when_the_outer_parameter_is_unused() {
        let reg = registry(&[
            ("Outer", &["T"], json!({ "type": "object" })),
            ("Inner", &["T"], json!({ "payload": { "$ref": "$T" } })),
        ]);
        let (value, concrete, diagnostics) = run(&reg, json!({ "$ref": "$Outer<Inner<User>>" }));
        assert_eq!(value, json!({ "$ref": "$Outer_Inner_User" }));
        assert_eq!(concrete["Outer_Inner_User"], json!({ "type": "object" }));
        assert_eq!(
            concrete["Inner_User"],
            json!({ "payload": { "$ref": "$User" } })
        );
        assert!(diagnostics.is_empty());
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
        assert_eq!(
            concrete["Page_User"],
            json!({ "items": { "$ref": "$User" } }),
            "a name where a schema is expected is a reference"
        );
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
            json!({ "value": "$User", "children": { "items": { "$ref": "$Tree_User" } } })
        );
        assert!(diagnostics.is_empty());
    }

    #[test]
    fn bare_parameters_in_nested_generic_references_are_bound() {
        // Reflection writes a field `Envelope<T>` as `$Envelope<T>`, without
        // `$` on the parameter.
        let reg = registry(&[
            ("Envelope", &["T"], json!({ "data": { "$ref": "$T" } })),
            (
                "Page",
                &["T"],
                json!({
                    "first": { "$ref": "$Envelope<T>" },
                    "rest": { "items": { "$ref": "$Envelope<$T>" } },
                    "next": { "$ref": "$Page<T>" },
                    "pairs": { "$ref": "$Envelope<Envelope<T>>" },
                }),
            ),
        ]);
        let (value, concrete, diagnostics) = run(&reg, json!("$Page<User>"));
        assert_eq!(value, json!("$Page_User"));
        assert_eq!(
            concrete["Page_User"],
            json!({
                "first": { "$ref": "$Envelope_User" },
                "rest": { "items": { "$ref": "$Envelope_User" } },
                "next": { "$ref": "$Page_User" },
                "pairs": { "$ref": "$Envelope_Envelope_User" },
            })
        );
        assert_eq!(
            concrete["Envelope_User"],
            json!({ "data": { "$ref": "$User" } })
        );
        assert_eq!(
            concrete.keys().collect::<Vec<_>>(),
            ["Envelope_Envelope_User", "Envelope_User", "Page_User"]
        );
        assert!(diagnostics.is_empty(), "{diagnostics:#?}");
    }

    #[test]
    fn bound_arguments_may_be_generic_types() {
        let reg = registry(&[
            ("Envelope", &["T"], json!({ "data": { "$ref": "$T" } })),
            (
                "Page",
                &["T"],
                json!({ "first": { "$ref": "$Envelope<T>" } }),
            ),
        ]);
        let (_, concrete, diagnostics) = run(&reg, json!("$Page<Vec<User>>"));
        assert_eq!(
            concrete["Page_Vec_User"],
            json!({ "first": { "$ref": "$Envelope_Vec_User" } })
        );
        assert_eq!(
            concrete["Envelope_Vec_User"],
            json!({ "data": { "type": "array", "items": { "$ref": "$User" } } })
        );
        assert!(diagnostics.is_empty(), "{diagnostics:#?}");
    }

    #[test]
    fn parameter_names_in_text_are_not_bound() {
        let reg = registry(&[(
            "Page",
            &["T"],
            json!({ "description": "A page of T, see $Envelope<T> and $T" }),
        )]);
        let (_, concrete, _) = run(&reg, json!("$Page<User>"));
        assert_eq!(
            concrete["Page_User"],
            json!({ "description": "A page of T, see $Envelope<T> and $User" })
        );
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
        assert_eq!(
            concrete["Page_User"],
            json!({ "items": { "$ref": "$User" } })
        );
    }

    #[test]
    fn wrappers_of_references_are_inline_schemas() {
        let (value, concrete, diagnostics) = run(
            &Registry::new(),
            json!({ "a": "$Box<User>", "b": "$Option<User>" }),
        );
        assert_eq!(
            value,
            json!({
                "a": { "$ref": "$User" },
                "b": { "anyOf": [{ "$ref": "$User" }, { "type": "null" }] },
            })
        );
        assert!(concrete.is_empty());
        assert!(diagnostics.is_empty(), "{diagnostics:?}");
    }

    #[test]
    fn a_blueprint_named_like_a_container_wins() {
        let reg = registry(&[("Vec", &["T"], json!({ "custom": "$T" }))]);
        let (value, concrete, _) = run(&reg, json!({ "$ref": "$Vec<Tag>" }));
        assert_eq!(value, json!({ "$ref": "$Vec_Tag" }));
        assert_eq!(concrete["Vec_Tag"], json!({ "custom": "$Tag" }));
    }
}

/// A whole `$Name` where a schema is expected is a reference.
mod schema_positions {
    use super::*;

    #[test]
    fn names_in_schema_positions_become_references() {
        let reg = registry(&[("Page", &["T"], json!({ "items": "$T" }))]);
        let (value, concrete, diagnostics) = run(
            &reg,
            json!({
                "oneOf": ["$ErrorBody", "$HostedUnavailable"],
                "allOf": ["$A"], "anyOf": ["$B"], "prefixItems": ["$C"],
                "items": "$Page<User>",
                "not": "$D", "additionalProperties": "$E", "unevaluatedProperties": "$F",
                "contains": "$G", "propertyNames": "$H", "if": "$I", "then": "$J", "else": "$K",
                "properties": { "owner": "$User" },
                "patternProperties": { "^x": "$L" },
                "$defs": { "M": "$M" },
                "content": { "application/json": { "schema": "$N", "itemSchema": "$O" } },
            }),
        );
        assert!(diagnostics.is_empty(), "{diagnostics:?}");
        let reference = |name: &str| json!({ "$ref": format!("${name}") });
        assert_eq!(
            value["oneOf"],
            json!([reference("ErrorBody"), reference("HostedUnavailable")])
        );
        for (key, name) in [
            ("not", "D"),
            ("additionalProperties", "E"),
            ("unevaluatedProperties", "F"),
            ("contains", "G"),
            ("propertyNames", "H"),
            ("if", "I"),
            ("then", "J"),
            ("else", "K"),
        ] {
            assert_eq!(value[key], reference(name), "{key}");
        }
        assert_eq!(value["allOf"][0], reference("A"));
        assert_eq!(value["anyOf"][0], reference("B"));
        assert_eq!(value["prefixItems"][0], reference("C"));
        assert_eq!(value["items"], reference("Page_User"), "instantiated");
        assert_eq!(concrete["Page_User"], json!({ "items": reference("User") }));
        assert_eq!(value["properties"]["owner"], reference("User"));
        assert_eq!(value["patternProperties"]["^x"], reference("L"));
        assert_eq!(value["$defs"]["M"], reference("M"));
        let media = &value["content"]["application/json"];
        assert_eq!(
            (&media["schema"], &media["itemSchema"]),
            (&reference("N"), &reference("O"))
        );
    }

    #[test]
    fn other_strings_keep_their_meaning() {
        let reg = Registry::new();
        let input = json!({
            "description": "$User",
            "$ref": "$User",
            "enum": ["$User"],
            "const": "$User",
            "example": { "items": "$User" },
            "examples": [{ "schema": "$User" }],
            "default": { "not": "$User" },
            "discriminator": { "mapping": { "user": "$User" } },
            "x-extension": { "items": "$User" },
            "items": "see $User",
            "properties": { "name": "User" },
        });
        let (value, _, _) = run(&reg, input.clone());
        assert_eq!(value, input);
    }

    #[test]
    fn std_containers_and_built_in_types_follow() {
        let reg = Registry::new();
        let (value, _, _) = run(&reg, json!({ "items": "$Vec<User>", "not": "$String" }));
        assert_eq!(
            value,
            json!({
                "items": { "type": "array", "items": { "$ref": "$User" } },
                "not": { "$ref": "$String" },
            })
        );
    }
}
