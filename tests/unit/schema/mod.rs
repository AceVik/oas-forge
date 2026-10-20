//! `src/schema/`: type mapping, nullability and dialects.

mod dialect;
mod types;

use oas_forge::schema::nullable;
use serde_json::json;

#[test]
fn nullable_extends_simple_types() {
    assert_eq!(
        nullable(json!({ "type": "string" })),
        json!({ "type": ["string", "null"] })
    );
    assert_eq!(
        nullable(json!({ "type": ["string", "integer"] })),
        json!({ "type": ["string", "integer", "null"] })
    );
    assert_eq!(
        nullable(json!({ "type": "string", "enum": ["a", "b"] })),
        json!({ "type": ["string", "null"], "enum": ["a", "b", null] })
    );
}

#[test]
fn nullable_wraps_references_and_composed_schemas() {
    for schema in [
        json!({ "$ref": "$User" }),
        json!({ "oneOf": [{ "type": "string" }] }),
        json!({ "type": "object", "allOf": [{ "$ref": "$A" }] }),
        json!({ "const": 1 }),
        json!({ "type": 5 }),
    ] {
        assert_eq!(
            nullable(schema.clone()),
            json!({ "anyOf": [schema, { "type": "null" }] })
        );
    }
}

#[test]
fn nullable_leaves_schemas_that_accept_null() {
    for schema in [
        json!({}),
        json!({ "type": "null" }),
        json!({ "type": ["string", "null"] }),
        json!({ "anyOf": [{ "$ref": "$A" }, { "type": "null" }] }),
        json!(true),
    ] {
        assert_eq!(nullable(schema.clone()), schema);
    }
}

mod strip_null {
    use oas_forge::schema::{nullable, strip_null};
    use serde_json::json;

    #[test]
    fn removes_what_nullable_adds() {
        for schema in [
            json!({ "type": "string" }),
            json!({ "type": "string", "enum": ["a"] }),
            json!({ "$ref": "$User" }),
            json!({ "oneOf": [{ "type": "string" }] }),
            json!({ "type": ["string", "integer"] }),
        ] {
            assert_eq!(strip_null(nullable(schema.clone())), schema);
        }
    }

    #[test]
    fn keeps_composed_and_unrelated_schemas() {
        assert_eq!(
            strip_null(json!({ "anyOf": [{ "type": "a" }, { "type": "b" }, { "type": "null" }] })),
            json!({ "anyOf": [{ "type": "a" }, { "type": "b" }] })
        );
        assert_eq!(
            strip_null(json!({ "anyOf": [true, { "type": "null" }] })),
            json!(true)
        );
        assert_eq!(
            strip_null(
                json!({ "anyOf": [{ "$ref": "$A" }, { "type": "null" }], "description": "d" })
            ),
            json!({ "$ref": "$A", "description": "d" })
        );
        for schema in [
            json!({ "anyOf": [{ "type": "a" }] }),
            json!({}),
            json!(true),
            json!({ "type": "null" }),
        ] {
            assert_eq!(strip_null(schema.clone()), schema);
        }
    }
}

#[test]
fn nullable_keeps_constants_exact() {
    // `{type: [string, null], const: "a"}` would still reject `null`.
    for schema in [
        json!({ "type": "string", "const": "a" }),
        json!({ "type": ["string", "integer"], "const": "a" }),
    ] {
        assert_eq!(
            nullable(schema.clone()),
            json!({ "anyOf": [schema, { "type": "null" }] })
        );
    }
}

#[test]
fn nullable_recognizes_null_alternatives() {
    let with_null = json!({ "anyOf": [{ "type": "string" }, { "type": "null" }] });
    assert_eq!(nullable(with_null.clone()), with_null);
    let without = json!({ "anyOf": [{ "type": "string" }, { "type": "integer" }] });
    assert_eq!(
        nullable(without.clone()),
        json!({ "anyOf": [without, { "type": "null" }] })
    );
}
