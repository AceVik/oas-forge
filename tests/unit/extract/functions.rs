//! Functions and methods: routes and free blocks.

use super::{document_items, kinds, routes};
use crate::common::{extract, pipeline, yaml};

#[test]
fn route_handlers_keep_their_doc_lines_and_name() {
    let items =
        extract("\n/// Get Users\n/// @route GET /users\n/// @tag Users\nfn get_users() {}");
    let routes = routes(&items);
    assert_eq!(routes.len(), 1);
    assert_eq!(
        routes[0].lines,
        [" Get Users", " @route GET /users", " @tag Users"]
    );
    assert_eq!(routes[0].line_numbers, [2, 3, 4]);
    assert_eq!(routes[0].operation_id.as_deref(), Some("get_users"));
    assert_eq!(routes[0].location.line(), 2);
    assert!(routes[0].tags.is_empty());
}

#[test]
fn route_handlers_compile_with_the_function_name_as_operation_id() {
    let (document, _) = pipeline(&[(
        "api.rs",
        r#"
        /// Get Users
        /// Returns a list of users.
        /// @route GET /users
        fn get_users() {}
        "#,
    )]);
    let operation = &document["paths"]["/users"]["get"];
    assert_eq!(operation["operationId"], "get_users");
    assert_eq!(operation["summary"], "Get Users");
    assert_eq!(operation["description"], "Returns a list of users.");
}

#[test]
fn non_doc_attributes_are_not_part_of_the_dsl() {
    let items = extract(
        "#[get(\"/x\")]\n/// @route GET /x\n#[doc(hidden)]\n#[doc = include_str!(\"x.md\")]\n#[doc = 5]\nfn x() {}",
    );
    assert_eq!(routes(&items)[0].lines, [" @route GET /x"]);
}

#[test]
fn routes_take_precedence_over_openapi_blocks() {
    let items = extract("/// @route GET /x\n/// @openapi\n/// paths: {}\nfn x() {}");
    assert_eq!(kinds(&items), ["route:x"]);
}

#[test]
fn plain_functions_are_ignored() {
    assert!(extract("/// Just a helper.\nfn helper() {}").is_empty());
}

#[test]
fn openapi_blocks_on_functions_are_documents() {
    let items = extract(
        r#"
        /// @openapi
        /// paths:
        ///   /health:
        ///     get:
        ///       description: ok
        fn health() {}
        "#,
    );
    let documents = document_items(&items);
    assert_eq!(documents.len(), 1);
    assert_eq!(
        documents[0].text,
        "paths:\n  /health:\n    get:\n      description: ok"
    );
}

#[test]
fn nested_handlers_are_found() {
    let items = extract("fn outer() {\n    /// @route GET /inner\n    fn inner() {}\n}");
    assert_eq!(routes(&items)[0].operation_id.as_deref(), Some("inner"));
}

#[test]
fn impl_methods_are_extracted() {
    let items = extract(
        r#"
        struct Api;
        impl Api {
            /// @openapi
            /// paths:
            ///   /method: {}
            fn handler(&self) {}

            /// @route GET /routed
            fn routed(&self) {}

            /// Not documented for OpenAPI.
            fn helper(&self) {}
        }
        "#,
    );
    assert_eq!(kinds(&items), ["document", "route:routed"]);
    assert_eq!(
        yaml(&document_items(&items)[0].text),
        yaml("paths: { /method: {} }")
    );
}

#[test]
fn channel_handlers_and_asyncapi_blocks() {
    let items = extract(
        "/// @channel orders\n/// @send Placed: $Order\nfn publish() {}\n/// @asyncapi\n/// info: { description: x }\n/// @openapi\n/// paths: {}\nfn docs() {}",
    );
    assert_eq!(kinds(&items), ["channel:publish", "asyncapi", "document"]);
}
