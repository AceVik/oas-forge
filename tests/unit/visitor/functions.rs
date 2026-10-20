use crate::common::{extract, yaml};
use oas_forge::dsl::parse_route_dsl;
use oas_forge::visitor::ExtractedItem;

/// Returns `(operation_id, content, line)` of every route item.
fn routes(items: &[ExtractedItem]) -> Vec<(&str, &str, usize)> {
    items
        .iter()
        .filter_map(|item| match item {
            ExtractedItem::RouteDSL {
                content,
                line,
                operation_id,
            } => Some((operation_id.as_str(), content.as_str(), *line)),
            _ => None,
        })
        .collect()
}

#[test]
fn route_handler_emits_raw_dsl_named_after_the_function() {
    let items =
        extract("\n/// Get Users\n/// @route GET /users\n/// @tag Users\nfn get_users() {}");
    assert_eq!(
        routes(&items),
        [(
            "get_users",
            " Get Users\n @route GET /users\n @tag Users",
            2
        )]
    );
}

#[test]
fn emitted_dsl_compiles_with_the_function_name_as_operation_id() {
    let items = extract(
        r#"
        /// Get Users
        /// Returns a list of users.
        /// @route GET /users
        fn get_users() {}
        "#,
    );
    let (op_id, content, _) = routes(&items)[0];
    let doc_lines: Vec<String> = content.lines().map(str::to_string).collect();

    let doc = yaml(&parse_route_dsl(&doc_lines, op_id).unwrap());
    let op = &doc["paths"]["/users"]["get"];
    assert_eq!(op["operationId"], "get_users");
    assert_eq!(op["summary"], "Get Users");
    assert_eq!(op["description"], "Returns a list of users.");
}

#[test]
fn non_doc_attributes_are_not_part_of_the_dsl() {
    let items = extract(
        "#[get(\"/x\")]\n/// @route GET /x\n#[doc(hidden)]\n#[doc = include_str!(\"x.md\")]\n#[doc = 5]\nfn x() {}",
    );
    assert_eq!(routes(&items)[0].1, " @route GET /x");
}

#[test]
fn plain_functions_are_ignored() {
    assert!(extract("/// Just a helper.\nfn helper() {}").is_empty());
}

#[test]
fn openapi_blocks_on_functions_are_raw_documents() {
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
    match items.as_slice() {
        [
            ExtractedItem::Schema {
                name: None,
                content,
                ..
            },
        ] => {
            assert_eq!(
                content,
                "paths:\n  /health:\n    get:\n      description: ok"
            );
        }
        other => panic!("expected one anonymous document, got {other:#?}"),
    }
}

#[test]
fn nested_handlers_are_found() {
    let items = extract("fn outer() {\n    /// @route GET /inner\n    fn inner() {}\n}");
    assert_eq!(routes(&items)[0].0, "inner");
}

#[test]
fn impl_methods_with_openapi_blocks_are_extracted() {
    let items = extract(
        r#"
        struct Api;
        impl Api {
            /// @openapi
            /// paths:
            ///   /method: {}
            fn handler(&self) {}

            /// Not documented for OpenAPI.
            fn helper(&self) {}
        }
        "#,
    );
    assert!(matches!(
        items.as_slice(),
        [ExtractedItem::Schema { name: None, content, .. }] if content.contains("/method")
    ));
}
