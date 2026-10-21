//! `src/dsl/params.rs`: path, query, header and cookie parameters.

use super::{findings, param, params};
use crate::common::{codes, operation, route, try_route};
use oas_forge::diagnostics::Code;
use serde_json::json;

mod path_params {
    use super::*;

    #[test]
    fn inline_param_with_type_and_description() {
        let doc = route(&["@route GET /items/{id: u32 \"Item ID\"}"]);
        let op = &doc["paths"]["/items/{id}"]["get"];

        assert_eq!(
            params(op),
            vec![json!({
                "name": "id",
                "in": "path",
                "required": true,
                "description": "Item ID",
                "schema": { "type": "integer", "format": "int64", "minimum": 0, "maximum": 4294967295u32 }
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
    fn inline_param_with_unparsable_type_falls_back_to_string() {
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
        let (document, diagnostics) = try_route(&["@route GET /items/{id}"]);
        assert!(document.is_none());
        assert_eq!(codes(&diagnostics), [Code::UndeclaredPathParam]);
    }

    #[test]
    fn declared_but_unused_param_rejects_the_route() {
        let (document, diagnostics) = try_route(&["@route GET /items", "@path-param id: u32"]);
        assert!(document.is_none());
        assert_eq!(codes(&diagnostics), [Code::UnusedPathParam]);
    }
}

mod declarations {
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
            json!({
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
    fn unparsable_type_falls_back_to_required_string() {
        let op = operation(&["@route GET /x", "@query-param q: Vec<"], "/x", "get");
        let q = param(&op, "q");
        assert_eq!(q["schema"], json!({ "type": "string" }));
        assert_eq!(q["required"], true);
    }

    #[test]
    fn param_without_colon_is_reported_and_ignored() {
        let op = operation(&["@route GET /x", "@query-param q String"], "/x", "get");
        assert!(params(&op).is_empty());
        assert_eq!(
            findings(&["@route GET /x", "@query-param q String"]),
            [Code::InvalidDirective]
        );
    }

    #[test]
    fn quoted_values_keep_their_spaces() {
        let op = operation(
            &[
                "@route GET /x",
                "@query-param q: String example=\"two words\" \"A  spaced description\"",
            ],
            "/x",
            "get",
        );
        let q = param(&op, "q");
        assert_eq!(q["example"], "two words");
        assert_eq!(q["description"], "A  spaced description");
    }
}

mod dsl_v2 {
    use super::*;
    use oas_forge::diagnostics::Code;

    #[test]
    fn the_general_form_names_the_location() {
        let op = operation(
            &[
                "@route GET /x/{id}",
                "@param path id: u8",
                "@param query q: String",
                "@param header X-Trace: Uuid",
                "@param cookie s: String",
            ],
            "/x/{id}",
            "get",
        );
        let locations: Vec<_> = params(&op).iter().map(|p| p["in"].clone()).collect();
        assert_eq!(locations, ["path", "query", "header", "cookie"]);
        assert_eq!(
            findings(&["@route GET /x", "@param body b: String"]),
            [Code::InvalidDirective]
        );
        assert_eq!(
            findings(&["@route GET /x", "@param query"]),
            [Code::InvalidDirective]
        );
    }

    #[test]
    fn flags_set_schema_and_parameter_fields() {
        let op = operation(
            &[
                "@route GET /x",
                "@param query sort: String enum=[name, price] default=name \"Sort order\"",
                "@param query limit: u32 min=1 max=100 default=20",
                "@param query q: String min=2 max=5 pattern=\"^[a-z]+$\" format=word",
                "@param query tags: Vec<String> style=form explode=false allow-reserved allow-empty",
                "@param query exploded: Vec<String> explode",
                "@param query d: String default=x required example=\"two words\"",
            ],
            "/x",
            "get",
        );
        assert_eq!(
            param(&op, "sort"),
            json!({
                "name": "sort", "in": "query", "required": false, "description": "Sort order",
                "schema": { "type": "string", "enum": ["name", "price"], "default": "name" },
            })
        );
        assert_eq!(
            param(&op, "limit")["schema"],
            json!({ "type": "integer", "format": "int64", "minimum": 1, "maximum": 100, "default": 20 })
        );
        assert_eq!(
            param(&op, "q")["schema"],
            json!({ "type": "string", "minLength": 2, "maxLength": 5, "pattern": "^[a-z]+$", "format": "word" })
        );
        let tags = param(&op, "tags");
        assert_eq!(
            (
                &tags["style"],
                &tags["explode"],
                &tags["allowReserved"],
                &tags["allowEmptyValue"]
            ),
            (&json!("form"), &json!(false), &json!(true), &json!(true))
        );
        assert_eq!(param(&op, "exploded")["explode"], true);
        let d = param(&op, "d");
        assert_eq!(
            (&d["required"], &d["example"]),
            (&json!(true), &json!("two words"))
        );
    }

    #[test]
    fn unknown_flags_are_reported_but_the_parameter_is_kept() {
        let src = [
            "@route GET /x",
            "@param query q: String bogus enum=notalist min=abc",
        ];
        let op = operation(&src, "/x", "get");
        assert_eq!(param(&op, "q")["schema"], json!({ "type": "string" }));
        assert_eq!(findings(&src), [Code::InvalidDirective; 3]);
    }

    #[test]
    fn generic_types_with_spaces_stay_one_token() {
        let op = operation(
            &[
                "@route GET /x",
                "@param query m: HashMap<String, u8> \"Map\"",
            ],
            "/x",
            "get",
        );
        assert_eq!(param(&op, "m")["schema"]["type"], "object");
        assert_eq!(param(&op, "m")["description"], "Map");
    }

    #[test]
    fn querystring_parameters_use_content() {
        let op = operation(
            &[
                "@route QUERY /search",
                "@querystring-param filter: $Filter \"Filter\"",
            ],
            "/search",
            "query",
        );
        assert_eq!(
            param(&op, "filter"),
            json!({
                "name": "filter", "in": "querystring", "required": true, "description": "Filter",
                "content": { "application/x-www-form-urlencoded": { "schema": { "$ref": "$Filter" } } },
            })
        );
    }

    #[test]
    fn repeated_parameters_replace_earlier_ones() {
        let op = operation(
            &[
                "@route GET /x",
                "@param query a: u8",
                "@param query a: String",
            ],
            "/x",
            "get",
        );
        assert_eq!(params(&op).len(), 1);
        assert_eq!(param(&op, "a")["schema"], json!({ "type": "string" }));
    }

    #[test]
    fn struct_expansion_leaves_a_marker() {
        let op = operation(
            &[
                "@route GET /x/{id}",
                "@path-params $Ids",
                "@query-params Filter",
                "@header-params $H",
                "@cookie-params $C",
            ],
            "/x/{id}",
            "get",
        );
        let markers: Vec<_> = params(&op)
            .iter()
            .map(|p| p["x-oas-forge-expand"].clone())
            .collect();
        // The marker remembers the annotation for findings of the expansion.
        let at = |location: &str, schema: &str| json!({ "in": location, "schema": schema, "file": "test.rs", "line": 1 });
        assert_eq!(
            markers,
            [
                at("path", "$Ids"),
                at("query", "$Filter"),
                at("header", "$H"),
                at("cookie", "$C"),
            ]
        );
        assert_eq!(
            findings(&["@route GET /x", "@query-params"]),
            [Code::InvalidDirective]
        );
    }

    #[test]
    fn qualified_struct_names_use_the_last_segment() {
        let op = operation(
            &["@route GET /x", "@query-params crate::models::Filter"],
            "/x",
            "get",
        );
        assert_eq!(params(&op)[0]["x-oas-forge-expand"]["schema"], "$Filter");
    }

    #[test]
    fn path_expansion_defers_the_placeholder_check() {
        let (document, diagnostics) = try_route(&["@route GET /x/{id}", "@path-params $Ids"]);
        assert!(document.is_some());
        assert!(codes(&diagnostics).is_empty());
    }
}

#[test]
fn flag_values_that_are_not_yaml_stay_text() {
    let op = operation(
        // An unclosed bracket takes the rest of the line.
        &[
            "@route GET /x",
            "@query-param q: String default= example=[open \"x\"",
        ],
        "/x",
        "get",
    );
    let q = param(&op, "q");
    assert_eq!(q["example"], "[open \"x\"");
    assert_eq!(q["schema"]["default"], "");
    assert_eq!(q["required"], false);
}

#[test]
fn brackets_in_a_pattern_do_not_group() {
    let op = operation(
        &[
            "@route GET /x",
            "@query-param q: String pattern=[^(]+ \"Search text\"",
        ],
        "/x",
        "get",
    );
    let q = param(&op, "q");
    assert_eq!(q["schema"]["pattern"], "[^(]+");
    assert_eq!(q["description"], "Search text");
}

#[test]
fn brackets_inside_quotes_do_not_nest() {
    let op = operation(
        &[
            "@route GET /x",
            "@query-param q: u32 \"Use [brackets\" default=1",
        ],
        "/x",
        "get",
    );
    let q = param(&op, "q");
    assert_eq!(q["description"], "Use [brackets");
    assert_eq!(q["schema"]["default"], 1);
}

#[test]
fn closing_brackets_inside_quotes_do_not_close() {
    let op = operation(
        &[
            "@route GET /x",
            "@query-param q: String enum=[\"a)\", b] \"Description\"",
        ],
        "/x",
        "get",
    );
    let q = param(&op, "q");
    assert_eq!(q["schema"]["enum"], serde_json::json!(["a)", "b"]));
    assert_eq!(q["description"], "Description");
}

/// `optional`: the counterpart of `required`.
mod optional_flag {
    use super::*;
    use oas_forge::diagnostics::Diagnostics;

    fn required(directive: &str) -> (serde_json::Value, Diagnostics) {
        let (document, diagnostics) = try_route(&["@route GET /x/{id: u64}", directive]);
        let op = &document.unwrap()["paths"]["/x/{id}"]["get"];
        let last = op["parameters"].as_array().unwrap().last().unwrap().clone();
        (last["required"].clone(), diagnostics)
    }

    #[test]
    fn a_parameter_is_required_unless_option_default_or_optional() {
        for (directive, expected) in [
            ("@query-param s: u32 \"Size\"", true),
            ("@query-param s: Option<u32>", false),
            ("@query-param s: u32 default=10", false),
            ("@query-param s: u32 optional \"Size\"", false),
            ("@query-param s: optional", false),
            ("@header-param X-Trace: String optional", false),
            ("@cookie-param session: optional \"Session\"", false),
            ("@param query s: u32 \"Size\" optional", false),
            ("@query-param s: Option<u32> required", true),
        ] {
            let (required, diagnostics) = required(directive);
            assert_eq!(required, expected, "{directive}");
            assert!(diagnostics.is_empty(), "{directive}: {diagnostics:#?}");
        }
    }

    #[test]
    fn optional_without_type_keeps_the_string_type() {
        let op = operation(&["@route GET /x", "@query-param s: optional"], "/x", "get");
        assert_eq!(param(&op, "s")["schema"], json!({ "type": "string" }));
    }

    #[test]
    fn contradictions_are_reported() {
        // Path parameters are always required.
        let (document, diagnostics) =
            try_route(&["@route GET /x/{id}", "@path-param id: u64 optional"]);
        assert_eq!(codes(&diagnostics), [Code::InvalidDirective]);
        assert!(
            diagnostics
                .iter()
                .next()
                .unwrap()
                .message()
                .contains("always required"),
            "{diagnostics:#?}"
        );
        assert_eq!(
            document.unwrap()["paths"]["/x/{id}"]["get"]["parameters"][0]["required"],
            true
        );
        // `required` wins over `optional`.
        let (required, diagnostics) = required("@query-param s: u32 optional required");
        assert_eq!(required, true);
        assert_eq!(codes(&diagnostics), [Code::InvalidDirective]);
    }
}
