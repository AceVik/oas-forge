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
