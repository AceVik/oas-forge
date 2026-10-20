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

#[test]
fn malformed_schema_sections_are_skipped() {
    let mut document = json!({ "openapi": "3.0.3", "components": { "schemas": 5 } });
    lower(&mut document, Dialect::OpenApi30, &mut Diagnostics::new());
    assert_eq!(document["components"]["schemas"], 5);
}

#[test]
fn dialects_have_display_names() {
    let names: Vec<String> = [
        Dialect::OpenApi30,
        Dialect::OpenApi31,
        Dialect::OpenApi32,
        Dialect::Swagger20,
        Dialect::AsyncApi3,
    ]
    .iter()
    .map(ToString::to_string)
    .collect();
    assert_eq!(
        names,
        [
            "OpenAPI 3.0",
            "OpenAPI 3.1",
            "OpenAPI 3.2",
            "Swagger 2.0",
            "AsyncAPI 3"
        ]
    );
}

#[test]
fn empty_tuples_and_required_lists_are_dropped() {
    assert_eq!(
        lowered(json!({ "type": "array", "items": false })).0,
        json!({ "type": "array" })
    );
    assert_eq!(
        lowered(json!({ "type": "object", "required": [] })).0,
        json!({ "type": "object" })
    );
}

#[test]
fn reference_siblings_join_an_existing_all_of() {
    assert_eq!(
        lowered(json!({ "$ref": "#/components/schemas/A", "allOf": [{ "minProperties": 1 }], "description": "d" })).0,
        json!({
            "allOf": [{ "$ref": "#/components/schemas/A" }, { "minProperties": 1 }],
            "description": "d"
        })
    );
}

mod single_schemas {
    use super::*;
    use oas_forge::schema::dialect::lower_schema_value;

    fn lower_one(mut schema: Value, dialect: Dialect) -> (Value, Diagnostics) {
        let mut diagnostics = Diagnostics::new();
        lower_schema_value(&mut schema, dialect, &mut diagnostics);
        (schema, diagnostics)
    }

    #[test]
    fn openapi_31_and_32_keep_the_schema() {
        let schema = json!({ "type": ["string", "null"], "prefixItems": [] });
        for dialect in [Dialect::OpenApi31, Dialect::OpenApi32] {
            assert_eq!(lower_one(schema.clone(), dialect).0, schema);
        }
    }

    #[test]
    fn openapi_30_and_swagger_lower_and_report() {
        let (schema, diagnostics) =
            lower_one(json!({ "type": ["string", "null"] }), Dialect::OpenApi30);
        assert_eq!(schema, json!({ "type": "string", "nullable": true }));
        assert!(diagnostics.is_empty());

        let (schema, diagnostics) = lower_one(
            json!({ "oneOf": [{ "type": "string" }] }),
            Dialect::Swagger20,
        );
        assert_eq!(schema, json!({ "x-oneOf": [{ "type": "string" }] }));
        assert_eq!(codes(&diagnostics), [Code::LossyLowering]);
        assert!(
            diagnostics
                .iter()
                .next()
                .unwrap()
                .message()
                .starts_with("Swagger 2.0 has no")
        );
    }

    #[test]
    fn asyncapi_uses_draft_7_definitions() {
        let (schema, diagnostics) = lower_one(
            json!({ "$defs": { "A": { "type": "string" } }, "unevaluatedProperties": false }),
            Dialect::AsyncApi3,
        );
        assert_eq!(
            schema,
            json!({ "definitions": { "A": { "type": "string" } } })
        );
        assert_eq!(codes(&diagnostics), [Code::LossyLowering]);
    }
}

/// Values that only look like schemas are data and stay as written.
mod data_is_not_lowered {
    use super::*;

    #[test]
    fn examples_of_media_types_are_kept() {
        let example = json!({ "schema": { "type": ["string", "null"] } });
        let mut document = json!({
            "openapi": "3.0.3",
            "paths": { "/a": { "get": { "responses": { "200": {
                "description": "OK",
                "content": { "application/json": {
                    "schema": { "type": ["string", "null"] },
                    "example": example,
                }}
            }}}}}
        });
        lower(&mut document, Dialect::OpenApi30, &mut Diagnostics::new());
        let media =
            &document["paths"]["/a"]["get"]["responses"]["200"]["content"]["application/json"];
        assert_eq!(
            media["schema"],
            json!({ "type": "string", "nullable": true })
        );
        assert_eq!(media["example"], example);
    }

    #[test]
    fn enum_values_are_not_subschemas() {
        let value = json!({ "type": ["string", "null"] });
        assert_eq!(
            lowered(json!({ "enum": [value.clone()] })).0,
            json!({ "enum": [value] })
        );
    }
}

#[test]
fn querystring_parameters_of_path_items_need_32() {
    let mut document = json!({
        "openapi": "3.1.1",
        "paths": { "/search": {
            "parameters": [
                { "name": "filter", "in": "querystring", "content": { "application/json": {} } },
                { "name": "page", "in": "query" },
            ],
            "get": { "responses": {} },
        }}
    });
    let mut diagnostics = Diagnostics::new();
    lower(&mut document, Dialect::OpenApi31, &mut diagnostics);
    assert_eq!(
        document["paths"]["/search"]["parameters"],
        json!([{ "name": "page", "in": "query" }])
    );
    assert!(!diagnostics.is_empty());
}
