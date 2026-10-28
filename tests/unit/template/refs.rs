use crate::common::{codes, here};
use oas_forge::diagnostics::{Code, Diagnostics};
use oas_forge::schema::TypeMapper;
use oas_forge::template::Registry;
use oas_forge::template::lexical::ESCAPED_DOLLAR;
use oas_forge::template::refs::{KnownSchemas, Targets, resolve, unescape};
use serde_json::{Value, json};

fn known(names: &[&str]) -> KnownSchemas {
    let mut known = KnownSchemas::default();
    for name in names {
        known.insert(*name);
    }
    known
}

fn run(value: Value, known: &KnownSchemas, registry: &Registry) -> (Value, Diagnostics) {
    let mut value = value;
    let mut diagnostics = Diagnostics::new();
    let targets = Targets {
        known,
        registry,
        types: TypeMapper::builtin(),
    };
    resolve(&mut value, targets, &here(), &mut diagnostics);
    (value, diagnostics)
}

#[test]
fn whole_scalars_naming_known_schemas_become_component_refs() {
    let (value, diagnostics) = run(
        json!({ "$ref": "$User", "list": ["$User", "$_Internal"] }),
        &known(&["User", "_Internal"]),
        &Registry::new(),
    );
    assert_eq!(
        value,
        json!({
            "$ref": "#/components/schemas/User",
            "list": ["#/components/schemas/User", "#/components/schemas/_Internal"]
        })
    );
    assert!(diagnostics.is_empty());
}

#[test]
fn text_containing_a_reference_is_untouched() {
    let input = json!({
        "description": "Returns $User objects",
        "a": "$UserName",
        "b": "$User_",
        "c": "costs $5",
        "d": "$",
        "e": "$$OIDC_URL/x",
        "f": 3,
    });
    let (value, diagnostics) = run(input.clone(), &known(&["User"]), &Registry::new());
    assert_eq!(value, input);
    assert_eq!(
        codes(&diagnostics),
        [Code::UnresolvedReference, Code::UnresolvedReference]
    );
}

#[test]
fn unknown_names_are_reported_once() {
    let (value, diagnostics) = run(
        json!(["$Ghost", "$Ghost", "$Größe"]),
        &known(&[]),
        &Registry::new(),
    );
    assert_eq!(value, json!(["$Ghost", "$Ghost", "$Größe"]));
    assert_eq!(
        codes(&diagnostics),
        [Code::UnresolvedReference, Code::UnresolvedReference]
    );
    assert!(
        diagnostics
            .iter()
            .next()
            .unwrap()
            .message()
            .contains("$Ghost")
    );
}

#[test]
fn renamed_rust_types_resolve_through_aliases() {
    // Regression: `struct Profile` renamed to `UserProfile` left `$Profile` dangling.
    let mut registry = Registry::new();
    registry.add_alias("Profile", "UserProfile");
    registry.add_alias("Same", "Same");
    assert_eq!(registry.alias("Same"), None);

    let (value, _) = run(json!("$Profile"), &known(&["UserProfile"]), &registry);
    assert_eq!(value, json!("#/components/schemas/UserProfile"));

    let (value, diagnostics) = run(json!("$Profile"), &known(&[]), &registry);
    assert_eq!(value, json!("$Profile"), "alias target must exist");
    assert_eq!(codes(&diagnostics), [Code::UnresolvedReference]);
}

#[test]
fn known_names_are_collected_from_documents() {
    let mut known = KnownSchemas::default();
    known.collect_from(&json!({ "components": { "schemas": { "A": {}, "B": {} } } }));
    known.collect_from(&json!({ "paths": {} }));
    assert!(known.contains("A") && known.contains("B") && !known.contains("C"));
}

#[test]
fn unescape_restores_protected_dollars_in_values_and_keys() {
    let escaped = format!("{ESCAPED_DOLLAR}User");
    let mut value =
        json!({ "a": escaped.clone(), "list": [escaped.clone()], escaped.clone(): 1, "n": null });
    unescape(&mut value);
    assert_eq!(
        value,
        json!({ "a": "$User", "list": ["$User"], "$User": 1, "n": null })
    );

    let mut plain = json!({ "a": "x" });
    unescape(&mut plain);
    assert_eq!(plain, json!({ "a": "x" }));
}

#[test]
fn regular_expressions_keep_their_escaped_dollars() {
    let escaped = format!("^{ESCAPED_DOLLAR}\\d+$");
    let mut value = json!({
        "pattern": escaped.clone(),
        "description": escaped.clone(),
        "properties": { "pattern": { "pattern": escaped.clone() } },
        "patternProperties": { escaped.clone(): { "type": "string" } },
        "examples": [{ "pattern": [escaped.clone()] }],
    });
    unescape(&mut value);
    assert_eq!(
        value,
        json!({
            "pattern": "^\\$\\d+$",
            "description": "^$\\d+$",
            "properties": { "pattern": { "pattern": "^\\$\\d+$" } },
            "patternProperties": { "^\\$\\d+$": { "type": "string" } },
            "examples": [{ "pattern": ["^$\\d+$"] }],
        })
    );
}

#[test]
fn references_to_built_in_types_become_their_schema() {
    // `$Page<String>` instantiates `{"$ref": "$T"}` as `{"$ref": "$String"}`.
    let (value, diagnostics) = run(
        json!({
            "a": { "$ref": "$String", "description": "d", "type": "kept" },
            "b": { "$ref": "$u8" },
            "c": { "$ref": "$Uuid" },
            "d": { "$ref": "$Vec" },
            "e": { "$ref": "$Unknown" },
        }),
        &known(&[]),
        &Registry::new(),
    );
    assert_eq!(
        value,
        json!({
            "a": { "description": "d", "type": "kept" },
            "b": { "type": "integer", "format": "int32", "minimum": 0, "maximum": 255 },
            "c": { "type": "string", "format": "uuid" },
            "d": { "type": "array" },
            // Reported, and still a well-formed pointer.
            "e": { "$ref": "#/components/schemas/Unknown" },
        })
    );
    assert_eq!(codes(&diagnostics), [Code::UnresolvedReference]);
}

/// The built-in schema of a name written bare carries its alternative
/// (`x-oas-forge-builtin`): the alternative when the name is documented,
/// also through an alias, else the built-in schema without the marker.
#[test]
fn the_alternative_of_a_built_in_schema_is_chosen_when_documented() {
    let uuid = json!({
        "type": "string",
        "format": "uuid",
        "x-oas-forge-builtin": { "name": "Uuid", "schema": { "$ref": "$Uuid", "title": "Id" } },
    });
    let url = json!({
        "type": "string",
        "format": "uri",
        "x-oas-forge-builtin": { "name": "Url", "schema": { "$ref": "$Url" } },
    });
    let mut registry = Registry::new();
    registry.add_alias("Url", "Link");
    let (value, diagnostics) = run(
        json!({ "a": uuid, "b": { "items": url } }),
        &known(&["Uuid", "Link"]),
        &registry,
    );
    assert_eq!(
        value,
        json!({
            "a": { "$ref": "#/components/schemas/Uuid", "title": "Id" },
            "b": { "items": { "$ref": "#/components/schemas/Link" } },
        })
    );
    assert!(diagnostics.is_empty(), "{diagnostics:?}");
    let (value, diagnostics) = run(
        json!({ "a": uuid, "b": url }),
        &known(&[]),
        &Registry::new(),
    );
    assert_eq!(
        value,
        json!({
            "a": { "type": "string", "format": "uuid" },
            "b": { "type": "string", "format": "uri" },
        })
    );
    assert!(diagnostics.is_empty(), "{diagnostics:?}");
}

#[test]
fn documented_schemas_shadow_built_in_types() {
    let mut registry = Registry::new();
    registry.add_alias("Url", "Link");
    let (value, _) = run(
        json!([{ "$ref": "$String" }, { "$ref": "$Url" }]),
        &known(&["String", "Link"]),
        &registry,
    );
    assert_eq!(
        value,
        json!([{ "$ref": "#/components/schemas/String" }, { "$ref": "#/components/schemas/Link" }])
    );
}

/// Whole `$Name` strings in schema positions end up as references, through
/// the whole pipeline (the generics stage wraps them).
mod schema_positions {
    use crate::common::pipeline;
    use serde_json::json;

    #[test]
    fn a_one_of_of_names_in_a_route_is_a_list_of_references() {
        let (document, diagnostics) = pipeline(&[(
            "lib.rs",
            r#"
            /// @openapi
            pub struct ErrorBody { pub error: String }
            /// @openapi
            pub struct HostedUnavailable { pub retry: u32 }
            /// @openapi
            pub struct Page<T> { pub items: Vec<T> }
            /// Join
            /// @route POST /join
            /// @return 409: { oneOf: [ $ErrorBody, $HostedUnavailable ] } "Cannot join"
            /// @return 200: { type: object, properties: { page: $Page<ErrorBody>, name: $String } }
            pub async fn join() {}
            "#,
        )]);
        assert!(diagnostics.is_empty(), "{diagnostics:?}");
        let responses = &document["paths"]["/join"]["post"]["responses"];
        assert_eq!(
            responses["409"]["content"]["application/json"]["schema"]["oneOf"],
            json!([
                { "$ref": "#/components/schemas/ErrorBody" },
                { "$ref": "#/components/schemas/HostedUnavailable" },
            ])
        );
        let properties = &responses["200"]["content"]["application/json"]["schema"]["properties"];
        assert_eq!(
            properties["page"],
            json!({ "$ref": "#/components/schemas/Page_ErrorBody" })
        );
        assert_eq!(properties["name"], json!({ "type": "string" }));
    }

    #[test]
    fn a_discriminator_mapping_stays_a_pointer() {
        let (document, _) = pipeline(&[(
            "lib.rs",
            "/// @openapi\npub struct Cat { pub a: u8 }\n/// @openapi-type Pet\n/// oneOf: [$Cat]\n/// discriminator: { propertyName: kind, mapping: { cat: $Cat } }\nfn _schemas() {}",
        )]);
        let pet = &document["components"]["schemas"]["Pet"];
        assert_eq!(
            pet["oneOf"],
            json!([{ "$ref": "#/components/schemas/Cat" }])
        );
        assert_eq!(
            pet["discriminator"]["mapping"]["cat"],
            "#/components/schemas/Cat"
        );
    }
}
