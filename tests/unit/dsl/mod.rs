//! `src/dsl/`: the route DSL, split like the source modules.

mod channel;
mod operation;
mod params;
mod responses;

use crate::common::{codes, here, lines, operation, route, try_route};
use oas_forge::diagnostics::{Code, Diagnostics, Location, Severity};
use oas_forge::dsl::{self, RouteSource, Signature};
use oas_forge::schema::TypeExpr;
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
    fn conflict_notes_point_at_the_yaml_line() {
        let src = lines(&[
            "@route GET /x",
            "@return 200: String \"DSL\"",
            "@return 404: String \"Missing\"",
            "responses:",
            "  '404': { description: Gone }",
            "  '200':",
            "    content: {}",
            "    description: YAML",
            "parameters:",
            "  - { name: q, in: query, description: DSL }",
            "@query-param q: String \"Query\"",
        ]);
        let numbers: Vec<usize> = (20..20 + src.len()).collect();
        let location = here();
        let mut diagnostics = Diagnostics::new();
        dsl::compile(
            RouteSource::new(&src, &location)
                .operation_id(Some("op"))
                .line_numbers(&numbers),
            &mut diagnostics,
        )
        .unwrap();
        let notes: Vec<String> = diagnostics
            .iter()
            .map(|d| format!("{} {}", d.location().unwrap().line(), d.message()))
            .collect();
        assert_eq!(
            notes,
            [
                "24 embedded YAML overrides the DSL value at `/responses/404/description`",
                "27 embedded YAML overrides the DSL value at `/responses/200/description`",
                // Array items cannot be matched; the list is the best guess.
                "28 embedded YAML overrides the DSL value at `/parameters/0/description`",
            ]
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

mod methods {
    use super::*;

    #[test]
    fn a_method_must_be_a_token() {
        let (document, diagnostics) = try_route(&["@route G(E)T /x"]);
        assert!(document.is_none());
        assert_eq!(codes(&diagnostics), [Code::InvalidRoute]);
        assert!(
            diagnostics
                .iter()
                .next()
                .unwrap()
                .message()
                .contains("`G(E)T` is not an HTTP method")
        );
    }
}

/// Compiles `src` with module `defaults` and a handler `signature`.
fn compile_with(
    src: &[&str],
    defaults: &[&str],
    signature: Option<&Signature>,
) -> (Option<Value>, Diagnostics) {
    let mut diagnostics = Diagnostics::new();
    let block = lines(src);
    let defaults = lines(defaults);
    let location = here();
    let compiled = dsl::compile(
        RouteSource::new(&block, &location)
            .operation_id(Some("op"))
            .defaults(&defaults)
            .signature(signature),
        &mut diagnostics,
    );
    (compiled.map(|c| c.document), diagnostics)
}

mod module_defaults {
    use super::*;

    fn op(src: &[&str], defaults: &[&str], path: &str) -> Value {
        let (document, diagnostics) = compile_with(src, defaults, None);
        let document = document.unwrap_or_else(|| panic!("rejected: {diagnostics:#?}"));
        document["paths"][path]["get"].clone()
    }

    #[test]
    fn inner_module_security_replaces_the_outer_one() {
        let defaults = [
            "@route-defaults",
            "@security bearer",
            "@route-defaults",
            "@security admin_key",
            "@security admin_cert",
        ];
        let op = op(&["@route GET /x"], &defaults, "/x");
        // Not `bearer OR admin_key`: the inner module restricts access.
        assert_eq!(
            op["security"],
            json!([{ "admin_key": [] }, { "admin_cert": [] }])
        );
    }

    #[test]
    fn outer_security_applies_when_the_inner_module_sets_none() {
        let defaults = [
            "@route-defaults",
            "@security bearer",
            "@route-defaults",
            "@tag Inner",
        ];
        let op = op(&["@route GET /x"], &defaults, "/x");
        assert_eq!(op["security"], json!([{ "bearer": [] }]));
        assert_eq!(op["tags"], json!(["Inner"]));
    }

    #[test]
    fn tag_lines_of_one_block_add_up() {
        let op = op(
            &["@route GET /x", "@tag Users", "@tag Admin, Users"],
            &[],
            "/x",
        );
        assert_eq!(op["tags"], json!(["Users", "Admin"]));
        let defaults = [
            "@route-defaults",
            "@tag A",
            "@tag B",
            "@route-defaults",
            "@tag C",
        ];
        assert_eq!(op_tags(&defaults), json!(["C"]));
        assert_eq!(op_tags(&defaults[..3]), json!(["A", "B"]));
    }

    fn op_tags(defaults: &[&str]) -> Value {
        op(&["@route GET /x"], defaults, "/x")["tags"].clone()
    }

    #[test]
    fn yaml_in_defaults_fills_what_the_route_leaves_open() {
        let defaults = [
            "@route-defaults",
            "x-rate-limit: 10",
            "x-owner: team-a",
            "parameters:",
            "  - { name: trace, in: header, schema: { type: string } }",
            "  - { name: page, in: query, schema: { type: integer } }",
        ];
        let op = op(
            &[
                "@route GET /x",
                "@query-param page: u8 \"Own\"",
                "x-owner: team-b",
            ],
            &defaults,
            "/x",
        );
        assert_eq!(op["x-rate-limit"], 10);
        assert_eq!(op["x-owner"], "team-b", "the route wins");
        let names: Vec<_> = params(&op).iter().map(|p| p["name"].clone()).collect();
        assert_eq!(names, [json!("page"), json!("trace")]);
        assert_eq!(param(&op, "page")["description"], "Own");
    }

    #[test]
    fn path_and_document_yaml_in_defaults() {
        let defaults = [
            "@route-defaults",
            "@openapi path",
            "summary: Shared",
            "x-scope: module",
            "@openapi document",
            "components:",
            "  schemas:",
            "    Shared: { type: string }",
        ];
        let (document, diagnostics) = compile_with(
            &["@route GET /x", "@openapi path", "x-scope: route"],
            &defaults,
            None,
        );
        assert!(diagnostics.is_empty(), "{diagnostics:#?}");
        let document = document.unwrap();
        let item = &document["paths"]["/x"];
        assert_eq!(item["summary"], "Shared");
        assert_eq!(item["x-scope"], "route", "the route wins");
        assert_eq!(
            document["components"]["schemas"]["Shared"],
            json!({ "type": "string" })
        );
    }

    #[test]
    fn no_infer_in_defaults_turns_inference_off() {
        let mut signature = Signature::default();
        signature.response = TypeExpr::parse("User");
        let (document, _) = compile_with(
            &["@route GET /x"],
            &["@route-defaults", "@no-infer"],
            Some(&signature),
        );
        let document = document.unwrap();
        assert_eq!(
            document["paths"]["/x"]["get"]["responses"],
            json!({ "default": { "description": "Default response" } })
        );
    }

    #[test]
    fn prefixes_are_joined_with_one_slash_and_declare_inline_parameters() {
        let defaults = [
            "@route-defaults",
            "@prefix /api/",
            "@route-defaults",
            "@prefix tenants/{tenant: String \"Tenant slug\"}",
        ];
        let (document, diagnostics) = compile_with(&["@route GET users"], &defaults, None);
        assert!(diagnostics.is_empty(), "{diagnostics:#?}");
        let document = document.unwrap();
        let op = &document["paths"]["/api/tenants/{tenant}/users"]["get"];
        assert_eq!(
            param(op, "tenant"),
            json!({ "name": "tenant", "in": "path", "required": true, "description": "Tenant slug", "schema": { "type": "string" } })
        );
    }

    #[test]
    fn the_root_route_under_a_prefix_is_the_prefix() {
        let defaults = ["@route-defaults", "@prefix /v2/tenants/{tenant: String}/"];
        let (document, diagnostics) = compile_with(&["@route GET /"], &defaults, None);
        assert!(diagnostics.is_empty(), "{diagnostics:#?}");
        let paths = document.unwrap()["paths"].clone();
        assert_eq!(
            paths.as_object().unwrap().keys().collect::<Vec<_>>(),
            ["/v2/tenants/{tenant}"]
        );
        // Without a prefix, `/` stays the root.
        assert!(route(&["@route GET /"])["paths"]["/"]["get"].is_object());
        let (document, _) = compile_with(&["@route GET /users/"], &defaults, None);
        assert!(document.unwrap()["paths"]["/v2/tenants/{tenant}/users/"].is_object());
    }

    #[test]
    fn webhooks_get_no_path_parameters_from_defaults() {
        let defaults = [
            "@route-defaults",
            "@prefix /tenants/{tenant}",
            "@path-param tenant: String",
            "@header-param X-Trace: String",
        ];
        let (document, diagnostics) = compile_with(&["@webhook orderPaid"], &defaults, None);
        assert!(diagnostics.is_empty(), "{diagnostics:#?}");
        let op = &document.unwrap()["webhooks"]["orderPaid"]["post"];
        let names: Vec<_> = params(op).iter().map(|p| p["name"].clone()).collect();
        assert_eq!(names, [json!("X-Trace")]);
    }

    #[test]
    fn extend_markers_of_defaults_and_route_do_not_collide() {
        let marker = |n: u8| format!("x-oas-forge-extend-{n}: 'Frag{n}'");
        let route_marker = marker(1);
        let default_marker = marker(1);
        let (document, _) = compile_with(
            &["@route GET /x", &route_marker],
            &["@route-defaults", &default_marker],
            None,
        );
        let op = &document.unwrap()["paths"]["/x"]["get"];
        assert_eq!(op["x-oas-forge-extend-1"], "Frag1");
        assert_eq!(op["x-oas-forge-extend-defaults-1"], "Frag1");
    }
}

mod block_lines {
    use super::*;

    #[test]
    fn an_extend_marker_does_not_start_a_yaml_section() {
        let op = operation(
            &[
                "Summary",
                "x-oas-forge-extend-1: 'Common'",
                "More description.",
                "@route GET /x",
            ],
            "/x",
            "get",
        );
        assert_eq!(op["description"], "More description.");
        assert_eq!(op["x-oas-forge-extend-1"], "Common");
    }

    #[test]
    fn prose_starting_with_x_is_not_yaml() {
        let src = [
            "Summary",
            "x-forwarded headers are honoured.",
            "x-request-id: echoed",
            "@route GET /x",
        ];
        assert_eq!(findings(&src), []);
        let op = operation(&src, "/x", "get");
        assert_eq!(op["description"], "x-forwarded headers are honoured.");
        assert_eq!(op["x-request-id"], "echoed");
    }

    #[test]
    fn a_path_without_leading_slash_is_normalized() {
        assert!(route(&["@route GET users"])["paths"]["/users"]["get"].is_object());
    }
}

mod inference {
    use super::*;

    fn with(signature: &Signature, src: &[&str]) -> Value {
        let (document, diagnostics) = compile_with(src, &[], Some(signature));
        let document = document.unwrap_or_else(|| panic!("rejected: {diagnostics:#?}"));
        document["paths"]["/x"]["get"].clone()
    }

    fn returning(ty: &str) -> Signature {
        let mut signature = Signature::default();
        signature.response = TypeExpr::parse(ty);
        signature
    }

    #[test]
    fn a_default_response_does_not_stop_the_success_response() {
        let op = with(
            &returning("User"),
            &["@route GET /x", "@return default: $Error \"Unexpected\""],
        );
        let codes: Vec<_> = op["responses"]
            .as_object()
            .unwrap()
            .keys()
            .cloned()
            .collect();
        assert_eq!(
            codes,
            ["200", "default"],
            "the success response comes first"
        );
        assert_eq!(response_schema(&op, "200"), json!({ "$ref": "$User" }));
    }

    #[test]
    fn a_documented_success_response_wins() {
        for code in ["201", "2XX"] {
            let op = with(
                &returning("User"),
                &["@route GET /x", &format!("@return {code}: $Other")],
            );
            assert!(op["responses"].get("200").is_none(), "{code}");
        }
    }

    #[test]
    fn single_query_parameters_do_not_stop_query_inference() {
        let mut signature = Signature::default();
        signature.query = TypeExpr::parse("ListParams");
        let op = with(&signature, &["@route GET /x", "@query-param sort: String"]);
        assert_eq!(
            params(&op)[1],
            json!({ "x-oas-forge-expand": { "in": "query", "schema": "$ListParams", "file": "test.rs", "line": 1 } })
        );
        for declared in ["@query-params $Other", "@querystring-param q: $Filter"] {
            let op = with(&signature, &["@route GET /x", declared]);
            assert_eq!(params(&op).len(), 1, "{declared}");
        }
    }

    #[test]
    fn optional_query_structs_mark_their_expansion() {
        let mut signature = Signature::default();
        signature.query = TypeExpr::parse("ListParams");
        signature.optional_query = true;
        let op = with(&signature, &["@route GET /x"]);
        assert_eq!(params(&op)[0]["x-oas-forge-expand"]["optional"], true);
    }

    #[test]
    fn an_optional_body_is_not_required() {
        let mut signature = Signature::default();
        signature.body = TypeExpr::parse("User").map(|t| (t, "application/json".to_string()));
        signature.optional_body = true;
        let op = with(&signature, &["@route GET /x"]);
        assert_eq!(op["requestBody"]["required"], false);
        assert_eq!(
            op["requestBody"]["content"]["application/json"]["schema"],
            json!({ "$ref": "$User" })
        );
    }
}
