//! Modules: tag lists for the operations inside and free blocks.

use super::{document_items, routes};
use crate::common::{extract, pipeline};
use serde_json::{Value, json};

fn tags_of(document: &Value, path: &str, method: &str) -> Value {
    document["paths"][path][method]["tags"].clone()
}

#[test]
fn module_tags_are_attached_to_items_inside() {
    let items = extract(
        r#"
        /// tags: [GroupA, GroupB]
        mod api {
            /// @openapi
            /// paths: {}
            fn handler() {}

            /// @route GET /x
            fn routed() {}
        }
        "#,
    );
    assert_eq!(document_items(&items)[0].tags, ["GroupA", "GroupB"]);
    assert_eq!(routes(&items)[0].tags, ["GroupA", "GroupB"]);
}

#[test]
fn module_tags_apply_to_every_operation_without_tags() {
    let (document, _) = pipeline(&[(
        "api.rs",
        r#"
        /// @openapi
        /// tags: [GroupA, GroupB]
        mod api {
            /// @openapi
            /// paths:
            ///   /test:
            ///     get:
            ///       description: read
            ///     post:
            ///       description: create
            ///     parameters: []
            fn handler() {}
        }
        "#,
    )]);
    assert_eq!(
        tags_of(&document, "/test", "get"),
        json!(["GroupA", "GroupB"])
    );
    assert_eq!(
        tags_of(&document, "/test", "post"),
        json!(["GroupA", "GroupB"])
    );
    assert_eq!(document["paths"]["/test"]["parameters"], json!([]));
    assert_eq!(
        document["tags"],
        json!([{ "name": "GroupA" }, { "name": "GroupB" }]),
        "the module's own block declares the tags as objects"
    );
}

#[test]
fn injected_tags_come_first_in_the_operation() {
    let (document, _) = pipeline(&[(
        "api.rs",
        r#"
        /// tags: [MyTag]
        mod my_mod {
            /// @openapi
            /// paths:
            ///   /foo:
            ///     get:
            ///       description: op
            fn my_fn() {}
        }
        "#,
    )]);
    let keys: Vec<_> = document["paths"]["/foo"]["get"]
        .as_object()
        .unwrap()
        .keys()
        .cloned()
        .collect();
    assert_eq!(keys, ["tags", "description"]);
}

#[test]
fn explicit_tags_are_not_overwritten() {
    let (document, _) = pipeline(&[(
        "api.rs",
        r#"
        /// tags: [Module]
        mod api {
            /// @openapi
            /// paths:
            ///   /own:
            ///     get:
            ///       tags: [Own]
            fn handler() {}

            /// @route GET /routed
            /// @tag Explicit
            fn routed() {}
        }
        "#,
    )]);
    assert_eq!(tags_of(&document, "/own", "get"), json!(["Own"]));
    assert_eq!(tags_of(&document, "/routed", "get"), json!(["Explicit"]));
}

#[test]
fn routes_inherit_module_tags() {
    // Regression: module tags were only applied to `@openapi` blocks.
    let (document, _) = pipeline(&[(
        "api.rs",
        "/// tags: [Users]\nmod users {\n    /// @route GET /users\n    fn list() {}\n}",
    )]);
    assert_eq!(tags_of(&document, "/users", "get"), json!(["Users"]));
}

#[test]
fn nested_modules_accumulate_and_siblings_are_isolated() {
    let items = extract(
        r#"
        /// tags: [Outer]
        mod outer {
            /// tags: [Inner]
            mod inner {
                /// @route GET /deep
                fn deep() {}
            }

            /// @route GET /shallow
            fn shallow() {}
        }

        mod sibling {
            /// @route GET /free
            fn free() {}
        }
        "#,
    );
    let tags: Vec<_> = routes(&items).iter().map(|r| r.tags.clone()).collect();
    assert_eq!(tags, [vec!["Outer", "Inner"], vec!["Outer"], vec![]]);
}

#[test]
fn malformed_tag_lists_are_ignored_without_panicking() {
    // Regression: a `]` before the `[` produced an inverted slice range and panicked.
    let items = extract(
        r#"
        /// x] tags: [A
        /// tags: []
        /// tags: none
        /// tags: [ , ]
        #[doc(hidden)]
        #[doc = include_str!("x.md")]
        #[doc = 5]
        #[allow(dead_code)]
        mod api {
            /// @route GET /x
            fn handler() {}
        }
        "#,
    );
    assert!(routes(&items)[0].tags.is_empty());
}

#[test]
fn module_openapi_blocks_are_documents() {
    let items = extract(
        r#"
        /// @openapi
        /// components:
        ///   schemas:
        ///     FromModule: { type: string }
        mod api {}
        "#,
    );
    assert_eq!(
        document_items(&items)[0].text,
        "components:\n  schemas:\n    FromModule: { type: string }"
    );
}

#[test]
fn route_defaults_are_collected_per_module() {
    let items = extract(
        r#"
        /// Intro text.
        /// @route-defaults
        /// @prefix /api
        /// @tag Api
        /// @openapi
        /// tags: [{ name: Api }]
        mod api {
            /// @route-defaults
            /// @prefix /v1
            mod v1 {
                /// @route GET /x
                fn x() {}
            }
            /// @route GET /y
            fn y() {}
        }
        /// @route GET /z
        fn z() {}
        "#,
    );
    let defaults: Vec<_> = routes(&items).iter().map(|r| r.defaults.clone()).collect();
    assert_eq!(
        defaults,
        [
            vec![
                "@route-defaults",
                " @prefix /api",
                " @tag Api",
                "@route-defaults",
                " @prefix /v1"
            ],
            vec!["@route-defaults", " @prefix /api", " @tag Api"],
            vec![],
        ]
    );
    let lines: Vec<_> = routes(&items)
        .iter()
        .map(|r| r.default_line_numbers.clone())
        .collect();
    assert_eq!(lines, [vec![3, 4, 5, 9, 10], vec![3, 4, 5], vec![]]);
    assert_eq!(
        document_items(&items)[0].text,
        "tags: [{ name: Api }]",
        "the module's @openapi block is still a document"
    );
}

#[test]
fn route_defaults_end_an_openapi_block() {
    let items = extract(
        "/// @openapi
/// x-a: 1
/// @route-defaults
/// @tag T
mod m {}",
    );
    assert_eq!(document_items(&items)[0].text, "x-a: 1");
}

#[test]
fn only_lists_of_names_are_module_tags() {
    let items = extract(
        r#"
        /// @openapi
        /// tags: [{ name: Users, description: "All, users" }]
        mod users {
            /// @route GET /users
            fn list() {}
        }
        /// tags: ["Admin Area", Ops]
        /// Other tags: [not, these]
        mod admin {
            /// @route GET /admin
            fn admin() {}
        }
        /// @openapi tags: [Inline]
        mod inline {
            /// @route GET /inline
            fn inline() {}
        }
        "#,
    );
    let tags: Vec<&Vec<String>> = routes(&items).iter().map(|r| &r.tags).collect();
    assert_eq!(
        tags,
        [
            &Vec::<String>::new(),
            &vec!["Admin Area".to_string(), "Ops".to_string()],
            &vec!["Inline".to_string()],
        ]
    );
}
