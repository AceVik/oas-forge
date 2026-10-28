//! `@openapi…` sections in the doc comments of functions, methods and modules.

use super::{document_items, fragment, kinds};
use crate::common::{codes, extract, extract_with_diagnostics, schema};
use oas_forge::diagnostics::Code;
use serde_json::json;

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
    let paged = fragment(&items, "Paged");
    assert_eq!(paged.params, ["limit", "offset"]);
    assert_eq!(paged.body, "- name: {{limit}}\n- name: {{offset}}");
    assert_eq!(
        paged.location.line(),
        2,
        "fragments are located at their header"
    );
}

#[test]
fn fragment_without_or_with_empty_parameter_list() {
    let items = extract(
        r#"
        /// @openapi-fragment Plain
        /// a: 1
        /// @openapi-fragment Empty( , )
        /// b: 2
        fn _fragments() {}
        "#,
    );
    assert!(fragment(&items, "Plain").params.is_empty());
    assert_eq!(fragment(&items, "Plain").body, "a: 1");
    assert!(fragment(&items, "Empty").params.is_empty());
    assert_eq!(fragment(&items, "Empty").body, "b: 2");
}

#[test]
fn named_schema_inside_function_docs() {
    let items = extract("/// @openapi-type Money\n/// type: string\nfn _types() {}");
    assert_eq!(kinds(&items), ["named:Money"]);
    assert_eq!(schema(&items, "Money"), json!({ "type": "string" }));
    assert_eq!(items[0].location().line(), 2, "located at the YAML");
}

#[test]
fn empty_named_schemas_and_documents_are_dropped() {
    let items = extract("/// @openapi-type Empty\n///\n/// @openapi\n///   \nfn _empty() {}");
    assert!(items.is_empty(), "{items:#?}");
}

#[test]
fn generic_header_without_a_named_item_is_reported_and_ignored() {
    for code in [
        "/// @openapi<T>\n/// type: object\nfn _generic() {}",
        "/// @openapi<T\n/// type: object\nfn _unterminated() {}",
        "/// @openapi rename<T>\n/// type: object\nfn _odd() {}",
    ] {
        let (items, diagnostics) = extract_with_diagnostics(code);
        assert!(items.is_empty(), "{code}");
        assert_eq!(codes(&diagnostics), [Code::UnnamedBlueprint], "{code}");
        assert_eq!(
            diagnostics
                .iter()
                .next()
                .unwrap()
                .location()
                .unwrap()
                .line(),
            1
        );
    }
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
    let documents = document_items(&items);
    assert_eq!(kinds(&items), ["document"]);
    assert_eq!(documents[0].text, "paths:\n  /health: {}");
    assert_eq!(documents[0].location.line(), 5);
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
    assert_eq!(kinds(&items), ["document", "fragment:After"]);
    assert_eq!(
        document_items(&items)[0].text,
        r#"{"paths": {"/json": {}}}"#
    );
    assert_eq!(fragment(&items, "After").body, "a: b");
}

#[test]
fn json_after_documentation_is_still_recognized() {
    let items =
        extract("/// Intro\n/// {\"paths\": {}}\n/// @openapi-fragment F\n/// a: b\nfn f() {}");
    assert_eq!(kinds(&items), ["document", "fragment:F"]);
    assert_eq!(document_items(&items)[0].text, r#"{"paths": {}}"#);
}

#[test]
fn json_inside_an_openapi_block_is_part_of_the_block() {
    let items = extract("/// @openapi\n/// {\"paths\": {}}\nfn f() {}");
    assert_eq!(kinds(&items), ["document"]);
    assert_eq!(document_items(&items)[0].text, r#"{"paths": {}}"#);
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
    assert_eq!(
        document_items(&items)[0].text,
        "# Health endpoints\n\npaths:\n  /health: {}"
    );
}

#[test]
fn shared_indentation_is_removed_without_splitting_characters() {
    // Regression: byte-based unindent panicked on wide whitespace.
    let items = extract("///    @openapi\n///    a:\n///      b: \u{3000}x\nfn f() {}");
    assert_eq!(document_items(&items)[0].text, "a:\n  b: \u{3000}x");
}

#[test]
fn block_comments_are_split_into_lines() {
    let items = extract("/**\n@openapi\npaths:\n  /block: {}\n*/\nfn f() {}");
    let documents = document_items(&items);
    assert_eq!(documents[0].text, "paths:\n  /block: {}");
    assert_eq!(documents[0].location.line(), 3);
}

#[test]
fn an_interrupted_doc_comment_ends_the_section() {
    let items = extract(
        r#"
        /// @openapi-fragment Paging
        /// - name: page

        /// Not part of the fragment.
        /// @openapi
        /// paths: {}
        fn f() {}
        "#,
    );
    assert_eq!(kinds(&items), ["fragment:Paging", "document"]);
    assert_eq!(fragment(&items, "Paging").body, "- name: page");
    assert_eq!(document_items(&items)[0].text, "paths: {}");
}

/// Prose next to `@openapi` sections is never read as YAML.
mod prose {
    use super::*;
    use crate::common::pipeline;

    const ROOT: &str = "//! @openapi\n//! openapi: 3.1.0\n//! info: { title: T, version: 1.0.0 }\n";

    #[test]
    fn prose_before_a_scoped_section_is_ignored() {
        for scope in ["", " document", " path", " operation"] {
            let items = extract(&format!(
                "/// Some prose: with a colon.\n///\n/// @openapi{scope}\n/// components:\n///   schemas:\n///     X: {{ type: string }}\npub fn docs() {{}}"
            ));
            let documents = document_items(&items);
            assert_eq!(documents.len(), 1, "@openapi{scope}");
            assert_eq!(
                documents[0].text,
                "components:\n  schemas:\n    X: { type: string }"
            );
        }
    }

    #[test]
    fn a_paragraph_of_prose_ends_the_yaml_of_a_section() {
        let (items, diagnostics) = extract_with_diagnostics(
            "/// @openapi\n/// tags: [{ name: x }]\n///\n/// Some prose: with a colon.\n/// More prose.\n///\n/// @openapi document\n/// components:\n///   schemas:\n///     X: { type: string }\npub fn docs() {}",
        );
        assert!(diagnostics.is_empty(), "{diagnostics:?}");
        let texts: Vec<&str> = document_items(&items)
            .iter()
            .map(|d| d.text.as_str())
            .collect();
        assert_eq!(
            texts,
            [
                "tags: [{ name: x }]",
                "components:\n  schemas:\n    X: { type: string }"
            ]
        );
    }

    #[test]
    fn yaml_after_a_blank_line_stays_yaml() {
        let items = extract(
            "/// @openapi\n/// tags:\n///   - name: x\n///\n///     description: |\n///       Text\n///\n///       more.\n///\n/// # a comment\n/// - not: top\n///\n/// {\"x-a\": 1\n///\n/// }\n/// [1]\n/// \"quoted key\": 1\n/// x-b: 2\n/// x-c:\npub fn docs() {}",
        );
        let documents = document_items(&items);
        assert_eq!(documents.len(), 1);
        assert!(
            documents[0].text.ends_with("x-b: 2\nx-c:"),
            "{}",
            documents[0].text
        );
    }

    #[test]
    fn a_markdown_bullet_after_a_blank_line_is_prose() {
        let items = extract(
            "/// @openapi\n/// tags: [{ name: x }]\n///\n/// * a note: for developers\npub fn docs() {}",
        );
        assert_eq!(document_items(&items)[0].text, "tags: [{ name: x }]");
    }

    #[test]
    fn the_whole_pipeline_reads_prose_between_blocks_as_prose() {
        for source in [
            format!(
                "{ROOT}//!\n//! Some prose.\n//!\n//! @openapi document\n//! components:\n//!   schemas:\n//!     X: {{ type: string }}"
            ),
            format!(
                "{ROOT}\n/// @openapi\n/// tags: [{{ name: x }}]\n///\n/// Some prose.\n///\n/// @openapi document\n/// components:\n///   schemas:\n///     X: {{ type: string }}\npub fn docs() {{}}"
            ),
        ] {
            let (document, diagnostics) = pipeline(&[("lib.rs", source.as_str())]);
            assert!(diagnostics.is_empty(), "{diagnostics:?}");
            assert_eq!(
                document["components"]["schemas"]["X"],
                json!({ "type": "string" }),
                "{source}"
            );
        }
    }

    #[test]
    fn scoped_sections_on_types_are_document_blocks() {
        for item in [
            "pub struct Docs;",
            "pub struct Docs { pub a: u8 }",
            "pub enum E { A }",
            "pub type A = u8;",
        ] {
            let (items, diagnostics) = extract_with_diagnostics(&format!(
                "/// Some prose: with a colon.\n///\n/// @openapi document\n/// components:\n///   schemas:\n///     X: {{ type: string }}\n{item}"
            ));
            assert!(diagnostics.is_empty(), "{item}: {diagnostics:?}");
            let documents = document_items(&items);
            assert_eq!(documents.len(), 1, "{item}: {items:?}");
            assert_eq!(documents[0].location.line(), 4);
            assert!(
                kinds(&items).iter().all(|k| !k.starts_with("schema:")),
                "`@openapi document` does not export {item}"
            );
        }
    }

    #[test]
    fn a_type_keeps_its_schema_next_to_a_document_block() {
        let (items, diagnostics) = extract_with_diagnostics(
            "/// A user.\n/// @openapi\n/// minProperties: 1\n///\n/// Developer notes: not YAML.\n///\n/// @openapi document\n/// tags: [{ name: users }]\npub struct User { pub id: u8 }",
        );
        assert!(diagnostics.is_empty(), "{diagnostics:?}");
        let user = schema(&items, "User");
        assert_eq!(user["minProperties"], 1);
        assert_eq!(user["description"], "A user.\n\nDeveloper notes: not YAML.");
        assert_eq!(document_items(&items)[0].text, "tags: [{ name: users }]");
    }
}

#[test]
fn fragment_names_may_be_in_backticks() {
    let items = crate::common::extract(
        "/// @openapi-fragment `Paged(limit, offset)`\n/// limit: {{limit}}\nfn _fragments() {}\n\n/// @openapi-fragment `Plain`\n/// a: 1\npub struct Hidden;",
    );
    let paged = fragment(&items, "Paged");
    assert_eq!(paged.params, ["limit", "offset"]);
    assert_eq!(fragment(&items, "Plain").body, "a: 1");
}

#[test]
fn fenced_yaml_on_functions_and_modules() {
    let items = crate::common::extract(
        "/// Health.\n///\n/// @openapi\n/// ```yaml\n/// paths:\n///   /health: {}\n/// ```\n///\n/// Not YAML.\nfn health() {}\n\n/// @openapi-fragment Limits\n/// ```yaml\n/// x-limit: 1\n/// ```\n/// More docs.\nmod limits {}",
    );
    assert_eq!(document_items(&items)[0].text, "paths:\n  /health: {}");
    assert_eq!(fragment(&items, "Limits").body, "x-limit: 1");
}

#[test]
fn a_fenced_section_takes_exactly_its_lines() {
    // The body starts like a JSON block and two `@scope` lines follow the
    // closing fence: none of these lines is read again, and the block after
    // them is not skipped.
    let items = crate::common::extract(
        "/// @openapi\n/// ```yaml\n/// x-a: 1\n/// { x-b: 2 }\n/// ```\n/// @scope a\n/// @scope b\n/// @openapi\n/// x-next: 1\nfn f() {}",
    );
    let texts: Vec<String> = document_items(&items)
        .iter()
        .map(|d| d.text.clone())
        .collect();
    assert_eq!(
        texts,
        ["x-a: 1\n{ x-b: 2 }\n\n@scope a\n@scope b", "x-next: 1"]
    );
}
