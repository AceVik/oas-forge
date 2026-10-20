//! File-level (`//!`) documentation blocks handled by `visit_file`.

use crate::common::{extract, schema, schema_names, yaml};
use oas_forge::dsl::parse_route_dsl;
use oas_forge::visitor::ExtractedItem;
use serde_json::json;

fn kinds(items: &[ExtractedItem]) -> Vec<String> {
    items
        .iter()
        .map(|item| match item {
            ExtractedItem::Schema { name: Some(n), .. } => format!("schema:{n}"),
            ExtractedItem::Schema { name: None, .. } => "document".to_string(),
            ExtractedItem::Fragment { name, .. } => format!("fragment:{name}"),
            ExtractedItem::Blueprint { name, .. } => format!("blueprint:{name}"),
            ExtractedItem::RouteDSL { operation_id, .. } => format!("route:{operation_id}"),
        })
        .collect()
}

#[test]
fn plain_file_docs_are_ignored() {
    assert!(extract("//! Just a crate description.\n\nfn main() {}").is_empty());
}

#[test]
fn virtual_type_block_defines_a_named_schema() {
    let items = extract(
        r#"
        //! @openapi-type Email
        //! type: string
        //! format: email
        //! description: Valid email address

        fn main() {}
        "#,
    );
    assert_eq!(
        schema(&items, "Email"),
        json!({ "type": "string", "format": "email", "description": "Valid email address" })
    );
}

#[test]
fn openapi_block_is_extracted_as_raw_document() {
    let items = extract("//! @openapi\n//! openapi: 3.1.0\n//! info:\n//!   title: T");
    match items.as_slice() {
        [
            ExtractedItem::Schema {
                name: None,
                content,
                ..
            },
        ] => {
            assert_eq!(
                yaml(content),
                json!({ "openapi": "3.1.0", "info": { "title": "T" } })
            );
        }
        other => panic!("expected root document, got {other:#?}"),
    }
}

#[test]
fn a_new_header_closes_the_previous_block() {
    let items = extract(
        r#"
        //! @openapi-type Email
        //! type: string
        //! @openapi-type Age
        //! type: integer
        //! @openapi
        //! paths: {}
        //! @openapi-type Id
        //! type: string
        //! @route GET /ids
        "#,
    );
    assert_eq!(
        kinds(&items),
        [
            "schema:Email",
            "schema:Age",
            "document",
            "schema:Id",
            "route:virtual_route_10"
        ]
    );
    assert_eq!(schema(&items, "Age"), json!({ "type": "integer" }));
}

#[test]
fn virtual_route_collects_following_lines() {
    let items = extract(
        r#"
        //! Module level docs
        //!
        //! @route GET /virtual/users
        //! @return 200: "List of users"

        pub mod inner {}
        "#,
    );
    match items.as_slice() {
        [
            ExtractedItem::RouteDSL {
                content,
                line,
                operation_id,
            },
        ] => {
            assert_eq!(*line, 4);
            assert_eq!(operation_id, "virtual_route_4");
            assert_eq!(
                content,
                " @route GET /virtual/users\n @return 200: \"List of users\""
            );

            let doc_lines: Vec<String> = content.lines().map(str::to_string).collect();
            let doc = yaml(&parse_route_dsl(&doc_lines, operation_id).unwrap());
            let op = &doc["paths"]["/virtual/users"]["get"];
            assert_eq!(op["operationId"], "virtual_route_4");
            assert_eq!(op["responses"]["200"]["description"], "List of users");
        }
        other => panic!("expected one virtual route, got {other:#?}"),
    }
}

#[test]
fn non_doc_inner_attributes_close_open_blocks() {
    let items = extract(
        r#"
        #![allow(clippy::all)]
        //! @openapi-type Id
        //! type: string
        #![allow(dead_code)]
        //! @route GET /x
        #![allow(unused)]
        //! @openapi
        //! paths: {}
        #![allow(missing_docs)]
        "#,
    );
    assert_eq!(
        kinds(&items),
        ["schema:Id", "route:virtual_route_6", "document"]
    );
}

#[test]
fn non_literal_doc_attributes_neither_contribute_nor_close_blocks() {
    let items = extract(
        r#"
        //! @openapi-type Id
        #![doc = include_str!("intro.md")]
        #![doc(html_root_url = "https://docs.rs/x")]
        #![doc = 5]
        //! type: string
        "#,
    );
    assert_eq!(schema(&items, "Id"), json!({ "type": "string" }));
}

#[test]
fn items_after_file_docs_are_visited() {
    let items = extract("//! @openapi-type Id\n//! type: string\n\n/// @openapi\nstruct S;");
    assert_eq!(schema_names(&items), ["Id", "S"]);
}
