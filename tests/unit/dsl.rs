use crate::common::{lines, operation, route};
use oas_forge::dsl::parse_route_dsl;
use serde_json::{Value, json};

fn parse(src: &[&str]) -> Option<String> {
    parse_route_dsl(&lines(src), "op")
}

fn params(op: &Value) -> &Vec<Value> {
    op["parameters"].as_array().expect("parameters array")
}

fn param<'a>(op: &'a Value, name: &str) -> &'a Value {
    params(op)
        .iter()
        .find(|p| p["name"] == name)
        .unwrap_or_else(|| panic!("no parameter `{name}` in {op:#}"))
}

fn response_schema(op: &Value, code: &str) -> Value {
    op["responses"][code]["content"]["application/json"]["schema"].clone()
}

mod detection {
    use super::*;

    #[test]
    fn returns_none_without_route_directive() {
        assert!(parse(&["Just docs", "@tag Users"]).is_none());
        assert!(parse(&[]).is_none());
    }

    #[test]
    fn returns_none_when_route_lacks_path() {
        assert!(parse(&["@route GET"]).is_none());
    }

    #[test]
    fn emits_path_item_with_lowercase_method() {
        let doc = route(&["  @route POST /users"]);
        assert!(doc["paths"]["/users"]["post"].is_object());
    }

    #[test]
    fn minimal_operation_has_id_and_empty_collections_only() {
        let op = operation(&["@route GET /ping"], "/ping", "get");
        assert_eq!(
            op,
            json!({
                "operationId": "op",
                "tags": [],
                "parameters": [],
                "responses": {}
            })
        );
    }

    #[test]
    fn output_has_no_yaml_document_marker() {
        let yaml = parse(&["@route GET /ping"]).unwrap();
        assert!(yaml.starts_with("paths:"), "got: {yaml}");
    }
}

mod documentation {
    use super::*;

    #[test]
    fn first_plain_line_is_summary_and_rest_is_description() {
        let op = operation(
            &[
                "Get Users",
                "Returns a list of users.",
                "Paginated.",
                "@route GET /users",
            ],
            "/users",
            "get",
        );
        assert_eq!(op["summary"], "Get Users");
        assert_eq!(op["description"], "Returns a list of users.\nPaginated.");
    }

    #[test]
    fn description_keeps_relative_indentation_and_skips_blank_lines() {
        let op = operation(
            &["Summary", "", "    first", "      nested", "@route GET /x"],
            "/x",
            "get",
        );
        assert_eq!(op["description"], "first\n  nested");
    }

    #[test]
    fn text_after_directives_still_belongs_to_the_docs() {
        let op = operation(&["@route GET /x", "Summary", "Details"], "/x", "get");
        assert_eq!(op["summary"], "Summary");
        assert_eq!(op["description"], "Details");
    }

    #[test]
    fn unknown_directives_are_ignored() {
        let op = operation(&["@route GET /x", "@deprecated", "Summary"], "/x", "get");
        assert_eq!(op["summary"], "Summary");
        assert!(op.get("deprecated").is_none());
    }
}

mod tags {
    use super::*;

    #[test]
    fn splits_trims_and_drops_empty_entries() {
        let op = operation(&["@route GET /x", "@tag Users, Admin ,"], "/x", "get");
        assert_eq!(op["tags"], json!(["Users", "Admin"]));
    }
}

mod path_params {
    use super::*;

    #[test]
    fn inline_param_with_type_and_description() {
        let doc = route(&["@route GET /items/{id: u32 \"Item ID\"}"]);
        let op = &doc["paths"]["/items/{id}"]["get"];

        assert_eq!(
            params(op),
            &vec![json!({
                "name": "id",
                "in": "path",
                "required": true,
                "description": "Item ID",
                "schema": { "type": "integer", "format": "int32" }
            })]
        );
    }

    #[test]
    fn inline_param_with_type_only() {
        let op = operation(&["@route GET /items/{id: Uuid}"], "/items/{id}", "get");
        let id = param(&op, "id");
        assert_eq!(id["schema"]["format"], "uuid");
        assert!(id.get("description").is_none());
    }

    #[test]
    fn inline_param_with_description_only_defaults_to_string() {
        let op = operation(
            &["@route GET /posts/{slug \"Post slug\"}"],
            "/posts/{slug}",
            "get",
        );
        let slug = param(&op, "slug");
        assert_eq!(slug["schema"], json!({ "type": "string" }));
        assert_eq!(slug["description"], "Post slug");
    }

    #[test]
    fn inline_param_with_unparseable_type_falls_back_to_string() {
        let op = operation(&["@route GET /items/{id: Vec<}"], "/items/{id}", "get");
        assert_eq!(param(&op, "id")["schema"], json!({ "type": "string" }));
    }

    #[test]
    fn multiple_inline_params_are_normalized() {
        let doc = route(&["@route GET /orgs/{org: String}/users/{id: u64}"]);
        let op = &doc["paths"]["/orgs/{org}/users/{id}"]["get"];
        let names: Vec<_> = params(op).iter().map(|p| p["name"].clone()).collect();
        assert_eq!(names, [json!("org"), json!("id")]);
    }

    #[test]
    fn bare_placeholder_can_be_declared_with_path_param_directive() {
        let op = operation(
            &[
                "@route GET /users/{id}",
                "@path-param id: Option<u32> \"User ID\"",
            ],
            "/users/{id}",
            "get",
        );
        assert_eq!(params(&op).len(), 1);
        let id = param(&op, "id");
        assert_eq!(id["in"], "path");
        assert_eq!(id["required"], true, "path params are always required");
        assert_eq!(id["description"], "User ID");
    }

    #[test]
    fn undeclared_placeholder_rejects_the_route() {
        assert!(parse(&["@route GET /items/{id}"]).is_none());
    }

    #[test]
    fn declared_but_unused_param_rejects_the_route() {
        assert!(parse(&["@route GET /items", "@path-param id: u32"]).is_none());
    }
}

mod params {
    use super::*;

    #[test]
    fn location_follows_the_directive() {
        let op = operation(
            &[
                "@route GET /x",
                "@query-param q: String",
                "@header-param X-Trace: String",
                "@cookie-param session: String",
            ],
            "/x",
            "get",
        );
        assert_eq!(param(&op, "q")["in"], "query");
        assert_eq!(param(&op, "X-Trace")["in"], "header");
        assert_eq!(param(&op, "session")["in"], "cookie");
    }

    #[test]
    fn primitive_types_are_mapped() {
        let op = operation(
            &[
                "@route GET /test",
                "@query-param q: String",
                "@query-param limit: i32",
                "@query-param active: bool",
            ],
            "/test",
            "get",
        );
        assert_eq!(param(&op, "q")["schema"], json!({ "type": "string" }));
        assert_eq!(
            param(&op, "limit")["schema"],
            json!({ "type": "integer", "format": "int32" })
        );
        assert_eq!(param(&op, "active")["schema"], json!({ "type": "boolean" }));
    }

    #[test]
    fn array_types_are_mapped() {
        let op = operation(
            &[
                "@route GET /test",
                "@query-param tags: [String]",
                "@query-param ids: Vec<i32>",
            ],
            "/test",
            "get",
        );
        assert_eq!(param(&op, "tags")["schema"]["items"]["type"], "string");
        assert_eq!(param(&op, "ids")["schema"]["items"]["type"], "integer");
    }

    #[test]
    fn type_defaults_to_string_when_omitted() {
        let op = operation(
            &[
                "@route GET /test",
                "@query-param simple:",
                "@query-param flagged: deprecated",
                "@query-param forced: required",
                "@query-param sample: example=\"x\"",
                "@query-param described: \"Only a description\"",
            ],
            "/test",
            "get",
        );
        for name in ["simple", "flagged", "forced", "sample", "described"] {
            assert_eq!(
                param(&op, name)["schema"],
                json!({ "type": "string" }),
                "param `{name}`"
            );
        }
        assert_eq!(param(&op, "flagged")["deprecated"], true);
        assert_eq!(param(&op, "sample")["example"], "x");
        assert_eq!(param(&op, "described")["description"], "Only a description");
    }

    #[test]
    fn all_flags_and_description_combine() {
        let op = operation(
            &[
                "@route GET /test",
                "@query-param q: String required deprecated example=\"foo\" \"Search Term\"",
            ],
            "/test",
            "get",
        );
        assert_eq!(
            param(&op, "q"),
            &json!({
                "name": "q",
                "in": "query",
                "required": true,
                "deprecated": true,
                "example": "foo",
                "description": "Search Term",
                "schema": { "type": "string" }
            })
        );
    }

    #[test]
    fn flags_are_order_independent_without_type() {
        let op = operation(
            &[
                "@route GET /search",
                "@query-param sort: deprecated required example=\"desc\" \"Sort Order\"",
            ],
            "/search",
            "get",
        );
        let sort = param(&op, "sort");
        assert_eq!(sort["deprecated"], true);
        assert_eq!(sort["required"], true);
        assert_eq!(sort["example"], "desc");
        assert_eq!(sort["description"], "Sort Order");
    }

    #[test]
    fn option_type_is_optional_unless_marked_required() {
        let op = operation(
            &[
                "@route GET /x",
                "@query-param a: Option<String>",
                "@query-param b: Option<String> required",
            ],
            "/x",
            "get",
        );
        assert_eq!(param(&op, "a")["required"], false);
        assert_eq!(param(&op, "b")["required"], true);
    }

    #[test]
    fn unparseable_type_falls_back_to_required_string() {
        let op = operation(&["@route GET /x", "@query-param q: Vec<"], "/x", "get");
        let q = param(&op, "q");
        assert_eq!(q["schema"], json!({ "type": "string" }));
        assert_eq!(q["required"], true);
    }

    #[test]
    fn param_without_colon_is_ignored() {
        let op = operation(&["@route GET /x", "@query-param q String"], "/x", "get");
        assert!(params(&op).is_empty());
    }
}

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
            json!({ "content": { "application/json": { "schema": { "$ref": "$User" } } } })
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
        assert_eq!(body_schema("Option<User>"), json!({ "$ref": "$User" }));
    }

    #[test]
    fn custom_generics_stay_raw_for_the_monomorphizer() {
        assert_eq!(body_schema("$Page<User>"), json!({ "$ref": "$Page<User>" }));
        assert_eq!(body_schema("Page<User>"), json!({ "$ref": "Page<User>" }));
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

mod responses {
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
    fn description_defaults_to_empty_string() {
        assert_eq!(responses(&["@return 200: User"])["200"]["description"], "");
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
            json!({ "type": "integer", "format": "int64" })
        );
        assert_eq!(schema_of("Option<User>"), json!({ "$ref": "$User" }));
        assert_eq!(
            schema_of("Vec<User>"),
            json!({ "type": "array", "items": { "$ref": "$User" } })
        );
    }

    #[test]
    fn custom_generics_stay_raw_for_the_monomorphizer() {
        assert_eq!(schema_of("$Page<User>"), json!({ "$ref": "$Page<User>" }));
        assert_eq!(schema_of("Json<User>"), json!({ "$ref": "Json<User>" }));
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
            assert_eq!(op["responses"]["200"]["description"], "", "{residue}");
            assert_eq!(response_schema(&op, "200"), json!({ "$ref": "$User" }));
        }
    }

    #[test]
    fn multiple_responses_accumulate() {
        let responses = responses(&["@return 200: User", "@return 404: \"Not Found\""]);
        assert_eq!(responses.as_object().unwrap().len(), 2);
    }

    #[test]
    fn return_without_colon_is_ignored() {
        assert_eq!(responses(&["@return 200 User"]), json!({}));
    }
}

mod security {
    use super::*;

    fn security(directives: &[&str]) -> Value {
        let mut src = vec!["@route GET /x"];
        src.extend_from_slice(directives);
        operation(&src, "/x", "get")["security"].clone()
    }

    #[test]
    fn scheme_without_scopes() {
        assert_eq!(security(&["@security Basic"]), json!([{ "Basic": [] }]));
    }

    #[test]
    fn quoted_scopes_are_unquoted() {
        assert_eq!(
            security(&["@security OAuth2(\"read\", \"write\")"]),
            json!([{ "OAuth2": ["read", "write"] }])
        );
    }

    #[test]
    fn multiple_requirements_accumulate() {
        assert_eq!(
            security(&["@security Basic", "@security oidcAuth(\"read\")"]),
            json!([{ "Basic": [] }, { "oidcAuth": ["read"] }])
        );
    }

    #[test]
    fn empty_parentheses_mean_no_scopes() {
        assert_eq!(security(&["@security OAuth2()"]), json!([{ "OAuth2": [] }]));
    }

    #[test]
    fn unclosed_parenthesis_does_not_panic_or_truncate() {
        // Regression: `OAuth2(` panicked and `OAuth2(read` lost its last character.
        assert_eq!(security(&["@security OAuth2("]), json!([{ "OAuth2": [] }]));
        assert_eq!(
            security(&["@security OAuth2(read"]),
            json!([{ "OAuth2": ["read"] }])
        );
    }
}

mod yaml_overrides {
    use super::*;

    #[test]
    fn raw_yaml_is_merged_into_the_operation() {
        let op = operation(
            &[
                "@route GET /users",
                "responses:",
                "  '200':",
                "    description: Override",
                "servers:",
                "  - url: https://api.example.com",
            ],
            "/users",
            "get",
        );
        assert_eq!(op["responses"]["200"]["description"], "Override");
        assert_eq!(op["servers"][0]["url"], "https://api.example.com");
    }

    #[test]
    fn every_supported_key_starts_an_override_block() {
        let op = operation(
            &[
                "@route GET /x",
                "parameters: []",
                "requestBody: { required: true }",
                "responses: { '200': { description: OK } }",
                "security: []",
                "externalDocs: { url: 'https://docs' }",
                "callbacks: {}",
                "servers: []",
            ],
            "/x",
            "get",
        );
        assert_eq!(op["requestBody"]["required"], true);
        assert_eq!(op["responses"]["200"]["description"], "OK");
        assert_eq!(op["externalDocs"]["url"], "https://docs");
        assert_eq!(op["callbacks"], json!({}));
        assert_eq!(op["security"], json!([]));
        assert!(op.get("summary").is_none(), "override lines are no summary");
    }

    #[test]
    fn yaml_parameters_from_docs_are_used() {
        let op = operation(
            &[
                "@route GET /items",
                "@tag Items",
                "parameters:",
                "  - name: p",
                "    in: query",
                "    schema: { type: integer }",
            ],
            "/items",
            "get",
        );
        assert_eq!(params(&op).len(), 1);
        assert_eq!(params(&op)[0]["name"], "p");
    }

    #[test]
    fn yaml_objects_merge_with_dsl_responses() {
        let op = operation(
            &[
                "@route GET /x",
                "@return 200: User \"OK\"",
                "responses:",
                "  '404': { description: Missing }",
            ],
            "/x",
            "get",
        );
        assert_eq!(op["responses"]["200"]["description"], "OK");
        assert_eq!(op["responses"]["404"]["description"], "Missing");
    }

    #[test]
    fn yaml_arrays_replace_dsl_parameters() {
        let op = operation(
            &[
                "@route GET /x",
                "@query-param dsl: String",
                "parameters:",
                "  - name: yaml",
                "    in: query",
            ],
            "/x",
            "get",
        );
        let names: Vec<_> = params(&op).iter().map(|p| p["name"].clone()).collect();
        assert_eq!(names, [json!("yaml")]);
    }

    #[test]
    fn a_directive_ends_the_override_block() {
        let op = operation(
            &[
                "@route GET /x",
                "responses:",
                "  '200': { description: OK }",
                "@tag T",
                "Summary after block",
            ],
            "/x",
            "get",
        );
        assert_eq!(op["summary"], "Summary after block");
        assert_eq!(op["tags"], json!(["T"]));
    }

    #[test]
    fn invalid_override_yaml_is_ignored() {
        let op = operation(
            &["@route GET /x", "@return 200: User", "responses: [unclosed"],
            "/x",
            "get",
        );
        assert!(op["responses"]["200"].is_object());
    }

    #[test]
    fn unknown_keys_are_treated_as_documentation() {
        let op = operation(&["@route GET /x", "foo: bar"], "/x", "get");
        assert_eq!(op["summary"], "foo: bar");
    }
}
