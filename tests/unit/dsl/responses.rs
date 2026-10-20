//! `src/dsl/responses.rs`: request bodies and responses.

use super::{findings, response_schema};
use crate::common::operation;
use oas_forge::diagnostics::Code;
use serde_json::{Value, json};

mod request_body {
    use super::*;

    fn body(src: &[&str]) -> Value {
        operation(src, "/x", "post")["requestBody"].clone()
    }

    fn body_schema(type_str: &str) -> Value {
        body(&["@route POST /x", &format!("@body {type_str}")])["content"]["application/json"]
            ["schema"]
            .clone()
    }

    #[test]
    fn defaults_to_json_media_type() {
        assert_eq!(
            body(&["@route POST /x", "@body User"]),
            json!({ "content": { "application/json": { "schema": { "$ref": "$User" } } }, "required": true })
        );
    }

    #[test]
    fn optional_types_make_the_body_optional() {
        assert_eq!(
            body(&["@route POST /x", "@body Option<User>"])["required"],
            false
        );
    }

    #[test]
    fn accepts_custom_media_type() {
        let body = body(&["@route POST /x", "@body String text/plain"]);
        assert_eq!(
            body["content"],
            json!({ "text/plain": { "schema": { "type": "string" } } })
        );
    }

    #[test]
    fn std_wrappers_are_resolved() {
        assert_eq!(
            body_schema("Vec<User>"),
            json!({ "type": "array", "items": { "$ref": "$User" } })
        );
        // `Option<T>` accepts `null`, like serde.
        assert_eq!(
            body_schema("Option<User>"),
            json!({ "anyOf": [{ "$ref": "$User" }, { "type": "null" }] })
        );
    }

    #[test]
    fn custom_generics_stay_raw_for_the_monomorphizer() {
        assert_eq!(body_schema("$Page<User>"), json!({ "$ref": "$Page<User>" }));
        assert_eq!(body_schema("Page<User>"), json!({ "$ref": "$Page<User>" }));
    }

    #[test]
    fn non_rust_names_become_component_refs() {
        assert_eq!(
            body_schema("$User"),
            json!({ "$ref": "#/components/schemas/User" })
        );
        assert_eq!(
            body_schema("user-input"),
            json!({ "$ref": "#/components/schemas/user-input" })
        );
    }

    #[test]
    fn empty_body_directive_is_ignored() {
        assert!(body(&["@route POST /x", "@body"]).is_null());
    }
}

mod return_directive {
    use super::*;

    fn responses(returns: &[&str]) -> Value {
        let mut src = vec!["@route GET /x"];
        src.extend_from_slice(returns);
        operation(&src, "/x", "get")["responses"].clone()
    }

    fn schema_of(type_str: &str) -> Value {
        let op = operation(
            &["@route GET /x", &format!("@return 200: {type_str}")],
            "/x",
            "get",
        );
        response_schema(&op, "200")
    }

    #[test]
    fn typed_response_with_description() {
        assert_eq!(
            responses(&["@return 200: User \"Success\""])["200"],
            json!({
                "description": "Success",
                "content": { "application/json": { "schema": { "$ref": "$User" } } }
            })
        );
    }

    #[test]
    fn description_defaults_to_the_reason_phrase() {
        // OpenAPI requires a description; 0.1 wrote an empty one.
        let responses = responses(&[
            "@return 200: User",
            "@return 404: \"\"",
            "@return 418:",
            "@return 4XX: Problem",
            "@return default: Problem",
        ]);
        let descriptions: Vec<_> = ["200", "404", "418", "4XX", "default"]
            .iter()
            .map(|code| responses[code]["description"].clone())
            .collect();
        assert_eq!(
            descriptions,
            [
                json!("OK"),
                json!("Not Found"),
                json!("Response"),
                json!("Client Error"),
                json!("Default response")
            ]
        );
    }

    #[test]
    fn unit_responses_have_no_content() {
        let responses = responses(&[
            "@return 204: \"Deleted\"",
            "@return 202: () \"Accepted\"",
            "@return 205: unit \"Reset\"",
            "@return 200:",
        ]);
        for code in ["204", "202", "205", "200"] {
            assert!(
                responses[code].get("content").is_none(),
                "{code} must not have a body: {responses:#}"
            );
        }
        assert_eq!(responses["204"]["description"], "Deleted");
        assert_eq!(responses["202"]["description"], "Accepted");
        assert_eq!(responses["205"]["description"], "Reset");
    }

    #[test]
    fn primitive_and_std_types_are_mapped() {
        assert_eq!(
            schema_of("u64 \"Created ID\""),
            json!({ "type": "integer", "format": "int64", "minimum": 0 })
        );
        assert_eq!(
            schema_of("Option<User>"),
            json!({ "anyOf": [{ "$ref": "$User" }, { "type": "null" }] })
        );
        assert_eq!(
            schema_of("Vec<User>"),
            json!({ "type": "array", "items": { "$ref": "$User" } })
        );
    }

    #[test]
    fn custom_generics_stay_raw_for_the_monomorphizer() {
        assert_eq!(schema_of("$Page<User>"), json!({ "$ref": "$Page<User>" }));
        // Framework body wrappers are transparent.
        assert_eq!(schema_of("Json<User>"), json!({ "$ref": "$User" }));
    }

    #[test]
    fn non_rust_names_become_component_refs() {
        assert_eq!(
            schema_of("$User \"Found\""),
            json!({ "$ref": "#/components/schemas/User" })
        );
        assert_eq!(
            schema_of("api-error"),
            json!({ "$ref": "#/components/schemas/api-error" })
        );
    }

    #[test]
    fn inline_schema_with_description() {
        let op = operation(
            &[
                "@route GET /codecs",
                "@return 200: { \"type\": \"array\", \"items\": { \"$ref\": \"#/components/schemas/Codec\" } } \"Hardware encoders\"",
            ],
            "/codecs",
            "get",
        );
        assert_eq!(op["responses"]["200"]["description"], "Hardware encoders");
        assert_eq!(
            response_schema(&op, "200"),
            json!({ "type": "array", "items": { "$ref": "#/components/schemas/Codec" } })
        );
    }

    #[test]
    fn inline_schema_without_description() {
        assert_eq!(schema_of("{ type: string }"), json!({ "type": "string" }));
        // Regression: quoted JSON keys were mistaken for a description.
        assert_eq!(
            schema_of("{ \"type\": \"string\" }"),
            json!({ "type": "string" })
        );
    }

    #[test]
    fn invalid_inline_schema_is_kept_as_raw_ref() {
        assert_eq!(schema_of("{ broken"), json!({ "$ref": "{ broken" }));
    }

    #[test]
    fn unmatched_trailing_quote_does_not_panic() {
        // Regression: these slices used to panic with "byte range starts at .. but ends at ..".
        for residue in ["User\"", "User \""] {
            let op = operation(
                &["@route GET /x", &format!("@return 200: {residue}")],
                "/x",
                "get",
            );
            assert_eq!(op["responses"]["200"]["description"], "OK", "{residue}");
            assert_eq!(response_schema(&op, "200"), json!({ "$ref": "$User" }));
        }
    }

    #[test]
    fn multiple_responses_accumulate() {
        let responses = responses(&["@return 200: User", "@return 404: \"Not Found\""]);
        assert_eq!(responses.as_object().unwrap().len(), 2);
    }

    #[test]
    fn return_without_colon_is_reported_and_ignored() {
        assert_eq!(
            responses(&["@return 200 User"]),
            json!({ "default": { "description": "Default response" } })
        );
        assert_eq!(
            findings(&["@route GET /x", "@return 200 User"]),
            [Code::InvalidDirective]
        );
    }
}

mod dsl_v2 {
    use super::*;
    use crate::common::try_route;
    use oas_forge::diagnostics::Code;

    #[test]
    fn bodies_take_media_types_descriptions_and_optional() {
        let request = operation(
            &[
                "@route POST /x",
                "@body optional $User application/json application/xml \"The user\"",
                "@body $User multipart/form-data",
            ],
            "/x",
            "post",
        )["requestBody"]
            .clone();
        let media: Vec<_> = request["content"]
            .as_object()
            .unwrap()
            .keys()
            .cloned()
            .collect();
        assert_eq!(
            media,
            ["application/json", "application/xml", "multipart/form-data"]
        );
        assert_eq!(request["required"], false);
        assert_eq!(request["description"], "The user");
        assert_eq!(
            findings(&["@route POST /x", "@body"]),
            [Code::InvalidDirective]
        );
        assert_eq!(
            findings(&["@route POST /x", "@body \"only text\""]),
            [Code::InvalidDirective]
        );
    }

    #[test]
    fn responses_take_media_types_and_ranges() {
        let responses = operation(
            &[
                "@route GET /x",
                "@return 2XX: $User application/json application/xml \"Ok\"",
                "@return 2XX: $User text/csv",
                "@return default: unit \"Unexpected\"",
                "@return 304: application/octet-stream",
            ],
            "/x",
            "get",
        )["responses"]
            .clone();
        let media: Vec<_> = responses["2XX"]["content"]
            .as_object()
            .unwrap()
            .keys()
            .cloned()
            .collect();
        assert_eq!(media, ["application/json", "application/xml", "text/csv"]);
        assert_eq!(responses["default"], json!({ "description": "Unexpected" }));
        assert_eq!(
            responses["304"],
            json!({ "description": "Not Modified", "content": { "application/octet-stream": {} } })
        );
        assert_eq!(
            findings(&["@route GET /x", "@return : User"]),
            [Code::InvalidDirective]
        );
    }

    #[test]
    fn streams_use_item_schema() {
        let responses = operation(
            &[
                "@route GET /x",
                "@return 200: stream<$Event>",
                "@return 206: stream<u8> application/jsonl",
            ],
            "/x",
            "get",
        )["responses"]
            .clone();
        assert_eq!(
            responses["200"]["content"],
            json!({ "text/event-stream": { "itemSchema": { "$ref": "#/components/schemas/Event" } } })
        );
        assert_eq!(
            responses["206"]["content"]["application/jsonl"]["itemSchema"]["type"],
            "integer"
        );
    }

    #[test]
    fn response_headers() {
        let responses = operation(
            &[
                "@route GET /x",
                "@return-header 200 X-Rate-Limit: u32 \"Limit\"",
                "@return 200: String",
                "@return-header 429 Retry-After: Option<u64>",
                "@return-header 200 X-Plain:",
            ],
            "/x",
            "get",
        )["responses"]
            .clone();
        assert_eq!(
            responses["200"]["headers"]["X-Rate-Limit"]["description"],
            "Limit"
        );
        assert_eq!(
            responses["200"]["content"]["application/json"]["schema"],
            json!({ "type": "string" })
        );
        assert_eq!(responses["429"]["description"], "Too Many Requests");
        assert_eq!(
            responses["429"]["headers"]["Retry-After"]["required"],
            false
        );
        assert_eq!(
            responses["200"]["headers"]["X-Plain"]["schema"],
            json!({ "type": "string" })
        );
        for bad in [
            "@return-header 200",
            "@return-header 200 Name",
            "@return-header 200 : u8",
            "@return-header 200 X: Vec<",
        ] {
            assert_eq!(
                findings(&["@route GET /x", bad]),
                [Code::InvalidDirective],
                "{bad}"
            );
        }
    }

    #[test]
    fn problems_reference_the_problem_details_schema() {
        let (document, _) =
            try_route(&["@route GET /x", "@problem 404 \"Missing\"", "@problem 500"]);
        let document = document.unwrap();
        let responses = &document["paths"]["/x"]["get"]["responses"];
        assert_eq!(responses["404"]["description"], "Missing");
        assert_eq!(responses["500"]["description"], "Internal Server Error");
        assert_eq!(
            responses["404"]["content"]["application/problem+json"]["schema"],
            json!({ "$ref": "#/components/schemas/ProblemDetails" })
        );
        let problem = &document["components"]["schemas"]["ProblemDetails"];
        let fields: Vec<_> = problem["properties"]
            .as_object()
            .unwrap()
            .keys()
            .cloned()
            .collect();
        assert_eq!(fields, ["type", "title", "status", "detail", "instance"]);
        assert_eq!(
            findings(&["@route GET /x", "@problem"]),
            [Code::InvalidDirective]
        );
        assert_eq!(
            findings(&["@route GET /x", "@problem not a code"]),
            [Code::InvalidDirective]
        );
    }
}
