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

/// The alternative that joins the schema is lowered with it: a `type`
/// array became `type: [integer, string], nullable: true`, a `type: "null"`
/// stayed, and `anyOf: [{type: "null"}]` became `anyOf: []`.
#[test]
fn the_merged_nullable_alternative_is_lowered_too() {
    assert_eq!(
        lowered(json!({ "anyOf": [{ "type": ["integer", "string"], "minimum": 1 }, { "type": "null" }] })).0,
        json!({ "nullable": true, "minimum": 1, "anyOf": [{ "type": "integer" }, { "type": "string" }] })
    );
    assert_eq!(
        lowered(json!({ "anyOf": [{ "type": "null" }] })).0,
        json!({ "nullable": true, "enum": [null] })
    );
    assert_eq!(
        lowered(
            json!({ "anyOf": [{ "type": "null", "description": "inner" }, { "type": "null" }] })
        )
        .0,
        json!({ "nullable": true, "description": "inner", "enum": [null] })
    );
    assert_eq!(
        lowered(json!({ "anyOf": [{ "const": 5 }, { "type": "null" }] })).0,
        json!({ "nullable": true, "enum": [5] })
    );
    // Swagger 2.0 lowers the same way, then keeps `anyOf` as an extension.
    let mut document = json!({ "components": { "schemas": {
        "A": { "anyOf": [{ "type": ["integer", "string"] }, { "type": "null" }] },
        "E": { "anyOf": [{ "type": "null" }] }
    } } });
    let mut diagnostics = Diagnostics::new();
    lower(&mut document, Dialect::Swagger20, &mut diagnostics);
    assert_eq!(
        document["components"]["schemas"],
        json!({
            "A": { "x-nullable": true, "x-anyOf": [{ "type": "integer" }, { "type": "string" }] },
            "E": { "x-nullable": true, "enum": [null] }
        })
    );
    assert_eq!(codes(&diagnostics), [Code::LossyLowering]); // `anyOf`
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

#[test]
fn asyncapi_lowering_leaves_multi_format_payloads_alone() {
    let payload = json!({
        "schemaFormat": "application/vnd.google.protobuf;version=3",
        "schema": "syntax = \"proto3\";\nmessage A { repeated int32 prefixItems = 1; }\n",
    });
    let mut document = json!({
        "asyncapi": "3.0.0",
        "components": { "messages": { "A": { "payload": payload.clone() } } },
    });
    let mut diagnostics = Diagnostics::new();
    lower(&mut document, Dialect::AsyncApi3, &mut diagnostics);
    assert_eq!(document["components"]["messages"]["A"]["payload"], payload);
    assert!(diagnostics.is_empty());
}

/// OpenAPI 3.2 features below `components` and in callbacks are removed like
/// those of `paths` when lowering to 3.1.
mod components_and_callbacks {
    use super::*;

    /// A path item with every OpenAPI 3.2 feature, and an extension that
    /// holds look-alike data.
    fn modern_path_item() -> Value {
        json!({
            "parameters": [
                { "name": "filter", "in": "querystring", "content": { "application/json": {} } },
                { "name": "page", "in": "query" },
            ],
            "query": { "responses": {} },
            "additionalOperations": { "PURGE": { "responses": {} } },
            "post": {
                "parameters": [{ "name": "q", "in": "querystring", "content": { "application/json": {} } }],
                "requestBody": { "content": { "application/jsonl": { "itemSchema": { "type": "string" } } } },
                "responses": { "200": {
                    "description": "Events",
                    "content": { "text/event-stream": { "itemSchema": { "type": "string" } } },
                }},
            },
            "x-meta": { "parameters": [{ "in": "querystring" }], "requestBody": { "content": { "a/b": { "itemSchema": {} } } } },
        })
    }

    /// `modern_path_item` without its 3.2 features.
    fn restricted_path_item() -> Value {
        json!({
            "parameters": [{ "name": "page", "in": "query" }],
            "post": {
                "parameters": [],
                "requestBody": { "content": { "application/jsonl": {} } },
                "responses": { "200": { "description": "Events", "content": { "text/event-stream": {} } } },
            },
            "x-meta": { "parameters": [{ "in": "querystring" }], "requestBody": { "content": { "a/b": { "itemSchema": {} } } } },
        })
    }

    fn lowered_31(mut document: Value) -> (Value, Vec<String>) {
        let mut diagnostics = Diagnostics::new();
        lower(&mut document, Dialect::OpenApi31, &mut diagnostics);
        assert!(
            diagnostics
                .iter()
                .all(|d| d.code() == Code::UnsupportedFeature),
            "{diagnostics:?}"
        );
        let messages = diagnostics
            .iter()
            .map(|d| d.message().to_string())
            .collect();
        (document, messages)
    }

    const ALL_REMOVED: [&str; 4] = [
        "`QUERY` operations need OpenAPI 3.2 or newer and were left out of the OpenAPI 3.1 document",
        "`additionalOperations` need OpenAPI 3.2 or newer and were left out of the OpenAPI 3.1 document",
        "`querystring` parameters need OpenAPI 3.2 or newer and were left out of the OpenAPI 3.1 document",
        "streamed responses (`itemSchema`) need OpenAPI 3.2 or newer and were left out of the OpenAPI 3.1 document",
    ];

    #[test]
    fn path_items_of_components_are_restricted() {
        let (document, messages) = lowered_31(json!({
            "openapi": "3.1.1",
            "components": { "pathItems": { "Search": modern_path_item(), "Ref": { "$ref": "#/x" } } },
        }));
        assert_eq!(
            document["components"]["pathItems"],
            json!({ "Search": restricted_path_item(), "Ref": { "$ref": "#/x" } })
        );
        assert_eq!(messages, ALL_REMOVED);
    }

    #[test]
    fn callbacks_of_operations_are_restricted_at_any_depth() {
        let mut inner = modern_path_item();
        inner["post"]["callbacks"] =
            json!({ "nested": { "{$request.body#/url}": modern_path_item() } });
        let (document, messages) = lowered_31(json!({
            "openapi": "3.1.1",
            "paths": { "/subscribe": { "post": {
                "responses": {},
                "callbacks": {
                    "onEvent": {
                        "{$request.body#/callback}": inner,
                        "x-note": { "query": { "responses": {} } },
                    },
                    "shared": { "$ref": "#/components/callbacks/Shared" },
                },
            }}},
        }));
        let callbacks = &document["paths"]["/subscribe"]["post"]["callbacks"];
        let mut expected = restricted_path_item();
        expected["post"]["callbacks"] =
            json!({ "nested": { "{$request.body#/url}": restricted_path_item() } });
        assert_eq!(callbacks["onEvent"]["{$request.body#/callback}"], expected);
        assert_eq!(
            callbacks["onEvent"]["x-note"],
            json!({ "query": { "responses": {} } }),
            "extensions are data"
        );
        assert_eq!(
            callbacks["shared"],
            json!({ "$ref": "#/components/callbacks/Shared" })
        );
        assert_eq!(messages, ALL_REMOVED);
    }

    #[test]
    fn callbacks_of_components_are_restricted() {
        let (document, messages) = lowered_31(json!({
            "openapi": "3.1.1",
            "components": { "callbacks": {
                "Shared": { "{$request.body#/url}": modern_path_item() },
                "Malformed": "not a callback",
            }},
        }));
        assert_eq!(
            document["components"]["callbacks"],
            json!({
                "Shared": { "{$request.body#/url}": restricted_path_item() },
                "Malformed": "not a callback",
            })
        );
        assert_eq!(messages, ALL_REMOVED);
    }

    #[test]
    fn streamed_component_responses_and_request_bodies_lose_item_schemas() {
        for section in ["responses", "requestBodies"] {
            let (document, messages) = lowered_31(json!({
                "openapi": "3.1.1",
                "components": { section: {
                    "Events": {
                        "description": "Events",
                        "content": {
                            "text/event-stream": { "itemSchema": { "type": "string" } },
                            "application/json": { "schema": { "type": "string" } },
                        },
                    },
                    "NoContent": { "description": "None" },
                }},
            }));
            assert_eq!(
                document["components"][section],
                json!({
                    "Events": {
                        "description": "Events",
                        "content": {
                            "text/event-stream": {},
                            "application/json": { "schema": { "type": "string" } },
                        },
                    },
                    "NoContent": { "description": "None" },
                }),
                "{section}"
            );
            assert_eq!(messages, [ALL_REMOVED[3]], "{section}");
        }
    }

    #[test]
    fn other_component_sections_are_left_alone() {
        let components = json!({
            "schemas": { "S": { "type": "object", "properties": { "query": { "type": "string" } } } },
            "parameters": { "Q": { "name": "q", "in": "query" } },
            "headers": { "H": { "schema": { "type": "string" } } },
            "examples": { "E": { "value": { "itemSchema": 1, "query": 2 } } },
        });
        let (document, messages) =
            lowered_31(json!({ "openapi": "3.1.1", "components": components.clone() }));
        assert_eq!(document["components"], components);
        assert!(messages.is_empty(), "{messages:?}");
    }

    #[test]
    fn openapi_32_keeps_everything() {
        let document = json!({
            "openapi": "3.2.0",
            "components": {
                "pathItems": { "Search": modern_path_item() },
                "callbacks": { "Shared": { "{$request.body#/url}": modern_path_item() } },
            },
        });
        let mut lowered = document.clone();
        let mut diagnostics = Diagnostics::new();
        lower(&mut lowered, Dialect::OpenApi32, &mut diagnostics);
        assert_eq!(lowered, document);
        assert!(diagnostics.is_empty());
    }
}

/// `contentEncoding` and `contentMediaType` (JSON Schema 2020-12) become the
/// OpenAPI 3.0 formats `byte` and `binary`, as the OpenAPI 3.0 → 3.1
/// migration guide maps them.
mod content_keywords {
    use super::*;
    use oas_forge::schema::dialect::lower_schema_value;

    fn lower_to(dialect: Dialect, mut schema: Value) -> (Value, Vec<String>) {
        let mut diagnostics = Diagnostics::new();
        lower_schema_value(&mut schema, dialect, &mut diagnostics);
        let messages = diagnostics
            .iter()
            .map(|d| d.message().to_string())
            .collect();
        (schema, messages)
    }

    #[test]
    fn base64_content_is_a_byte_string() {
        for dialect in [Dialect::OpenApi30, Dialect::Swagger20] {
            assert_eq!(
                lower_to(
                    dialect,
                    json!({ "type": "string", "contentEncoding": "base64" })
                ),
                (json!({ "type": "string", "format": "byte" }), vec![]),
                "{dialect}"
            );
        }
        // A nullable string is a string after lowering.
        assert_eq!(
            lower_to(
                Dialect::OpenApi30,
                json!({ "type": ["string", "null"], "contentEncoding": "base64" })
            )
            .0,
            json!({ "type": "string", "nullable": true, "format": "byte" })
        );
    }

    #[test]
    fn binary_content_is_a_binary_string() {
        for media in [
            "application/octet-stream",
            "image/png",
            "Application/PDF; q=1",
        ] {
            for dialect in [Dialect::OpenApi30, Dialect::Swagger20] {
                assert_eq!(
                    lower_to(
                        dialect,
                        json!({ "type": "string", "contentMediaType": media })
                    ),
                    (json!({ "type": "string", "format": "binary" }), vec![]),
                    "{media} {dialect}"
                );
            }
        }
        // Without `type` too: the keywords only apply to strings.
        assert_eq!(
            lower_to(
                Dialect::OpenApi30,
                json!({ "contentMediaType": "image/png" })
            )
            .0,
            json!({ "format": "binary" })
        );
    }

    #[test]
    fn the_media_type_of_encoded_content_is_lost() {
        let (schema, messages) = lower_to(
            Dialect::OpenApi30,
            json!({ "type": "string", "contentEncoding": "base64", "contentMediaType": "image/png" }),
        );
        assert_eq!(schema, json!({ "type": "string", "format": "byte" }));
        assert_eq!(
            messages,
            ["OpenAPI 3.0 has no equivalent for `contentMediaType`; the schema was approximated"]
        );
    }

    #[test]
    fn text_content_and_other_encodings_have_no_format() {
        for media in [
            "text/plain",
            "application/json",
            "application/xml",
            "application/yaml",
            "application/problem+json",
            "application/atom+xml",
            "application/ld+yaml",
        ] {
            let (schema, messages) = lower_to(
                Dialect::OpenApi30,
                json!({ "type": "string", "contentMediaType": media }),
            );
            assert_eq!(schema, json!({ "type": "string" }), "{media}");
            assert_eq!(messages.len(), 1, "{media}");
        }
        let (schema, messages) = lower_to(
            Dialect::OpenApi30,
            json!({ "type": "string", "contentEncoding": "base32", "contentMediaType": "image/png" }),
        );
        assert_eq!(schema, json!({ "type": "string" }));
        assert_eq!(messages.len(), 2, "{messages:?}");
    }

    #[test]
    fn explicit_formats_and_other_types_win() {
        // The same format: nothing is lost.
        assert_eq!(
            lower_to(
                Dialect::OpenApi30,
                json!({ "type": "string", "format": "byte", "contentEncoding": "base64" })
            ),
            (json!({ "type": "string", "format": "byte" }), vec![])
        );
        // Another format stays; the content keyword is reported.
        let (schema, messages) = lower_to(
            Dialect::OpenApi30,
            json!({ "type": "string", "format": "uri", "contentMediaType": "image/png" }),
        );
        assert_eq!(schema, json!({ "type": "string", "format": "uri" }));
        assert_eq!(messages.len(), 1);
        // Not a string: no format.
        let (schema, messages) = lower_to(
            Dialect::OpenApi30,
            json!({ "type": "object", "contentEncoding": "base64" }),
        );
        assert_eq!(schema, json!({ "type": "object" }));
        assert_eq!(messages.len(), 1);
    }

    #[test]
    fn openapi_31_keeps_the_content_keywords() {
        let schema = json!({ "type": "string", "contentEncoding": "base64" });
        assert_eq!(lower_to(Dialect::OpenApi31, schema.clone()).0, schema);
    }
}

/// In OpenAPI, `headers` maps names to Header Objects: the names are not
/// schema keywords, only the `schema` of each header is lowered.
#[test]
fn openapi_headers_are_maps_of_header_objects() {
    let mut document = json!({
        "openapi": "3.0.3",
        "paths": { "/a": { "get": { "responses": { "200": {
            "description": "OK",
            "headers": { "X-Rate-Limit": { "schema": { "type": ["integer", "null"] } } },
        }}}}},
        "components": { "headers": { "X-Trace": { "schema": { "const": "t" } } } },
    });
    let mut diagnostics = Diagnostics::new();
    lower(&mut document, Dialect::OpenApi30, &mut diagnostics);
    assert_eq!(
        document["paths"]["/a"]["get"]["responses"]["200"]["headers"],
        json!({ "X-Rate-Limit": { "schema": { "type": "integer", "nullable": true } } })
    );
    assert_eq!(
        document["components"]["headers"],
        json!({ "X-Trace": { "schema": { "enum": ["t"] } } })
    );
    assert!(diagnostics.is_empty(), "{diagnostics:?}");
}
