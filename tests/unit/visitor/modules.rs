use crate::common::{extract, yaml};
use oas_forge::visitor::ExtractedItem;
use serde_json::{Value, json};

/// Parses the anonymous document that defines `path`.
fn document_for(items: &[ExtractedItem], path: &str) -> Value {
    items
        .iter()
        .find_map(|item| match item {
            ExtractedItem::Schema { content, .. } if content.contains(path) => Some(yaml(content)),
            _ => None,
        })
        .unwrap_or_else(|| panic!("no document for {path} in {items:#?}"))
}

fn tags_of(items: &[ExtractedItem], path: &str, method: &str) -> Value {
    document_for(items, path)["paths"][path][method]["tags"].clone()
}

#[test]
fn module_tags_are_injected_under_every_verb() {
    let items = extract(
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
            fn handler() {}
        }
        "#,
    );
    assert_eq!(tags_of(&items, "/test", "get"), json!(["GroupA", "GroupB"]));
    assert_eq!(
        tags_of(&items, "/test", "post"),
        json!(["GroupA", "GroupB"])
    );
}

#[test]
fn injected_tags_sit_at_the_top_of_the_operation() {
    let items = extract(
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
    );
    let content = match &items[0] {
        ExtractedItem::Schema { content, .. } => content,
        other => panic!("expected schema, got {other:?}"),
    };
    let get = content.find("get:").unwrap();
    let tags = content.find("tags:").unwrap();
    let desc = content.find("description:").unwrap();
    assert!(get < tags && tags < desc, "{content}");
}

#[test]
fn explicit_tags_are_not_overwritten() {
    let items = extract(
        r#"
        /// tags: [Module]
        mod api {
            /// @openapi
            /// paths:
            ///   /own:
            ///     get:
            ///       tags: [Own]
            fn handler() {}
        }
        "#,
    );
    assert_eq!(tags_of(&items, "/own", "get"), json!(["Own"]));
}

#[test]
fn nested_modules_accumulate_and_siblings_are_isolated() {
    let items = extract(
        r#"
        /// tags: [Outer]
        mod outer {
            /// tags: [Inner]
            mod inner {
                /// @openapi
                /// paths:
                ///   /deep:
                ///     get:
                ///       description: deep
                fn deep() {}
            }

            /// @openapi
            /// paths:
            ///   /shallow:
            ///     get:
            ///       description: shallow
            fn shallow() {}
        }

        mod sibling {
            /// @openapi
            /// paths:
            ///   /free:
            ///     get:
            ///       description: free
            fn free() {}
        }
        "#,
    );
    assert_eq!(tags_of(&items, "/deep", "get"), json!(["Outer", "Inner"]));
    assert_eq!(tags_of(&items, "/shallow", "get"), json!(["Outer"]));
    assert!(tags_of(&items, "/free", "get").is_null());
}

#[test]
fn malformed_tag_lists_are_ignored_without_panicking() {
    // Regression: a `]` before the `[` produced an inverted slice range and panicked.
    let items = extract(
        r#"
        /// x] tags: [A
        /// tags: []
        /// tags: none
        #[doc(hidden)]
        #[doc = include_str!("x.md")]
        #[doc = 5]
        #[allow(dead_code)]
        mod api {
            /// @openapi
            /// paths:
            ///   /x:
            ///     get:
            ///       description: x
            fn handler() {}
        }
        "#,
    );
    assert!(tags_of(&items, "/x", "get").is_null());
}

#[test]
fn module_openapi_block_is_extracted_as_well() {
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
        document_for(&items, "FromModule")["components"]["schemas"]["FromModule"],
        json!({ "type": "string" })
    );
}
