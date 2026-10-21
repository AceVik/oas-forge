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
    fn optional_is_a_word_of_its_own() {
        assert_eq!(
            body(&["@route POST /x", "@body optionalSettings"]),
            json!({ "content": { "application/json": { "schema": { "$ref": "$optionalSettings" } } }, "required": true })
        );
    }

    #[test]
    fn half_media_types_are_not_media_types() {
        for token in ["text/", "/plain"] {
            let body = body(&["@route POST /x", &format!("@body String {token}")]);
            assert!(
                body["content"].get(token).is_none(),
                "{token}: {}",
                body["content"]
            );
        }
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
    fn sigil_names_are_smart_references() {
        // Resolved later, like `$ref: $User` in YAML.
        assert_eq!(body_schema("$User"), json!({ "$ref": "$User" }));
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
    fn every_registered_status_code_has_its_reason_phrase() {
        let table = [
            ("100", "Continue"),
            ("101", "Switching Protocols"),
            ("200", "OK"),
            ("201", "Created"),
            ("202", "Accepted"),
            ("203", "Non-Authoritative Information"),
            ("204", "No Content"),
            ("205", "Reset Content"),
            ("206", "Partial Content"),
            ("207", "Multi-Status"),
            ("300", "Multiple Choices"),
            ("301", "Moved Permanently"),
            ("302", "Found"),
            ("303", "See Other"),
            ("304", "Not Modified"),
            ("307", "Temporary Redirect"),
            ("308", "Permanent Redirect"),
            ("400", "Bad Request"),
            ("401", "Unauthorized"),
            ("402", "Payment Required"),
            ("403", "Forbidden"),
            ("404", "Not Found"),
            ("405", "Method Not Allowed"),
            ("406", "Not Acceptable"),
            ("408", "Request Timeout"),
            ("409", "Conflict"),
            ("410", "Gone"),
            ("411", "Length Required"),
            ("412", "Precondition Failed"),
            ("413", "Content Too Large"),
            ("414", "URI Too Long"),
            ("415", "Unsupported Media Type"),
            ("416", "Range Not Satisfiable"),
            ("417", "Expectation Failed"),
            ("421", "Misdirected Request"),
            ("422", "Unprocessable Content"),
            ("423", "Locked"),
            ("424", "Failed Dependency"),
            ("425", "Too Early"),
            ("426", "Upgrade Required"),
            ("428", "Precondition Required"),
            ("429", "Too Many Requests"),
            ("431", "Request Header Fields Too Large"),
            ("451", "Unavailable For Legal Reasons"),
            ("500", "Internal Server Error"),
            ("501", "Not Implemented"),
            ("502", "Bad Gateway"),
            ("503", "Service Unavailable"),
            ("504", "Gateway Timeout"),
            ("505", "HTTP Version Not Supported"),
            ("1XX", "Informational"),
            ("2XX", "Success"),
            ("3XX", "Redirection"),
            ("4XX", "Client Error"),
            ("5XX", "Server Error"),
            ("default", "Default response"),
            // Codes without a registered phrase.
            ("299", "Response"),
            ("599", "Response"),
        ];
        let returns: Vec<String> = table
            .iter()
            .map(|(code, _)| format!("@return {code}:"))
            .collect();
        let returns: Vec<&str> = returns.iter().map(String::as_str).collect();
        let responses = responses(&returns);
        for (code, phrase) in table {
            assert_eq!(responses[code]["description"], phrase, "{code}");
        }
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
    fn sigil_and_non_rust_names_become_references() {
        assert_eq!(schema_of("$User \"Found\""), json!({ "$ref": "$User" }));
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
    fn invalid_inline_schema_is_reported_and_ignored() {
        let src = [
            "@route GET /x",
            "@return 200: { type: array, items: { type: string }",
        ];
        assert_eq!(findings(&src), [Code::InvalidDirective]);
        let op = operation(&src, "/x", "get");
        assert_eq!(
            op["responses"],
            json!({ "default": { "description": "Default response" } })
        );
    }

    #[test]
    fn text_that_is_neither_type_nor_schema_name_is_reported() {
        let src = ["@route GET /x", "@return 200: User Profile"];
        assert_eq!(findings(&src), [Code::InvalidDirective]);
        assert_eq!(
            findings(&["@route POST /x", "@body { broken"]),
            [Code::InvalidDirective]
        );
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
            json!({ "text/event-stream": { "itemSchema": { "$ref": "$Event" } } })
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
        // Response headers are not required by default (OpenAPI's default).
        assert!(
            responses["429"]["headers"]["Retry-After"]
                .get("required")
                .is_none()
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

/// `@return-header` flags: `required`, and `optional` for readability.
mod response_header_flags {
    use super::*;

    fn headers(lines: &[&str]) -> Value {
        let mut src = vec!["@route GET /x"];
        src.extend_from_slice(lines);
        operation(&src, "/x", "get")["responses"]["200"]["headers"].clone()
    }

    #[test]
    fn headers_are_required_only_with_the_flag() {
        let headers = headers(&[
            "@return-header 200 Content-Language: String \"The language\"",
            "@return-header 200 X-Request-Id: String required \"Correlation id\"",
            "@return-header 200 X-Trace: u64 optional",
            "@return-header 200 X-Both: optional \"No type\"",
            "@return-header 200 X-Maybe: Option<u32> required",
        ]);
        assert_eq!(
            headers["Content-Language"],
            json!({ "schema": { "type": "string" }, "description": "The language" })
        );
        assert_eq!(
            headers["X-Request-Id"],
            json!({ "schema": { "type": "string" }, "required": true, "description": "Correlation id" })
        );
        assert!(headers["X-Trace"].get("required").is_none());
        assert_eq!(headers["X-Trace"]["schema"]["type"], "integer");
        assert_eq!(
            headers["X-Both"],
            json!({ "schema": { "type": "string" }, "description": "No type" })
        );
        assert_eq!(headers["X-Maybe"]["required"], true);
    }

    #[test]
    fn other_words_after_the_type_are_rejected() {
        assert_eq!(
            findings(&["@route GET /x", "@return-header 200 X: String sometimes"]),
            [Code::InvalidDirective]
        );
    }
}

/// Several lines for one status code: one response, their descriptions
/// combined when they agree on the content.
mod merged_responses {
    use super::*;
    use crate::common::{here, lines};
    use oas_forge::diagnostics::{Diagnostics, Location, Severity};
    use oas_forge::dsl::{self, RouteSource};

    /// The responses of `GET /x` and the findings, with the block on lines
    /// 10, 11, … and the defaults on lines 1, 2, ….
    fn compiled(block: &[&str], defaults: &[&str]) -> (Value, Diagnostics) {
        let block = lines(block);
        let numbers: Vec<usize> = (10..10 + block.len()).collect();
        let defaults = lines(defaults);
        let location = here();
        let mut diagnostics = Diagnostics::new();
        let route = dsl::compile(
            RouteSource::new(&block, &location)
                .line_numbers(&numbers)
                .defaults(&defaults),
            &mut diagnostics,
        )
        .unwrap_or_else(|| panic!("rejected: {diagnostics:#?}"));
        let responses = route.document["paths"]["/x"]["get"]["responses"].clone();
        (responses, diagnostics)
    }

    fn error_body() -> Value {
        json!({ "application/json": { "schema": { "$ref": "$ErrorBody" } } })
    }

    #[test]
    fn lines_with_the_same_content_combine_their_descriptions() {
        let (responses, diagnostics) = compiled(
            &[
                "@route GET /x",
                "@return 403: $ErrorBody \"The account is banned\"",
                "@return 403: $ErrorBody \"The password is wrong\"",
                "@return 403: $ErrorBody \"The account is banned\"",
                "@return 403: $ErrorBody",
                "@return 404: $ErrorBody \"No such deck\"",
                "@return 404: $ErrorBody",
                "@return 409: $ErrorBody",
            ],
            &[],
        );
        assert!(diagnostics.is_empty(), "{diagnostics:#?}");
        assert_eq!(
            responses["403"],
            json!({
                "description": "- The account is banned\n- The password is wrong",
                "content": error_body(),
            })
        );
        assert_eq!(responses["404"]["description"], "No such deck");
        assert_eq!(responses["409"]["description"], "Conflict");
    }

    #[test]
    fn defaults_and_the_block_combine_in_source_order() {
        let (responses, diagnostics) = compiled(
            &[
                "@route GET /x",
                "@return 403: $ErrorBody \"Own\"",
                "@problem 401 \"Token expired\"",
                "@return 200: String",
            ],
            &[
                "@route-defaults",
                "@return 403: $ErrorBody \"Outer\"",
                "@problem 401 \"Not signed in\"",
                "@return 200: String \"From the module\"",
                "@return-header 200 X-Rate: u32",
                "@route-defaults",
                "@return 403: $ErrorBody \"Inner\"",
                "@return 403: $ErrorBody \"Outer\"",
            ],
        );
        assert!(diagnostics.is_empty(), "{diagnostics:#?}");
        assert_eq!(responses["403"]["description"], "- Outer\n- Inner\n- Own");
        assert_eq!(responses["403"]["content"], error_body());
        assert_eq!(
            responses["401"]["description"],
            "- Not signed in\n- Token expired"
        );
        // The module's headers stay; a line without description adds nothing.
        assert_eq!(responses["200"]["description"], "From the module");
        assert!(responses["200"]["headers"]["X-Rate"].is_object());
    }

    #[test]
    fn a_header_of_the_block_keeps_the_body_of_the_defaults() {
        let (responses, diagnostics) = compiled(
            &["@route GET /x", "@return-header 200 X-Total: u64"],
            &["@route-defaults", "@return 200: String \"Items\""],
        );
        assert!(diagnostics.is_empty(), "{diagnostics:#?}");
        assert_eq!(responses["200"]["description"], "Items");
        assert_eq!(
            responses["200"]["content"]["application/json"]["schema"],
            json!({ "type": "string" })
        );
        assert!(responses["200"]["headers"]["X-Total"].is_object());
    }

    #[test]
    fn another_schema_for_a_media_type_in_the_block_wins_with_a_note() {
        let (responses, diagnostics) = compiled(
            &[
                "@route GET /x",
                "@tag Decks",
                "@return 404: String \"No such deck\"",
            ],
            &["@route-defaults", "@return 404: $ErrorBody \"Missing\""],
        );
        assert_eq!(
            responses["404"],
            json!({
                "description": "No such deck",
                "content": { "application/json": { "schema": { "type": "string" } } },
            })
        );
        let found: Vec<(Code, Severity, Location)> = diagnostics
            .iter()
            .map(|d| (d.code(), d.severity(), d.location().unwrap().clone()))
            .collect();
        assert_eq!(
            found,
            [(
                Code::MergeConflict,
                Severity::Note,
                Location::new("test.rs", 12)
            )]
        );
        assert!(
            diagnostics.iter().any(|d| d.message().contains("`404`")),
            "{diagnostics:#?}"
        );
    }

    #[test]
    fn other_media_types_add_up_with_their_descriptions() {
        // In the block, and between the defaults and the block; a response
        // without content takes the content of the other.
        let (responses, diagnostics) = compiled(
            &[
                "@route GET /x",
                "@return 413: $ErrorBody \"a\"",
                "@return 413: String text/plain \"b\"",
                "@return 415: String text/plain \"Own\"",
                "@return 204:",
            ],
            &[
                "@route-defaults",
                "@return 415: $ErrorBody \"From the module\"",
                "@return 204: $ErrorBody \"Body\"",
            ],
        );
        assert!(diagnostics.is_empty(), "{diagnostics:#?}");
        let both = json!({
            "application/json": { "schema": { "$ref": "$ErrorBody" } },
            "text/plain": { "schema": { "type": "string" } },
        });
        assert_eq!(
            responses["413"],
            json!({ "description": "- a\n- b", "content": both })
        );
        assert_eq!(
            responses["415"],
            json!({ "description": "- From the module\n- Own", "content": both })
        );
        assert_eq!(
            responses["204"],
            json!({ "description": "Body", "content": error_body() })
        );
    }

    #[test]
    fn embedded_yaml_adds_its_description() {
        let (responses, diagnostics) = compiled(
            &[
                "@route GET /x",
                "@return 403: $ErrorBody \"A\"",
                "@return 401: $ErrorBody",
                "responses:",
                "  '403': { description: B }",
                "  '401': { description: Who are you? }",
                "  '400': { description: Bad }",
            ],
            &[],
        );
        assert!(diagnostics.is_empty(), "{diagnostics:#?}");
        assert_eq!(responses["403"]["description"], "- A\n- B");
        assert_eq!(responses["403"]["content"], error_body());
        assert_eq!(responses["401"]["description"], "Who are you?");
        assert_eq!(responses["400"], json!({ "description": "Bad" }));
    }

    #[test]
    fn embedded_yaml_with_another_media_type_adds_it() {
        let (responses, diagnostics) = compiled(
            &[
                "@route GET /x",
                "@return 403: $ErrorBody \"A\"",
                "responses:",
                "  '403': { description: B, content: { text/plain: { schema: { type: string } } } }",
            ],
            &[],
        );
        assert!(diagnostics.is_empty(), "{diagnostics:#?}");
        assert_eq!(responses["403"]["description"], "- A\n- B");
        assert_eq!(
            responses["403"]["content"]["text/plain"],
            json!({ "schema": { "type": "string" } })
        );
        assert_eq!(
            responses["403"]["content"]["application/json"],
            error_body()["application/json"]
        );
    }

    #[test]
    fn embedded_yaml_with_another_content_wins_as_before() {
        let (responses, diagnostics) = compiled(
            &[
                "@route GET /x",
                "@return 403: $ErrorBody \"A\"",
                "responses:",
                "  '403':",
                "    description: C",
                "    content: { application/json: { schema: { type: string } } }",
            ],
            &[],
        );
        assert_eq!(responses["403"]["description"], "C");
        assert_eq!(
            codes_and_severities(&diagnostics),
            [(Code::MergeConflict, Severity::Note)]
        );
    }

    fn codes_and_severities(diagnostics: &Diagnostics) -> Vec<(Code, Severity)> {
        diagnostics
            .iter()
            .map(|d| (d.code(), d.severity()))
            .collect()
    }
}
