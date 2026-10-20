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
