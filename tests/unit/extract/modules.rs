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

/// Modules in files of their own: `mod name;` declarations and `//!`
/// defaults at the top of a file.
mod file_modules {
    use super::{document_items, routes};
    use oas_forge::diagnostics::Diagnostics;
    use oas_forge::extract::{self, Item, ModuleItem, Options};
    use std::path::Path;

    fn extract_at(path: &str, source: &str) -> Vec<Item> {
        extract::extract_str(
            Path::new(path),
            source,
            Options::new(),
            &mut Diagnostics::new(),
        )
        .unwrap()
    }

    fn modules(items: &[Item]) -> Vec<&ModuleItem> {
        items
            .iter()
            .filter_map(|item| match item {
                Item::Module(module) => Some(module),
                _ => None,
            })
            .collect()
    }

    /// The candidate files of every declaration, `/`-separated.
    fn files(items: &[Item]) -> Vec<Vec<String>> {
        modules(items)
            .iter()
            .map(|m| {
                m.files
                    .iter()
                    .map(|f| f.to_string_lossy().replace('\\', "/"))
                    .collect()
            })
            .collect()
    }

    #[test]
    fn declarations_name_the_files_of_their_module() {
        let items = extract_at(
            "src/lib.rs",
            "pub mod routes;\nmod r#type;\npub mod api {\n  pub mod v1;\n  #[path = \"legacy\"]\n  pub mod old {\n    mod x;\n  }\n}",
        );
        assert_eq!(
            files(&items),
            [
                ["src/routes.rs", "src/routes/mod.rs"],
                ["src/type.rs", "src/type/mod.rs"],
                ["src/api/v1.rs", "src/api/v1/mod.rs"],
                ["src/api/legacy/x.rs", "src/api/legacy/x/mod.rs"],
            ]
        );
        let names: Vec<&str> = modules(&items).iter().map(|m| m.name.as_str()).collect();
        assert_eq!(names, ["routes", "type", "v1", "x"]);
        assert_eq!(modules(&items)[2].location.line(), 4);
        // `mod.rs` and `main.rs` hold their modules next to them, other
        // files in a directory of their name.
        assert_eq!(
            files(&extract_at("src/routes/mod.rs", "mod decks;")),
            [["src/routes/decks.rs", "src/routes/decks/mod.rs"]]
        );
        assert_eq!(
            files(&extract_at("src/main.rs", "mod cli;")),
            [["src/cli.rs", "src/cli/mod.rs"]]
        );
        assert_eq!(
            files(&extract_at(
                "src/routes/social.rs",
                "mod feed;\nmod inner { mod deep; }"
            )),
            [
                ["src/routes/social/feed.rs", "src/routes/social/feed/mod.rs"],
                [
                    "src/routes/social/inner/deep.rs",
                    "src/routes/social/inner/deep/mod.rs"
                ],
            ]
        );
    }

    #[test]
    fn path_attributes_name_the_file() {
        // Outside inline modules relative to the directory of the file;
        // inside them like the module's own directory.
        let items = extract_at(
            "src/routes/social.rs",
            "#[path = \"shared/feed.rs\"]\nmod feed;\nmod inner {\n  #[path = \"deep.rs\"]\n  mod deep;\n}\n#[path = \"../gen/./api.rs\"]\nmod api;\n#[doc = \"not a path\"]\n#[path(oops)]\nmod odd;",
        );
        assert_eq!(
            files(&items),
            [
                vec!["src/routes/shared/feed.rs"],
                vec!["src/routes/social/inner/deep.rs"],
                vec!["src/gen/api.rs"],
                vec!["src/routes/social/odd.rs", "src/routes/social/odd/mod.rs"],
            ]
        );
        let items = extract_at("lib.rs", "#[path = \"../shared.rs\"]\nmod shared;");
        assert_eq!(files(&items), [["../shared.rs"]]);
    }

    #[test]
    fn declarations_carry_the_defaults_around_them() {
        let items = extract_at(
            "src/routes/mod.rs",
            r#"//! Routes.
//! @route-defaults
//! @tag Routes

/// @route-defaults
/// @tag Decks
pub mod decks;
pub mod social;
/// @route-defaults
/// @prefix /admin
pub mod admin {
    pub mod users;
}
"#,
        );
        let chains: Vec<(Vec<String>, Vec<usize>)> = modules(&items)
            .iter()
            .map(|m| (m.defaults.clone(), m.default_line_numbers.clone()))
            .collect();
        let routes_defaults = ["@route-defaults", " @tag Routes"];
        assert_eq!(
            chains,
            [
                (
                    [&routes_defaults[..], &["@route-defaults", " @tag Decks"]]
                        .concat()
                        .iter()
                        .map(|s| s.to_string())
                        .collect(),
                    vec![2, 3, 5, 6]
                ),
                (
                    routes_defaults.iter().map(|s| s.to_string()).collect(),
                    vec![2, 3]
                ),
                (
                    [
                        &routes_defaults[..],
                        &["@route-defaults", " @prefix /admin"]
                    ]
                    .concat()
                    .iter()
                    .map(|s| s.to_string())
                    .collect(),
                    vec![2, 3, 9, 10]
                ),
            ]
        );
    }

    #[test]
    fn file_defaults_apply_to_every_route_of_the_file() {
        let items = extract_at(
            "src/routes/social.rs",
            r#"//! Social routes.
//! @route-defaults
//! @prefix /social
//!
//! @route GET /virtual

/// @route GET /devices
pub async fn devices() {}

mod inner {
    /// @route-defaults
    /// @tag Inner
    mod deeper {
        /// @route GET /deep
        fn deep() {}
    }
}
"#,
        );
        let chains: Vec<(Vec<String>, Vec<usize>)> = routes(&items)
            .iter()
            .map(|r| (r.defaults.clone(), r.default_line_numbers.clone()))
            .collect();
        let file = vec![
            "@route-defaults".to_string(),
            " @prefix /social".into(),
            String::new(),
        ];
        assert_eq!(
            chains,
            [
                (file.clone(), vec![2, 3, 4]),
                (file.clone(), vec![2, 3, 4]),
                (
                    [
                        file.clone(),
                        vec!["@route-defaults".into(), " @tag Inner".into()]
                    ]
                    .concat(),
                    vec![2, 3, 4, 11, 12]
                ),
            ]
        );
    }

    #[test]
    fn file_defaults_end_at_a_separator_or_a_gap() {
        let items = extract_at(
            "api.rs",
            "//! @route-defaults\n//! @tag A\n//! ---\n//! @tag Ignored\n\n/// @route GET /a\nfn a() {}",
        );
        assert_eq!(routes(&items)[0].defaults, ["@route-defaults", " @tag A"]);
        let items = extract_at(
            "api.rs",
            "//! @route-defaults\n//! @tag A\n\n//! @tag Ignored\n\n/// @route GET /a\nfn a() {}",
        );
        assert_eq!(routes(&items)[0].defaults, ["@route-defaults", " @tag A"]);
    }

    #[test]
    fn file_defaults_end_an_openapi_block() {
        let items = extract_at(
            "src/lib.rs",
            "//! @openapi\n//! openapi: 3.1.0\n//! @route-defaults\n//! @tag Api\n//! x-team: core\n\n/// @route GET /a\nfn a() {}",
        );
        assert_eq!(document_items(&items)[0].text, "openapi: 3.1.0");
        assert_eq!(
            routes(&items)[0].defaults,
            ["@route-defaults", " @tag Api", " x-team: core"]
        );
    }

    #[test]
    fn inner_docs_of_inline_modules_are_module_defaults() {
        let items = extract_at(
            "api.rs",
            "mod api {\n    //! @route-defaults\n    //! @tag Api\n\n    /// @route GET /x\n    fn x() {}\n}",
        );
        assert_eq!(routes(&items)[0].defaults, ["@route-defaults", " @tag Api"]);
        assert!(
            modules(&items).is_empty(),
            "an inline module is no declaration"
        );
    }
}
