use crate::common::yaml;
use oas_forge::index::Registry;
use oas_forge::preprocessor::preprocess;
use serde_json::json;

fn registry(fragments: &[(&str, &[&str], &str)]) -> Registry {
    let mut registry = Registry::new();
    for (name, params, body) in fragments {
        registry.insert_fragment(
            name.to_string(),
            params.iter().map(|p| p.to_string()).collect(),
            body.to_string(),
        );
    }
    registry
}

#[test]
fn valid_yaml_without_directives_is_normalized() {
    assert_eq!(preprocess("a:   1", &Registry::new()), "a: 1\n");
}

#[test]
fn non_yaml_text_is_returned_unchanged() {
    let text = "@route GET /x\n@tag Users";
    assert_eq!(preprocess(text, &Registry::new()), text);
}

mod insert {
    use super::*;

    #[test]
    fn replaces_the_line_with_the_fragment() {
        let reg = registry(&[("Headers", &[], "header: x-val\nother: y-val")]);
        assert_eq!(
            preprocess("  @insert Headers", &reg),
            "header: x-val\nother: y-val\n"
        );
    }

    #[test]
    fn substitutes_named_parameters() {
        let reg = registry(&[("Field", &["name", "kind"], "{{name}}: {{kind}}")]);
        assert_eq!(
            preprocess("@insert Field(\"my-name\", text)", &reg),
            "my-name: text\n"
        );
    }

    #[test]
    fn missing_arguments_leave_placeholders() {
        let reg = registry(&[("Error", &["code"], "description: Error {{code}}")]);
        assert_eq!(
            yaml(&preprocess("@insert Error", &reg)),
            json!({ "description": "Error {{code}}" })
        );
        assert_eq!(
            yaml(&preprocess("@insert Error()", &reg)),
            json!({ "description": "Error {{code}}" })
        );
    }

    #[test]
    fn keeps_indentation_for_nested_inserts() {
        let reg = registry(&[("NotFound", &[], "'404':\n  description: Not Found")]);
        let out = preprocess(
            "responses:\n  '200': { description: OK }\n  @insert NotFound",
            &reg,
        );
        assert_eq!(
            yaml(&out),
            json!({ "responses": {
                "200": { "description": "OK" },
                "404": { "description": "Not Found" }
            }})
        );
    }

    #[test]
    fn preserves_doc_comment_markers() {
        let reg = registry(&[("CommonParams", &[], "- name: id\n  in: query")]);

        let out = preprocess("    /// List items\n    /// @insert CommonParams", &reg);
        assert_eq!(
            out,
            "    /// List items\n    /// - name: id\n    ///   in: query"
        );

        let out = preprocess("//! @insert CommonParams", &reg);
        assert_eq!(out.trim_end(), "//! - name: id\n//!   in: query");
    }

    #[test]
    fn empty_fragment_removes_the_line() {
        let reg = registry(&[("Nothing", &[], "   ")]);
        assert_eq!(preprocess("a: 1\n@insert Nothing", &reg), "a: 1\n");
    }

    #[test]
    fn unknown_fragment_keeps_the_line() {
        let text = "@insert Missing(\"\")";
        assert_eq!(preprocess(text, &Registry::new()), text);
    }
}

mod extend {
    use super::*;

    #[test]
    fn merges_fragment_into_the_surrounding_mapping() {
        let reg = registry(&[(
            "Base",
            &[],
            "responses:\n  '404':\n    description: Not Found",
        )]);
        let out = preprocess(
            "get:\n  @extend Base\n  responses:\n    '200':\n      description: OK",
            &reg,
        );
        assert_eq!(
            yaml(&out),
            json!({ "get": { "responses": {
                "200": { "description": "OK" },
                "404": { "description": "Not Found" }
            }}})
        );
    }

    #[test]
    fn substitutes_arguments() {
        // Quoted arguments only work after the scanner's macro pass has
        // auto-quoted the line (see `scanner::preprocess_macros`).
        let reg = registry(&[("Secured", &["role"], "security:\n  - oidc: [{{role}}]")]);
        let out = preprocess("get:\n  @extend Secured(admin)\n  summary: S", &reg);
        assert_eq!(
            yaml(&out),
            json!({ "get": { "summary": "S", "security": [{ "oidc": ["admin"] }] } })
        );
    }

    #[test]
    fn empty_argument_list_is_allowed() {
        let reg = registry(&[("Base", &[], "deprecated: true")]);
        let out = preprocess("get:\n  x-openapi-extend: Base()", &reg);
        assert_eq!(yaml(&out), json!({ "get": { "deprecated": true } }));
    }

    #[test]
    fn conflicting_scalars_take_the_fragment_value() {
        let reg = registry(&[("Base", &[], "summary: From fragment")]);
        let out = preprocess("get:\n  @extend Base\n  summary: Local", &reg);
        assert_eq!(yaml(&out)["get"]["summary"], "From fragment");
    }

    #[test]
    fn non_mapping_fragment_replaces_the_mapping() {
        let reg = registry(&[("List", &[], "- a\n- b")]);
        let out = preprocess("get:\n  @extend List\n  summary: S", &reg);
        assert_eq!(yaml(&out), json!({ "get": ["a", "b"] }));
    }

    #[test]
    fn works_inside_sequences() {
        let reg = registry(&[("Typed", &[], "schema: { type: string }")]);
        let out = preprocess("- name: a\n  @extend Typed\n- name: b", &reg);
        assert_eq!(
            yaml(&out),
            json!([{ "name": "a", "schema": { "type": "string" } }, { "name": "b" }])
        );
    }

    #[test]
    fn unknown_fragment_drops_the_marker() {
        let out = preprocess("get:\n  @extend Missing\n  summary: S", &Registry::new());
        assert_eq!(yaml(&out), json!({ "get": { "summary": "S" } }));
    }

    #[test]
    fn invalid_fragment_yaml_drops_the_marker() {
        let reg = registry(&[("Broken", &[], "a: [unclosed")]);
        let out = preprocess("get:\n  @extend Broken\n  summary: S", &reg);
        assert_eq!(yaml(&out), json!({ "get": { "summary": "S" } }));
    }

    #[test]
    fn non_string_marker_is_removed_without_merging() {
        let reg = registry(&[("Base", &[], "deprecated: true")]);
        let out = preprocess("get:\n  x-openapi-extend: 42\n  summary: S", &reg);
        assert_eq!(yaml(&out), json!({ "get": { "summary": "S" } }));
    }

    #[test]
    fn marker_survives_when_snippet_is_not_valid_yaml() {
        let out = preprocess("@route GET /x\n  @extend Base(a)", &Registry::new());
        assert_eq!(out, "@route GET /x\n  x-openapi-extend: \"Base(a)\"");
    }
}
