//! Free-form `@openapi` blocks on functions (`parse_doc_block`).

use crate::common::{extract, schema};
use oas_forge::visitor::ExtractedItem;
use serde_json::json;

fn fragment(items: &[ExtractedItem], wanted: &str) -> (Vec<String>, String) {
    items
        .iter()
        .find_map(|item| match item {
            ExtractedItem::Fragment {
                name,
                params,
                content,
                ..
            } if name == wanted => Some((params.clone(), content.clone())),
            _ => None,
        })
        .unwrap_or_else(|| panic!("no fragment {wanted} in {items:#?}"))
}

#[test]
fn fragment_with_parameters() {
    let items = extract(
        r#"
        /// @openapi-fragment Paged(limit, offset)
        /// - name: {{limit}}
        /// - name: {{offset}}
        fn _fragments() {}
        "#,
    );
    let (params, content) = fragment(&items, "Paged");
    assert_eq!(params, ["limit", "offset"]);
    assert_eq!(content, "- name: {{limit}}\n- name: {{offset}}");
}

#[test]
fn fragment_without_or_with_empty_parameter_list() {
    let items = extract(
        r#"
        /// @openapi-fragment Plain
        /// a: 1
        /// @openapi-fragment Empty()
        /// b: 2
        fn _fragments() {}
        "#,
    );
    assert_eq!(fragment(&items, "Plain"), (vec![], "a: 1".to_string()));
    assert_eq!(fragment(&items, "Empty"), (vec![], "b: 2".to_string()));
}

#[test]
fn virtual_type_inside_function_docs() {
    let items = extract("/// @openapi-type Money\n/// type: string\nfn _types() {}");
    assert_eq!(schema(&items, "Money"), json!({ "type": "string" }));
}

#[test]
fn generic_header_without_a_named_item_is_ignored() {
    assert!(extract("/// @openapi<T>\n/// type: object\nfn _generic() {}").is_empty());
    assert!(extract("/// @openapi<T\n/// type: object\nfn _unterminated() {}").is_empty());
}

#[test]
fn documentation_before_the_first_header_is_not_emitted() {
    // Regression: the summary line became its own snippet ("Health check"),
    // which then replaced the whole merged specification.
    let items = extract(
        r#"
        /// Health check
        /// Returns 200 while the service is up.
        /// @openapi
        /// paths:
        ///   /health: {}
        fn health() {}
        "#,
    );
    match items.as_slice() {
        [
            ExtractedItem::Schema {
                name: None,
                content,
                ..
            },
        ] => {
            assert_eq!(content, "paths:\n  /health: {}");
        }
        other => panic!("expected only the OpenAPI document, got {other:#?}"),
    }
}

#[test]
fn documentation_before_a_json_document_is_not_emitted() {
    let items =
        extract("/// Intro\n/// {\"paths\": {}}\n/// @openapi-fragment F\n/// a: b\nfn f() {}");
    assert!(matches!(
        &items[0],
        ExtractedItem::Schema { name: None, content, .. } if content == r#"{"paths": {}}"#
    ));
    assert_eq!(items.len(), 2);
}

#[test]
fn leading_json_document_is_kept_verbatim() {
    let items = extract(
        r#"
        /// {"paths": {"/json": {}}}
        /// @openapi-fragment After
        /// a: b
        fn _json() {}
        "#,
    );
    assert!(matches!(
        &items[0],
        ExtractedItem::Schema { name: None, content, .. } if content == r#"{"paths": {"/json": {}}}"#
    ));
    assert_eq!(fragment(&items, "After").1, "a: b");
}

#[test]
fn comments_and_blank_lines_are_preserved_in_documents() {
    let items = extract(
        r#"
        /// @openapi
        /// # Health endpoints
        ///
        /// paths:
        ///   /health: {}
        fn health() {}
        "#,
    );
    match items.as_slice() {
        [ExtractedItem::Schema { content, .. }] => {
            assert_eq!(content, "# Health endpoints\n\npaths:\n  /health: {}");
        }
        other => panic!("expected one document, got {other:#?}"),
    }
}
