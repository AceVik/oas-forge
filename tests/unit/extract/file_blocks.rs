//! File-level (`//!`) annotation blocks.

use super::{document_items, kinds, routes};
use crate::common::{codes, extract, extract_with_diagnostics, schema, schema_names, yaml};
use oas_forge::diagnostics::Code;
use serde_json::json;

#[test]
fn plain_file_docs_are_ignored() {
    assert!(extract("//! Just a crate description.\n\nfn main() {}").is_empty());
}

#[test]
fn named_schema_block() {
    let items = extract(
        r#"
        //! @openapi-type Email
        //! type: string
        //! format: email
        //! description: Valid email address

        fn main() {}
        "#,
    );
    assert_eq!(kinds(&items), ["named:Email"]);
    assert_eq!(
        schema(&items, "Email"),
        json!({ "type": "string", "format": "email", "description": "Valid email address" })
    );
    assert_eq!(items[0].location().line(), 3);
}

#[test]
fn openapi_block_is_a_document() {
    let items = extract("//! @openapi\n//! openapi: 3.1.0\n//! info:\n//!   title: T");
    let documents = document_items(&items);
    assert_eq!(documents.len(), 1);
    assert_eq!(
        yaml(&documents[0].text),
        json!({ "openapi": "3.1.0", "info": { "title": "T" } })
    );
    assert!(documents[0].tags.is_empty());
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
        //! @route GET /more
        "#,
    );
    assert_eq!(
        kinds(&items),
        [
            "named:Email",
            "named:Age",
            "document",
            "named:Id",
            "route:-",
            "route:-"
        ]
    );
    assert_eq!(schema(&items, "Age"), json!({ "type": "integer" }));
}

#[test]
fn virtual_routes_collect_following_lines() {
    let items = extract(
        r#"
        //! Module level docs
        //!
        //! @route GET /virtual/users
        //! @return 200: "List of users"

        pub mod inner {}
        "#,
    );
    let routes = routes(&items);
    assert_eq!(routes.len(), 1);
    let route = routes[0];
    assert_eq!(
        route.lines,
        [
            " @route GET /virtual/users",
            " @return 200: \"List of users\""
        ]
    );
    assert_eq!(route.line_numbers, [4, 5]);
    assert_eq!(
        route.operation_id, None,
        "derived from method and path later"
    );
    assert_eq!(route.location.line(), 4);
}

#[test]
fn separator_lines_end_a_block() {
    // Regression: the README example put a `---` line between fragments and
    // the text after it ended up in the fragment body.
    let (items, diagnostics) = extract_with_diagnostics(
        r#"
        //! @openapi-type Id
        //! type: string
        //! ---------------
        //! More crate documentation that is not YAML.
        //! =====
        //! @openapi-type Other
        //! type: integer
        "#,
    );
    assert_eq!(kinds(&items), ["named:Id", "named:Other"]);
    assert_eq!(schema(&items, "Id"), json!({ "type": "string" }));
    assert_eq!(codes(&diagnostics), [Code::BlockSeparator]);
    assert_eq!(
        diagnostics
            .iter()
            .next()
            .unwrap()
            .location()
            .unwrap()
            .line(),
        4
    );
}

#[test]
fn short_or_mixed_lines_are_not_separators() {
    let items = extract("//! @openapi\n//! a:\n//!   - x\n//! b: --\n//! c: '-=-'");
    assert_eq!(
        yaml(&document_items(&items)[0].text),
        json!({ "a": ["x"], "b": "--", "c": "-=-" })
    );
}

#[test]
fn a_gap_in_the_doc_comment_ends_the_block() {
    let items = extract(
        "//! @openapi-type Id\n//! type: string\n\n//! Unrelated crate documentation.\n//! It is not part of Id.",
    );
    assert_eq!(schema(&items, "Id"), json!({ "type": "string" }));
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
    assert_eq!(kinds(&items), ["named:Id", "route:-", "document"]);
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
fn route_headers_need_a_word_boundary() {
    let items = extract("//! @routes are documented below\n//! @route\n//! @return 204: \"x\"");
    assert_eq!(kinds(&items), ["route:-"]);
    assert_eq!(routes(&items)[0].lines.len(), 2);
}

#[test]
fn empty_named_schema_blocks_are_dropped() {
    assert!(extract("//! @openapi-type Nothing\n//!\n#![allow(unused)]").is_empty());
}

#[test]
fn items_after_file_docs_are_visited() {
    let items = extract("//! @openapi-type Id\n//! type: string\n\n/// @openapi\nstruct S;");
    assert_eq!(schema_names(&items), ["Id", "S"]);
}

#[test]
fn file_level_fragments_and_generic_headers() {
    let (items, diagnostics) = extract_with_diagnostics(
        "//! @openapi-fragment Err(code)\n//! description: {{code}}\n//! @openapi<T>\n//! type: object",
    );
    assert_eq!(kinds(&items), ["fragment:Err"]);
    assert_eq!(codes(&diagnostics), [Code::UnnamedBlueprint]);
}

#[test]
fn asyncapi_and_channel_blocks() {
    let items = extract(
        "//! @asyncapi\n//! asyncapi: 3.1.0\n//! @channel a/b\n//! @send M: u8\n//! @openapi\n//! paths: {}",
    );
    assert_eq!(kinds(&items), ["asyncapi", "channel:-", "document"]);
}

/// YAML in fenced code blocks right after a header (rustdoc renders it as
/// code): the fences are removed and the block ends at the closing fence.
mod fenced_yaml {
    use super::*;
    use crate::extract::fragment;

    #[test]
    fn a_fenced_block_is_the_yaml_of_its_header() {
        for (open, close) in [
            ("```yaml", "```"),
            ("```yml", "```"),
            ("```", "```"),
            ("~~~", "~~~"),
            ("````json", "````"),
        ] {
            let items = extract(&format!(
                "//! @openapi\n//! {open}\n//! openapi: 3.1.0\n//! info:\n//!   title: T\n//! ### a YAML comment, no separator\n//! {close}\n//! Prose after the block.\n\nfn main() {{}}"
            ));
            assert_eq!(
                document_items(&items)[0].text,
                "openapi: 3.1.0\ninfo:\n  title: T\n### a YAML comment, no separator",
                "{open}"
            );
            assert_eq!(document_items(&items)[0].location.line(), 3);
        }
    }

    #[test]
    fn fragments_named_schemas_and_asyncapi_blocks_take_fences() {
        let items = extract(
            "//! @openapi-fragment Paging(max)\n//! ```yaml\n//! - name: limit\n//!   schema: { maximum: {{max}} }\n//! ```\n//!\n//! @openapi-type Money\n//! ```yaml\n//! type: string\n//! ```\n//!\n//! @asyncapi\n//! ```yaml\n//! asyncapi: 3.0.0\n//! ```\n\nfn main() {}",
        );
        assert_eq!(
            fragment(&items, "Paging").body,
            "- name: limit\n  schema: { maximum: {{max}} }"
        );
        assert_eq!(schema(&items, "Money"), json!({ "type": "string" }));
        assert!(
            kinds(&items).contains(&"asyncapi".to_string()),
            "{items:#?}"
        );
    }

    #[test]
    fn a_code_block_in_another_language_is_no_yaml_fence() {
        let items = extract("//! @openapi\n//! ```rust\n//! x-a: 1\n//! ```\n\nfn main() {}");
        // As before: the fences are part of the (invalid) YAML text.
        assert!(document_items(&items)[0].text.starts_with("```rust"));
    }
}
