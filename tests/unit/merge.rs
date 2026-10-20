use oas_forge::merge::{Policy, merge};
use serde_json::{Value, json};

fn merged(target: Value, source: Value, policy: Policy) -> (Value, Vec<String>) {
    let mut target = target;
    let conflicts = merge(&mut target, source, policy);
    (target, conflicts)
}

mod document {
    use super::*;

    #[test]
    fn maps_merge_recursively() {
        let (value, conflicts) = merged(
            json!({ "User": { "type": "object", "properties": { "id": { "type": "integer" } } } }),
            json!({ "User": { "properties": { "name": { "type": "string" } } } }),
            Policy::Document,
        );
        assert_eq!(
            value,
            json!({ "User": { "type": "object", "properties": {
                "id": { "type": "integer" }, "name": { "type": "string" }
            }}})
        );
        assert!(conflicts.is_empty());
    }

    #[test]
    fn arrays_are_appended_and_deduplicated() {
        let (value, _) = merged(
            json!({ "tags": ["A", "B"] }),
            json!({ "tags": ["B", "C", "A"] }),
            Policy::Document,
        );
        assert_eq!(value, json!({ "tags": ["A", "B", "C"] }));
    }

    #[test]
    fn differing_scalars_are_won_by_the_source_and_reported() {
        let (value, conflicts) = merged(
            json!({ "info": { "title": "A", "version": "1" } }),
            json!({ "info": { "title": "B", "version": "1" } }),
            Policy::Document,
        );
        assert_eq!(value["info"]["title"], "B");
        assert_eq!(conflicts, ["/info/title"]);
    }

    #[test]
    fn type_mismatches_are_conflicts() {
        let (value, conflicts) = merged(
            json!({ "servers": ["a"] }),
            json!({ "servers": { "url": "b" } }),
            Policy::Document,
        );
        assert_eq!(value, json!({ "servers": { "url": "b" } }));
        assert_eq!(conflicts, ["/servers"]);
    }

    #[test]
    fn positional_arrays_are_not_deduplicated() {
        let (value, conflicts) = merged(
            json!({ "prefixItems": [{ "type": "string" }] }),
            json!({ "prefixItems": [{ "type": "string" }, { "type": "string" }] }),
            Policy::Document,
        );
        assert_eq!(value["prefixItems"].as_array().unwrap().len(), 2);
        assert_eq!(conflicts, ["/prefixItems"]);

        let (_, conflicts) = merged(
            json!({ "allOf": [1] }),
            json!({ "allOf": [1] }),
            Policy::Document,
        );
        assert!(conflicts.is_empty());
    }

    #[test]
    fn pointers_escape_slashes_and_tildes() {
        let (_, conflicts) = merged(
            json!({ "paths": { "/a~b": { "x": 1 } } }),
            json!({ "paths": { "/a~b": { "x": 2 } } }),
            Policy::Document,
        );
        assert_eq!(conflicts, ["/paths/~1a~0b/x"]);
    }
}

mod extend {
    use super::*;

    #[test]
    fn local_scalars_win_and_gaps_are_filled() {
        let (value, conflicts) = merged(
            json!({ "summary": "Local", "nested": { "a": 1 } }),
            json!({ "summary": "Fragment", "deprecated": true, "nested": { "a": 2, "b": 3 } }),
            Policy::Extend,
        );
        assert_eq!(
            value,
            json!({ "summary": "Local", "nested": { "a": 1, "b": 3 }, "deprecated": true })
        );
        assert!(conflicts.is_empty());
    }

    #[test]
    fn arrays_are_combined_and_positional_arrays_kept() {
        let (value, _) = merged(
            json!({ "security": [{ "a": [] }], "allOf": [1] }),
            json!({ "security": [{ "b": [] }], "allOf": [2] }),
            Policy::Extend,
        );
        assert_eq!(
            value,
            json!({ "security": [{ "a": [] }, { "b": [] }], "allOf": [1] })
        );
    }
}

mod operation {
    use super::*;

    #[test]
    fn parameters_are_matched_by_name_and_location() {
        let (value, _) = merged(
            json!({ "parameters": [
                { "name": "q", "in": "query", "required": true },
                { "name": "q", "in": "header" },
            ]}),
            json!({ "parameters": [
                { "name": "q", "in": "query", "description": "from YAML" },
                { "name": "extra", "in": "query" },
                { "$ref": "#/components/parameters/Paging" },
                { "$ref": "#/components/parameters/Paging" },
            ]}),
            Policy::Operation,
        );
        assert_eq!(
            value["parameters"],
            json!([
                { "name": "q", "in": "query", "required": true, "description": "from YAML" },
                { "name": "q", "in": "header" },
                { "name": "extra", "in": "query" },
                { "$ref": "#/components/parameters/Paging" },
            ])
        );
    }

    #[test]
    fn conflicts_inside_parameters_name_the_index() {
        let (value, conflicts) = merged(
            json!({ "parameters": [{ "name": "a", "in": "query" }, { "name": "b", "in": "query", "required": false }] }),
            json!({ "parameters": [{ "name": "b", "in": "query", "required": true }] }),
            Policy::Operation,
        );
        assert_eq!(value["parameters"][1]["required"], true);
        assert_eq!(conflicts, ["/parameters/1/required"]);
    }

    #[test]
    fn yaml_scalars_win_and_are_reported() {
        let (value, conflicts) = merged(
            json!({ "summary": "DSL", "responses": { "200": { "description": "OK" } } }),
            json!({ "summary": "YAML", "responses": { "404": { "description": "Missing" } } }),
            Policy::Operation,
        );
        assert_eq!(value["summary"], "YAML");
        assert_eq!(value["responses"].as_object().unwrap().len(), 2);
        assert_eq!(conflicts, ["/summary"]);
    }
}

mod override_policy {
    use super::*;

    #[test]
    fn maps_merge_and_everything_else_is_replaced() {
        let (value, conflicts) = merged(
            json!({ "type": "object", "required": ["id", "name"], "properties": { "id": { "type": "integer" } } }),
            json!({ "required": ["id"], "properties": { "id": { "minimum": 1 } }, "type": "string" }),
            Policy::Override,
        );
        assert_eq!(
            value,
            json!({ "type": "string", "required": ["id"], "properties": { "id": { "type": "integer", "minimum": 1 } } })
        );
        assert!(conflicts.is_empty());
    }

    #[test]
    fn a_non_map_override_replaces_the_value() {
        let (value, _) = merged(json!({ "a": 1 }), json!("text"), Policy::Override);
        assert_eq!(value, json!("text"));
    }
}

#[test]
fn equal_values_never_conflict() {
    for policy in [
        Policy::Document,
        Policy::Operation,
        Policy::Extend,
        Policy::Override,
    ] {
        let (value, conflicts) = merged(
            json!({ "a": 1, "b": [1] }),
            json!({ "a": 1, "b": [1] }),
            policy,
        );
        assert_eq!(value, json!({ "a": 1, "b": [1] }), "{policy:?}");
        assert!(conflicts.is_empty(), "{policy:?}");
    }
}
