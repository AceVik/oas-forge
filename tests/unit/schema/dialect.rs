//! `src/schema/dialect.rs`: lowering 2020-12 schemas for OpenAPI 3.0.

use crate::common::codes;
use oas_forge::diagnostics::{Code, Diagnostics};
use oas_forge::schema::dialect::{Dialect, lower};
use serde_json::{Value, json};

fn lowered(schema: Value) -> (Value, Diagnostics) {
    let mut document = json!({ "openapi": "3.0.3", "components": { "schemas": { "S": schema } } });
    let mut diagnostics = Diagnostics::new();
    lower(&mut document, Dialect::OpenApi30, &mut diagnostics);
    (document["components"]["schemas"]["S"].clone(), diagnostics)
}

#[test]
fn dialects_come_from_the_openapi_version() {
    assert_eq!(
        Dialect::of_document(&json!({ "openapi": "3.0.3" })),
        Some(Dialect::OpenApi30)
    );
    assert_eq!(
        Dialect::of_document(&json!({ "openapi": "3.1.1" })),
        Some(Dialect::OpenApi31)
    );
    assert_eq!(
        Dialect::of_document(&json!({ "openapi": "3.2.0" })),
        Some(Dialect::OpenApi32)
    );
    assert_eq!(Dialect::of_document(&json!({ "openapi": 3 })), None);
    assert_eq!(Dialect::of_document(&json!({})), None);
}

#[test]
fn newer_dialects_are_left_alone() {
    let mut document =
        json!({ "components": { "schemas": { "S": { "type": ["string", "null"] } } } });
    let original = document.clone();
    lower(&mut document, Dialect::OpenApi31, &mut Diagnostics::new());
    lower(&mut document, Dialect::OpenApi32, &mut Diagnostics::new());
    assert_eq!(document, original);
}

#[test]
fn null_types_become_nullable() {
    assert_eq!(
        lowered(json!({ "type": ["string", "null"] })).0,
        json!({ "type": "string", "nullable": true })
    );
    assert_eq!(
        lowered(json!({ "type": "null" })).0,
        json!({ "enum": [null], "nullable": true })
    );
    assert_eq!(
        lowered(json!({ "type": ["null"] })).0,
        json!({ "enum": [null], "nullable": true })
    );
    assert_eq!(
        lowered(json!({ "type": ["string", "integer"] })).0,
        json!({ "anyOf": [{ "type": "string" }, { "type": "integer" }] })
    );
    assert_eq!(
        lowered(json!({ "type": ["string"] })).0,
        json!({ "type": "string" })
    );
}

#[test]
fn nullable_references_use_all_of() {
    assert_eq!(
        lowered(json!({ "anyOf": [{ "$ref": "#/components/schemas/A" }, { "type": "null" }], "description": "d" })).0,
        json!({ "allOf": [{ "$ref": "#/components/schemas/A" }], "nullable": true, "description": "d" })
    );
    assert_eq!(
        lowered(json!({ "anyOf": [{ "type": "integer", "minimum": 1 }, { "type": "null" }] })).0,
        json!({ "nullable": true, "type": "integer", "minimum": 1 })
    );
    assert_eq!(
        lowered(
            json!({ "anyOf": [{ "type": "integer" }, { "type": "string" }, { "type": "null" }] })
        )
        .0,
        json!({ "anyOf": [{ "type": "integer" }, { "type": "string" }], "nullable": true })
    );
    assert_eq!(
        lowered(json!({ "anyOf": [true, { "type": "null" }] })).0,
        json!({ "allOf": [true], "nullable": true })
    );
}

#[test]
fn keywords_without_equivalent_are_rewritten() {
    assert_eq!(lowered(json!({ "const": 5 })).0, json!({ "enum": [5] }));
    assert_eq!(
        lowered(json!({ "exclusiveMinimum": 0, "exclusiveMaximum": 9.5 })).0,
        json!({ "exclusiveMinimum": true, "minimum": 0, "exclusiveMaximum": true, "maximum": 9.5 })
    );
    assert_eq!(
        lowered(json!({ "examples": [1, 2] })).0,
        json!({ "example": 1 })
    );
    assert_eq!(
        lowered(json!({ "$ref": "#/components/schemas/A", "description": "d" })).0,
        json!({ "description": "d", "allOf": [{ "$ref": "#/components/schemas/A" }] })
    );
    assert_eq!(
        lowered(json!({ "$ref": "#/components/schemas/A" })).0,
        json!({ "$ref": "#/components/schemas/A" })
    );
}

#[test]
fn tuples_and_unsupported_keywords_are_approximated_and_reported_once() {
    let (schema, diagnostics) = lowered(json!({
        "type": "array", "prefixItems": [{ "type": "string" }, { "type": "null" }], "items": false,
        "properties": { "x": { "propertyNames": { "pattern": "a" }, "unevaluatedProperties": false } },
        "unevaluatedItems": false, "dependentSchemas": {}, "propertyNames": {},
    }));
    assert_eq!(schema["items"], json!({ "anyOf": [{ "type": "string" }] }));
    assert!(schema.get("prefixItems").is_none());
    assert_eq!(schema["properties"]["x"], json!({}));
    assert_eq!(codes(&diagnostics), [Code::LossyLowering; 5]);
}

#[test]
fn every_schema_position_of_a_document_is_lowered() {
    let ns = || json!({ "type": ["string", "null"] });
    let mut document = json!({
        "openapi": "3.0.3",
        "paths": { "/a": {
            "parameters": [{ "name": "p", "in": "query", "schema": ns() }],
            "get": {
                "responses": { "200": {
                    "headers": { "X": { "schema": ns() } },
                    "content": { "application/json": {
                        "schema": { "type": "object", "properties": { "a": ns() }, "items": ns(),
                            "additionalProperties": ns(), "allOf": [ns()], "not": ns(),
                            "$defs": { "d": ns() } },
                        "example": { "type": ["string", "null"] },
                    } },
                } },
            },
        } },
        "components": { "schemas": { "S": ns() }, "x-other": { "default": { "type": ["a", "null"] } } },
    });
    lower(&mut document, Dialect::OpenApi30, &mut Diagnostics::new());
    let text = serde_json::to_string(&document).unwrap();
    // `$defs` is not a 3.0 keyword and is dropped.
    assert_eq!(text.matches("\"nullable\":true").count(), 8, "{text}");
    assert_eq!(
        document.pointer("/paths/~1a/get/responses/200/content/application~1json/example"),
        Some(&json!({ "type": ["string", "null"] })),
        "examples are data"
    );
    assert_eq!(
        document["components"]["x-other"]["default"],
        json!({ "type": ["a", "null"] })
    );
}
