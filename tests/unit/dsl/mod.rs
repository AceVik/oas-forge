//! `src/dsl/`: the route DSL, split like the source modules.

mod operation;
mod params;
mod responses;

use crate::common::{codes, here, lines, operation, route, try_route};
use oas_forge::diagnostics::{Code, Diagnostics, Location, Severity};
use oas_forge::dsl::{self, RouteSource};
use serde_json::{Value, json};

/// Compiles `src` and returns the codes of the findings.
pub(super) fn findings(src: &[&str]) -> Vec<Code> {
    codes(&try_route(src).1)
}

/// The `parameters` of an operation (empty when there are none).
pub(super) fn params(op: &Value) -> Vec<Value> {
    op.get("parameters")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default()
}

/// The parameter called `name`.
pub(super) fn param(op: &Value, name: &str) -> Value {
    params(op)
        .into_iter()
        .find(|p| p["name"] == name)
        .unwrap_or_else(|| panic!("no parameter `{name}` in {op:#}"))
}

/// The JSON schema of response `code`.
pub(super) fn response_schema(op: &Value, code: &str) -> Value {
    op["responses"][code]["content"]["application/json"]["schema"].clone()
}

mod detection {
    use super::*;

    #[test]
    fn route_blocks_are_recognized_by_the_directive() {
        assert!(dsl::is_route(&lines(&["Docs", "  @route GET /x"])));
        assert!(!dsl::is_route(&lines(&["Just docs", "@tag Users"])));
        assert!(!dsl::is_route(&[]));
    }

    #[test]
    fn blocks_without_route_are_rejected() {
        for src in [&["Just docs", "@tag Users"][..], &[]] {
            let (document, diagnostics) = try_route(src);
            assert!(document.is_none());
            assert_eq!(codes(&diagnostics), [Code::InvalidRoute]);
            assert!(diagnostics.has_errors());
        }
    }

    #[test]
    fn a_route_without_path_is_rejected_once_at_its_line() {
        let mut diagnostics = Diagnostics::new();
        let lines = lines(&["Docs", "@route GET"]);
        let location = Location::new("api.rs", 10);
        let compiled = dsl::compile(
            RouteSource::new(&lines, &location).line_numbers(&[10, 11]),
            &mut diagnostics,
        );
        assert!(compiled.is_none());
        assert_eq!(codes(&diagnostics), [Code::InvalidRoute]);
        assert_eq!(
            diagnostics.iter().next().unwrap().location(),
            Some(&Location::new("api.rs", 11))
        );
    }

    #[test]
    fn emits_path_item_with_lowercase_method() {
        let doc = route(&["  @route POST /users"]);
        assert!(doc["paths"]["/users"]["post"].is_object());
    }

    #[test]
    fn minimal_operation_has_only_id_and_responses() {
        // Changed in 0.2: empty `tags` and `parameters` are left out.
        let op = operation(&["@route GET /ping"], "/ping", "get");
        assert_eq!(
            op,
            json!({ "operationId": "op", "responses": { "default": { "description": "Default response" } } }),
            "OpenAPI requires at least one response"
        );
    }

    #[test]
    fn compiled_routes_name_their_path_and_method() {
        let lines = lines(&["@route PATCH /items/{id: u32}"]);
        let compiled = dsl::compile(
            RouteSource::new(&lines, &here()).operation_id(Some("op")),
            &mut Diagnostics::new(),
        )
        .unwrap();
        assert_eq!(
            (compiled.path.as_str(), compiled.method.as_str()),
            ("/items/{id}", "patch")
        );
        assert!(compiled.document["paths"]["/items/{id}"]["patch"].is_object());
    }

    #[test]
    fn routes_without_function_get_a_stable_operation_id() {
        // Regression: virtual routes were named after their line number.
        let mut diagnostics = Diagnostics::new();
        let lines = lines(&["@route GET /legacy/{id}/proxy-v2", "@path-param id: u32"]);
        let compiled = dsl::compile(RouteSource::new(&lines, &here()), &mut diagnostics).unwrap();
        assert_eq!(
            compiled.document["paths"]["/legacy/{id}/proxy-v2"]["get"]["operationId"],
            "get_legacy_id_proxy_v2"
        );
    }

    #[test]
    fn default_tags_apply_unless_the_route_has_its_own() {
        let defaults = vec!["Module".to_string()];
        for (src, expected) in [
            (&["@route GET /x"][..], json!(["Module"])),
            (&["@route GET /x", "@tag Own"][..], json!(["Own"])),
        ] {
            let lines = lines(src);
            let compiled = dsl::compile(
                RouteSource::new(&lines, &here())
                    .operation_id(Some("op"))
                    .default_tags(&defaults),
                &mut Diagnostics::new(),
            )
            .unwrap();
            assert_eq!(compiled.document["paths"]["/x"]["get"]["tags"], expected);
        }
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
    fn description_keeps_paragraphs() {
        let op = operation(
            &[
                "Summary",
                "First",
                "paragraph.",
                "",
                "Second.",
                "@route GET /x",
            ],
            "/x",
            "get",
        );
        assert_eq!(op["description"], "First\nparagraph.\n\nSecond.");
    }

    #[test]
    fn unknown_directives_are_reported_and_ignored() {
        let (document, diagnostics) = try_route(&["@route GET /x", "@frobnicate", "Summary"]);
        let op = &document.unwrap()["paths"]["/x"]["get"];
        assert_eq!(op["summary"], "Summary");
        assert!(op.get("frobnicate").is_none());
        assert_eq!(codes(&diagnostics), [Code::UnknownDirective]);
        assert!(!diagnostics.has_errors());
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
    fn yaml_parameters_are_merged_by_name_and_location() {
        // Changed in 0.2: YAML `parameters` no longer replace the DSL ones.
        let op = operation(
            &[
                "@route GET /x",
                "@query-param dsl: String",
                "@query-param both: u32",
                "parameters:",
                "  - name: yaml",
                "    in: query",
                "  - name: both",
                "    in: query",
                "    description: from YAML",
            ],
            "/x",
            "get",
        );
        let names: Vec<_> = params(&op).iter().map(|p| p["name"].clone()).collect();
        assert_eq!(names, [json!("dsl"), json!("both"), json!("yaml")]);
        assert_eq!(param(&op, "both")["description"], "from YAML");
        assert_eq!(
            param(&op, "both")["schema"]["format"],
            "int64",
            "kept from the DSL"
        );
    }

    #[test]
    fn yaml_wins_conflicts_with_a_note() {
        let (document, diagnostics) = try_route(&[
            "@route GET /x",
            "@return 200: String \"DSL\"",
            "responses:",
            "  '200': { description: YAML }",
        ]);
        assert_eq!(
            document.unwrap()["paths"]["/x"]["get"]["responses"]["200"]["description"],
            "YAML"
        );
        let note = diagnostics.iter().next().unwrap();
        assert_eq!(
            (note.code(), note.severity()),
            (Code::MergeConflict, Severity::Note)
        );
        assert!(
            note.message().contains("/responses/200/description"),
            "{note}"
        );
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
    fn invalid_override_yaml_is_reported_and_ignored() {
        let src = ["@route GET /x", "@return 200: User", "responses: [unclosed"];
        let op = operation(&src, "/x", "get");
        assert!(op["responses"]["200"].is_object());
        assert_eq!(findings(&src), [Code::InvalidRouteYaml]);
    }

    #[test]
    fn unknown_keys_are_treated_as_documentation() {
        let op = operation(&["@route GET /x", "foo: bar"], "/x", "get");
        assert_eq!(op["summary"], "foo: bar");
    }

    #[test]
    fn extension_keys_start_an_override_block() {
        let op = operation(
            &["@route GET /x", "x-codeSamples:", "  - lang: sh"],
            "/x",
            "get",
        );
        assert_eq!(op["x-codeSamples"], json!([{ "lang": "sh" }]));
        assert!(op.get("summary").is_none());
    }

    #[test]
    fn blank_lines_inside_a_block_are_kept() {
        let op = operation(
            &[
                "@route GET /x",
                "responses:",
                "  '200':",
                "",
                "    description: |",
                "      a",
                "",
                "      b",
            ],
            "/x",
            "get",
        );
        // A `|` block that ends the text keeps a final line break.
        assert_eq!(op["responses"]["200"]["description"], "a\n\nb\n");
    }
}

mod scopes {
    use super::*;

    #[test]
    fn path_scope_targets_the_path_item() {
        let doc = route(&[
            "@route GET /x",
            "@openapi path",
            "servers: [{ url: 'https://edge' }]",
            "summary: Shared",
        ]);
        let item = &doc["paths"]["/x"];
        assert_eq!(item["servers"][0]["url"], "https://edge");
        assert_eq!(item["summary"], "Shared");
        assert!(item["get"].get("servers").is_none());
    }

    #[test]
    fn document_scope_targets_the_document() {
        let doc = route(&[
            "@route GET /x",
            "@openapi document",
            "components:",
            "  responses:",
            "    NotFound: { description: Missing }",
            "@openapi",
            "deprecated: true",
        ]);
        assert_eq!(
            doc["components"]["responses"]["NotFound"]["description"],
            "Missing"
        );
        assert_eq!(doc["paths"]["/x"]["get"]["deprecated"], true);
    }

    #[test]
    fn operation_scope_accepts_any_operation_field() {
        let op = operation(
            &[
                "@route GET /x",
                "@openapi operation",
                "deprecated: true",
                "summary: S",
            ],
            "/x",
            "get",
        );
        assert_eq!(op["deprecated"], true);
        assert_eq!(op["summary"], "S");
    }

    #[test]
    fn unknown_scopes_fall_back_to_the_operation() {
        let src = ["@route GET /x", "@openapi everywhere", "deprecated: true"];
        assert_eq!(operation(&src, "/x", "get")["deprecated"], true);
        assert_eq!(findings(&src), [Code::InvalidDirective]);
    }

    #[test]
    fn non_mapping_blocks_are_reported_and_ignored() {
        // Found by the property tests: a scalar replaced the whole operation.
        for src in [
            &["@route GET /x", "@openapi", "0"][..],
            &["@route GET /x", "@openapi path", "- a"][..],
            &["@route GET /x", "@openapi document", "text"][..],
        ] {
            let (document, diagnostics) = try_route(src);
            assert_eq!(document.unwrap(), route(&["@route GET /x"]), "{src:?}");
            assert_eq!(codes(&diagnostics), [Code::InvalidRouteYaml], "{src:?}");
        }
    }

    #[test]
    fn empty_and_null_blocks_change_nothing() {
        let plain = route(&["@route GET /x"]);
        assert_eq!(
            route(&[
                "@route GET /x",
                "@openapi path",
                "",
                "@openapi document",
                "~"
            ]),
            plain
        );
    }
}
