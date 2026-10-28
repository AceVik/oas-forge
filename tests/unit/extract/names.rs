//! `src/extract/names.rs`: names of Rust types resolve by module path and
//! `use` declarations; colliding names are qualified.

use crate::common::{Project, codes, pipeline};
use oas_forge::Generator;
use oas_forge::diagnostics::Code;
use oas_forge::output::OutputKind;
use serde_json::{Value, json};

/// The root document of the projects below.
const ROOT: &str = "//! @openapi\n//! openapi: 3.1.0\n//! info: { title: T, version: 1.0.0 }\n";

/// A reference to the schema `name`.
fn reference(name: &str) -> Value {
    json!({ "$ref": format!("#/components/schemas/{name}") })
}

/// Two crates with a `Printing` each (one renamed), and a gateway.
fn crates() -> Project {
    let project = Project::new();
    project
        .file(
            "catalog/Cargo.toml",
            "[package]\nname = \"baylee-catalog\"\nversion = \"1.0.0\"\n",
        )
        .file(
            "catalog/src/lib.rs",
            "mod model;\npub use model::Printing;\n",
        )
        .file(
            "catalog/src/model.rs",
            "/// @openapi rename \"CatalogPrinting\"\npub struct Printing { pub set: String }\n",
        )
        .file(
            "cardtext/Cargo.toml",
            "[package]\nname = \"baylee-cardtext\"\nversion = \"1.0.0\"\n",
        )
        .file(
            "cardtext/src/lib.rs",
            "/// @openapi\npub struct Printing { pub face: u8 }\n",
        );
    project
}

/// The document of a generator run on `inputs` of `project`.
fn document(project: &Project, inputs: &[&str]) -> (Value, Vec<Code>) {
    let generator = inputs
        .iter()
        .fold(Generator::new().without_package(), |generator, input| {
            generator.input(project.path(input))
        });
    let spec = generator.build().unwrap();
    (spec.document().clone(), codes(spec.diagnostics()))
}

#[test]
fn a_use_and_a_path_pick_the_type_of_their_crate() {
    let project = crates();
    project.file(
        "gw/src/lib.rs",
        &format!(
            "{ROOT}use baylee_catalog::Printing;\n/// @openapi\npub struct Pool {{\n    pub printings: Vec<Printing>,\n    pub other: baylee_cardtext::Printing,\n    pub direct: ::baylee_catalog::Printing,\n}}\n"
        ),
    );
    let (document, codes) = document(&project, &["catalog/src", "cardtext/src", "gw/src"]);
    assert!(codes.is_empty(), "{codes:?}");
    let pool = &document["components"]["schemas"]["Pool"]["properties"];
    assert_eq!(pool["printings"]["items"], reference("CatalogPrinting"));
    assert_eq!(pool["other"], reference("Printing"));
    assert_eq!(pool["direct"], reference("CatalogPrinting"));
}

#[test]
fn without_a_hint_two_documented_types_of_a_name_are_ambiguous() {
    let project = crates();
    project.file(
        "gw/src/lib.rs",
        &format!("{ROOT}/// @openapi\npub struct Pool {{\n    /// Which one?\n    pub printing: Printing,\n}}\n"),
    );
    let generator = Generator::new()
        .without_package()
        .input(project.path("catalog/src"))
        .input(project.path("cardtext/src"))
        .input(project.path("gw/src"));
    let spec = generator.build().unwrap();
    assert_eq!(
        spec.document()["components"]["schemas"]["Pool"]["properties"]["printing"],
        json!({ "description": "Which one?" })
    );
    let findings: Vec<String> = spec
        .diagnostics()
        .iter()
        .map(|d| d.message().to_string())
        .collect();
    assert_eq!(
        findings,
        [
            "`Printing` may be any of the documented types `baylee_cardtext::Printing`, `baylee_catalog::model::Printing`; write its path or import it with `use`"
        ]
    );
    assert_eq!(codes(spec.diagnostics()), [Code::AmbiguousTypeName]);
}

#[test]
fn single_files_and_directories_without_a_manifest_are_crates_too() {
    let project = crates();
    project
        .file("cards/src/pool.rs", "/// @openapi\npub struct PoolCard { pub id: u32 }\n")
        .file(
            "plain/lib.rs",
            &format!("{ROOT}use cards::pool::PoolCard;\n/// @openapi\npub struct Deck {{ pub cards: Vec<PoolCard> }}\n"),
        );
    let (document, codes) = document(&project, &["cards/src/pool.rs", "plain"]);
    assert!(codes.is_empty(), "{codes:?}");
    assert_eq!(
        document["components"]["schemas"]["Deck"]["properties"]["cards"]["items"],
        reference("PoolCard")
    );
}

#[test]
fn colliding_names_are_qualified_and_references_follow() {
    let (document, diagnostics) = pipeline(&[
        (
            "lib.rs",
            "mod a;\nmod b;\n/// @openapi\npub struct Both { pub a: a::User, pub b: crate::b::User }",
        ),
        (
            "a.rs",
            "/// @openapi\npub struct User { pub id: u64 }\n/// @openapi\npub struct Team { pub lead: User }",
        ),
        (
            "b.rs",
            "/// @openapi\npub struct User { pub name: String }\n/// @openapi\npub struct Club { pub owner: self::User, pub members: Vec<super::b::User> }",
        ),
    ]);
    let schemas = &document["components"]["schemas"];
    let mut names: Vec<&String> = schemas.as_object().unwrap().keys().collect();
    names.sort();
    assert_eq!(
        names,
        ["Both", "Club", "Team", "crate.a.User", "crate.b.User"]
    );
    assert_eq!(schemas["crate.a.User"]["required"], json!(["id"]));
    assert_eq!(
        schemas["Team"]["properties"]["lead"],
        reference("crate.a.User")
    );
    assert_eq!(
        schemas["Club"]["properties"]["owner"],
        reference("crate.b.User")
    );
    assert_eq!(
        schemas["Club"]["properties"]["members"]["items"],
        reference("crate.b.User")
    );
    assert_eq!(
        schemas["Both"]["properties"]["a"],
        reference("crate.a.User")
    );
    assert_eq!(
        schemas["Both"]["properties"]["b"],
        reference("crate.b.User")
    );
    let findings: Vec<String> = diagnostics.iter().map(ToString::to_string).collect();
    assert_eq!(
        findings,
        [
            "warning[OF1004]: schema `User` is defined by several types (`crate::a::User`, `crate::b::User`); this one is named `crate.a.User`; give them distinct names with `@openapi rename` (a.rs:1)",
            "warning[OF1004]: schema `User` is defined by several types (`crate::a::User`, `crate::b::User`); this one is named `crate.b.User`; give them distinct names with `@openapi rename` (b.rs:1)",
        ]
    );
}

#[test]
fn a_schema_name_in_yaml_or_dsl_stays_a_schema_name() {
    let (document, diagnostics) = pipeline(&[
        ("lib.rs", &format!("{ROOT}mod a;\nmod b;")),
        ("a.rs", "/// @openapi\npub struct User { pub id: u64 }"),
        ("b.rs", "/// @openapi\npub struct User { pub name: String }"),
        (
            "c.rs",
            "use crate::a::User;\n/// Get\n/// @route GET /users\n/// @return 200: User\n/// @return 201: $User\n/// responses:\n///   '202':\n///     description: B\n///     content: { application/json: { schema: { $ref: $crate.b.User } } }\npub async fn users() {}",
        ),
    ]);
    let responses = &document["paths"]["/users"]["get"]["responses"];
    let schema = |code: &str| responses[code]["content"]["application/json"]["schema"].clone();
    assert_eq!(schema("200"), reference("crate.a.User"), "a Rust type");
    assert_eq!(
        schema("201"),
        reference("User"),
        "a schema name: no schema is called `User` any more"
    );
    assert_eq!(schema("202"), reference("crate.b.User"));
    assert!(
        codes(&diagnostics).contains(&Code::UnresolvedReference),
        "{diagnostics:?}"
    );
}

#[test]
fn an_undocumented_type_of_the_module_is_not_another_documented_one() {
    let (document, diagnostics) = pipeline(&[
        ("a.rs", "/// @openapi\npub struct Note { pub text: String }"),
        (
            "b.rs",
            "pub struct Note;\n/// @openapi\npub struct Page { pub note: Note, pub other: crate::a::Note }",
        ),
    ]);
    let page = &document["components"]["schemas"]["Page"]["properties"];
    assert_eq!(page["note"], json!({}));
    assert_eq!(page["other"], reference("Note"));
    let findings: Vec<String> = diagnostics
        .iter()
        .map(|d| d.message().to_string())
        .collect();
    assert_eq!(
        findings,
        ["`Note` is `crate::b::Note` here, which is not documented; document it with `@openapi`"]
    );
}

#[test]
fn an_undocumented_type_whose_name_no_documented_type_has_is_just_unknown() {
    let (_, diagnostics) = pipeline(&[(
        "b.rs",
        "pub struct Local;\n/// @openapi\npub struct Page { pub local: Local }",
    )]);
    let findings: Vec<&str> = diagnostics.iter().map(|d| d.message()).collect();
    assert_eq!(findings[0], "`$Local` does not name a known schema");
}

#[test]
fn a_documented_alias_collides_like_a_struct() {
    let (document, _) = pipeline(&[
        (
            "a.rs",
            "/// @openapi\npub type Status = u8;\n/// @openapi\npub struct S { pub status: Status }",
        ),
        ("b.rs", "/// @openapi\npub struct Status { pub code: u16 }"),
    ]);
    let schemas = &document["components"]["schemas"];
    assert_eq!(schemas["crate.a.Status"]["maximum"], 255);
    assert!(schemas["crate.b.Status"]["properties"]["code"].is_object());
    assert_eq!(
        schemas["S"]["properties"]["status"],
        reference("crate.a.Status")
    );
}

#[test]
fn uses_with_groups_renames_globs_and_re_exports() {
    let (document, diagnostics) = pipeline(&[
        ("lib.rs", "pub mod models;\npub mod api;"),
        (
            "models.rs",
            "mod inner {\n    /// @openapi\n    pub struct User { pub id: u64 }\n}\npub use inner::User;\n/// @openapi\npub struct Group { pub id: u64 }\n/// @openapi\npub struct Tag { pub id: u64 }",
        ),
        (
            "other.rs",
            "/// @openapi\npub struct Group { pub other: bool }\n/// @openapi\npub struct Tag { pub other: bool }\n/// @openapi\npub struct User { pub other: bool }",
        ),
        (
            "api.rs",
            r#"
            use crate::models::{self, Group as Team, User};
            use crate::models::*;
            /// @openapi
            pub struct Board {
                pub user: User,
                pub team: Team,
                pub tag: Tag,
                pub by_module: models::Group,
            }
            mod nested {
                use super::super::models::User as Member;
                /// @openapi
                pub struct Inner { pub member: Member }
            }
            "#,
        ),
    ]);
    assert!(
        diagnostics
            .iter()
            .all(|d| d.code() == Code::DuplicateSchema),
        "{diagnostics:?}"
    );
    let schemas = &document["components"]["schemas"];
    let board = &schemas["Board"]["properties"];
    assert_eq!(board["user"], reference("crate.models.inner.User"));
    assert_eq!(board["team"], reference("crate.models.Group"));
    assert_eq!(board["tag"], reference("crate.models.Tag"));
    assert_eq!(board["by_module"], reference("crate.models.Group"));
    assert_eq!(
        schemas["Inner"]["properties"]["member"],
        reference("crate.models.inner.User")
    );
}

#[test]
fn generic_arguments_name_the_instance_by_the_component() {
    let (document, diagnostics) = pipeline(&[
        (
            "a.rs",
            "/// @openapi rename \"Member\"\npub struct User { pub id: u64 }",
        ),
        ("b.rs", "/// @openapi\npub struct User { pub name: String }"),
        (
            "c.rs",
            "use crate::a::User;\n/// @openapi\npub struct Page<T> { pub items: Vec<T> }\n/// @openapi\npub struct Users { pub page: Page<User> }",
        ),
    ]);
    assert!(diagnostics.is_empty(), "{diagnostics:?}");
    let schemas = &document["components"]["schemas"];
    assert_eq!(
        schemas["Users"]["properties"]["page"],
        reference("Page_Member")
    );
    assert_eq!(
        schemas["Page_Member"]["properties"]["items"]["items"],
        reference("Member")
    );
}

#[test]
fn a_module_path_attribute_moves_the_module() {
    let (document, diagnostics) = pipeline(&[
        (
            "src/lib.rs",
            "/// Generated types.\n#[path = \"generated/types.rs\"]\nmod api;\n/// @openapi\npub struct Holder { pub a: api::User, pub b: crate::other::User }\nmod other;",
        ),
        (
            "src/generated/types.rs",
            "/// @openapi\npub struct User { pub id: u64 }",
        ),
        (
            "src/other.rs",
            "/// @openapi\npub struct User { pub name: String }",
        ),
    ]);
    let holder = &document["components"]["schemas"]["Holder"]["properties"];
    assert_eq!(holder["a"], reference("crate.api.User"));
    assert_eq!(holder["b"], reference("crate.other.User"));
    assert_eq!(codes(&diagnostics), [Code::DuplicateSchema; 2]);
}

#[test]
fn module_files_are_found_as_rustc_finds_them() {
    // `#[path]` inside inline modules (of a root and of a module file),
    // `#[path]` on an inline module, and a file declared by a moved file.
    let user = |field: &str| format!("/// @openapi\npub struct User {{ pub {field}: u64 }}");
    let (document, diagnostics) = pipeline(&[
        (
            "src/lib.rs",
            "pub mod outer;\npub mod nested {\n    #[path = \"renamed.rs\"]\n    pub mod inner;\n}\n#[path = \"gen/types.rs\"]\nmod api;\n#[path = \"thread_files\"]\nmod thread {\n    #[path = \"tls.rs\"]\n    mod local;\n}",
        ),
        ("src/nested/renamed.rs", &user("a")),
        (
            "src/outer.rs",
            "pub mod inline {\n    #[path = \"deep.rs\"]\n    pub mod deep_mod;\n}",
        ),
        ("src/outer/inline/deep.rs", &user("b")),
        // rustc reads a file that `#[path]` names like a `mod.rs`: its
        // modules are next to it, also those of its inline blocks.
        (
            "src/gen/types.rs",
            "mod child;\nmod block {\n    mod deep;\n}",
        ),
        ("src/gen/child.rs", &user("c")),
        ("src/gen/block/deep.rs", &user("e")),
        ("src/thread_files/tls.rs", &user("d")),
    ]);
    let schemas = document["components"]["schemas"].as_object().unwrap();
    let mut names: Vec<&str> = schemas.keys().map(String::as_str).collect();
    names.sort_unstable();
    assert_eq!(
        names,
        [
            "crate.api.block.deep.User",
            "crate.api.child.User",
            "crate.nested.inner.User",
            "crate.outer.inline.deep_mod.User",
            "crate.thread.local.User",
        ]
    );
    assert_eq!(codes(&diagnostics), [Code::DuplicateSchema; 5]);
}

#[test]
fn projects_without_collisions_are_unchanged() {
    let (document, diagnostics) = pipeline(&[
        (
            "models.rs",
            "/// @openapi rename \"Member\"\npub struct User { pub id: u64 }",
        ),
        (
            "api.rs",
            "/// @openapi\npub struct Team { pub lead: User, pub other: crate::models::User }\n/// @openapi-type Money\n/// type: string\nfn _t() {}\n/// @openapi\npub struct Price { pub amount: Money }",
        ),
    ]);
    assert!(diagnostics.is_empty(), "{diagnostics:?}");
    let schemas = &document["components"]["schemas"];
    assert_eq!(schemas["Team"]["properties"]["lead"], reference("Member"));
    assert_eq!(schemas["Team"]["properties"]["other"], reference("Member"));
    assert_eq!(schemas["Price"]["properties"]["amount"], reference("Money"));
}

#[test]
fn scoped_documents_keep_the_qualified_schemas_they_use() {
    let project = Project::new();
    project
        .file("Cargo.toml", "[package]\nname = \"shop-api\"\nversion = \"1.0.0\"\n")
        .file("src/root.rs", ROOT)
        .file("src/a.rs", "/// @openapi\npub struct Status { pub a: bool }")
        .file("src/b.rs", "/// @openapi\npub struct Status { pub b: bool }")
        .file(
            "src/routes.rs",
            "/// A\n/// @route GET /a\n/// @scope public\n/// @return 200: crate::a::Status\npub async fn a() {}\n/// B\n/// @route GET /b\n/// @scope admin\n/// @return 200: crate::b::Status\npub async fn b() {}",
        );
    let spec = Generator::new()
        .input(project.path("src"))
        .without_package()
        .build()
        .unwrap();
    let public = spec.render_scoped(OutputKind::Full, "public").unwrap();
    let schemas: Vec<&String> = public["components"]["schemas"]
        .as_object()
        .unwrap()
        .keys()
        .collect();
    assert_eq!(schemas, ["shop_api.a.Status"]);
    assert_eq!(
        public["paths"]["/a"]["get"]["responses"]["200"]["content"]["application/json"]["schema"],
        reference("shop_api.a.Status")
    );
}

#[test]
fn handler_signatures_expand_the_struct_of_their_module() {
    let (document, diagnostics) = pipeline(&[
        ("lib.rs", &format!("{ROOT}mod a;\nmod b;\nmod routes;")),
        (
            "a.rs",
            "/// @openapi\n#[derive(Deserialize)]\npub struct Filter { pub name: String }",
        ),
        (
            "b.rs",
            "/// @openapi\n#[derive(Deserialize)]\npub struct Filter { pub age: u8 }",
        ),
        (
            "routes.rs",
            "use crate::b::Filter;\n/// Search\n/// @route GET /search\npub async fn search(Query(filter): Query<Filter>) {}",
        ),
    ]);
    assert!(
        diagnostics
            .iter()
            .all(|d| d.code() == Code::DuplicateSchema),
        "{diagnostics:?}"
    );
    let parameters = &document["paths"]["/search"]["get"]["parameters"];
    assert_eq!(parameters[0]["name"], "age");
}

#[test]
fn type_parameters_are_no_documented_types() {
    let (document, diagnostics) = pipeline(&[
        (
            "item.rs",
            "/// @openapi rename \"Article\"\npub struct Item { pub id: u64 }",
        ),
        (
            "page.rs",
            "/// @openapi\npub struct Page<Item> { pub items: Vec<Item> }\n/// @openapi\npub struct User { pub id: u64 }\n/// @openapi\npub struct Users { pub page: Page<User> }\n/// @openapi\n#[serde(tag = \"kind\")]\npub enum Event<Item> { Added { item: Item } }\n/// @openapi\npub struct Log { pub event: Event<User> }",
        ),
    ]);
    assert!(diagnostics.is_empty(), "{diagnostics:?}");
    let schemas = &document["components"]["schemas"];
    assert_eq!(
        schemas["Page_User"]["properties"]["items"]["items"],
        reference("User")
    );
    assert_eq!(
        schemas["EventAdded_User"]["properties"]["item"],
        reference("User")
    );
}

#[test]
fn inputs_given_with_dots_find_their_modules() {
    let project = Project::new();
    project
        .file("src/lib.rs", &format!("{ROOT}#[path = \"gen/types.rs\"]\nmod api;\nmod other;\n/// @openapi\npub struct Holder {{ pub a: api::User }}"))
        .file("src/gen/types.rs", "/// @openapi\npub struct User { pub id: u64 }")
        .file("src/other.rs", "/// @openapi\npub struct User { pub name: String }");
    let input = project.root().join(".").join("src");
    let spec = Generator::new()
        .input(input)
        .without_package()
        .build()
        .unwrap();
    let holder = &spec.document()["components"]["schemas"]["Holder"]["properties"];
    let name = holder["a"]["$ref"].as_str().unwrap();
    assert!(name.ends_with(".api.User"), "{name}");
}

/// Edges of the resolution.
mod edges {
    use super::*;

    /// Module aliases (`use crate::m as a1;`) are followed eight deep, like
    /// re-exports.
    #[test]
    fn module_aliases_are_followed_to_a_limit() {
        let resolved = |aliases: usize| {
            let path = vec!["a1"; aliases].join("::");
            let (document, _) = pipeline(&[
                ("lib.rs", "mod m;\nmod other;\npub use crate::m as a1;"),
                (
                    "m.rs",
                    "pub use crate::m as a1;\n/// @openapi\npub struct User { pub id: u64 }",
                ),
                (
                    "other.rs",
                    "/// @openapi\npub struct User { pub name: String }",
                ),
                (
                    "s.rs",
                    &format!("/// @openapi\npub struct S {{ pub u: crate::{path}::User }}"),
                ),
            ]);
            document["components"]["schemas"]["S"]["properties"]["u"].clone()
        };
        assert_eq!(resolved(8), reference("crate.m.User"));
        assert_eq!(resolved(9), json!({}));
    }

    #[test]
    fn module_alias_cycles_end() {
        let (document, _) = pipeline(&[
            (
                "lib.rs",
                "mod c;\npub use crate::b as a;\npub use crate::a as b;",
            ),
            (
                "c.rs",
                "/// @openapi\npub struct User { pub id: u64 }\n/// @openapi\npub struct S { pub u: crate::a::User }",
            ),
        ]);
        assert_eq!(
            document["components"]["schemas"]["S"]["properties"]["u"],
            reference("User"),
            "the one documented `User`"
        );
    }

    #[test]
    fn re_export_cycles_end() {
        let (document, diagnostics) = pipeline(&[
            ("lib.rs", "mod a;\nmod b;\nmod c;"),
            ("a.rs", "pub use crate::b::User;"),
            ("b.rs", "pub use crate::a::User;"),
            (
                "c.rs",
                "/// @openapi\npub struct User { pub id: u64 }\n/// @openapi\npub struct S { pub u: crate::a::User }",
            ),
        ]);
        assert!(diagnostics.is_empty(), "{diagnostics:?}");
        assert_eq!(
            document["components"]["schemas"]["S"]["properties"]["u"],
            reference("User"),
            "the one documented `User`"
        );
    }

    #[test]
    fn glob_re_export_cycles_end() {
        let (document, diagnostics) = pipeline(&[
            ("lib.rs", "mod a;\nmod b;\nmod c;"),
            ("a.rs", "pub use crate::b::*;"),
            ("b.rs", "pub use crate::a::*;"),
            (
                "c.rs",
                "/// @openapi\npub struct User { pub id: u64 }\n/// @openapi\npub struct S { pub u: crate::a::User }",
            ),
        ]);
        assert!(diagnostics.is_empty(), "{diagnostics:?}");
        assert_eq!(
            document["components"]["schemas"]["S"]["properties"]["u"],
            reference("User"),
            "the one documented `User`"
        );
    }

    #[test]
    fn a_file_declared_before_its_declaring_file_is_found_in_a_later_pass() {
        // `src/a.rs` keeps its module but is named by `#[path]`, so its
        // modules are next to it; it is read before `src/lib.rs` marks it.
        let (document, _) = pipeline(&[
            ("src/x.rs", "/// @openapi\npub struct User { pub id: u64 }"),
            ("src/a.rs", "mod x;"),
            (
                "src/lib.rs",
                "#[path = \"a.rs\"]\nmod a;\n/// @openapi\npub struct User { pub name: String }",
            ),
        ]);
        let mut names: Vec<&String> = document["components"]["schemas"]
            .as_object()
            .unwrap()
            .keys()
            .collect();
        names.sort_unstable();
        assert_eq!(names, ["crate.User", "crate.a.x.User"]);
    }

    #[test]
    fn a_path_of_a_crate_finds_the_type_of_a_module_it_does_not_show() {
        let project = crates();
        project.file(
            "gw/src/lib.rs",
            &format!("{ROOT}/// @openapi\npub struct Pool {{ pub p: baylee_catalog::Printing }}\n"),
        );
        // Only the module of the catalog is scanned, not its `pub use`.
        let (document, codes) = document(
            &project,
            &["catalog/src/model.rs", "cardtext/src", "gw/src"],
        );
        assert!(codes.is_empty(), "{codes:?}");
        assert_eq!(
            document["components"]["schemas"]["Pool"]["properties"]["p"],
            reference("CatalogPrinting")
        );
    }

    #[test]
    fn two_glob_imports_with_the_name_are_ambiguous() {
        let (document, diagnostics) = pipeline(&[
            ("lib.rs", "mod a;\nmod b;\nmod c;"),
            ("a.rs", "/// @openapi\npub struct User { pub id: u64 }"),
            ("b.rs", "/// @openapi\npub struct User { pub name: String }"),
            (
                "c.rs",
                "use crate::a::*;\nuse crate::b::*;\n/// @openapi\npub struct S { pub u: User, pub again: User }",
            ),
        ]);
        assert_eq!(
            document["components"]["schemas"]["S"]["properties"]["u"],
            json!({})
        );
        let findings: Vec<String> = diagnostics
            .iter()
            .filter(|d| d.code() == Code::AmbiguousTypeName)
            .map(|d| d.message().to_string())
            .collect();
        assert_eq!(
            findings,
            [
                "`User` may be any of the documented types `crate::a::User`, `crate::b::User`; write its path or import it with `use`"
            ],
            "reported once for the item"
        );
    }

    #[test]
    fn a_transparent_alias_of_the_module_is_replaced_later() {
        let (document, diagnostics) = pipeline(&[
            (
                "a.rs",
                "type Status = u8;\n/// @openapi\npub struct S { pub status: Status }",
            ),
            ("b.rs", "/// @openapi\npub struct Status { pub code: u16 }"),
        ]);
        assert!(diagnostics.is_empty(), "{diagnostics:?}");
        assert_eq!(
            document["components"]["schemas"]["S"]["properties"]["status"]["maximum"],
            255
        );
    }

    #[test]
    fn modules_of_directories_inline_blocks_and_renamed_self_imports() {
        let (document, diagnostics) = pipeline(&[
            ("src/lib.rs", "mod models;\nmod api;"),
            (
                "src/models/mod.rs",
                "/// @openapi\npub struct User { pub id: u64 }",
            ),
            (
                "src/other.rs",
                "/// @openapi\npub struct User { pub name: String }",
            ),
            (
                "src/api.rs",
                r#"
                use crate::{models::{self as m}};
                mod outer {
                    pub mod inner {
                        /// @openapi
                        pub struct Deep { pub u: super::super::m::User, pub n: Note }
                        /// @openapi
                        pub struct Note { pub text: String }
                    }
                }
                "#,
            ),
        ]);
        assert!(
            diagnostics
                .iter()
                .all(|d| d.code() == Code::DuplicateSchema),
            "{diagnostics:?}"
        );
        let deep = &document["components"]["schemas"]["Deep"]["properties"];
        assert_eq!(deep["u"], reference("crate.models.User"));
        assert_eq!(deep["n"], reference("Note"));
    }

    #[test]
    fn colliding_templates_are_qualified_and_each_use_picks_its_own() {
        let (document, diagnostics) = pipeline(&[
            (
                "a.rs",
                "/// @openapi\npub struct Page<T> { pub items: Vec<T> }\n/// @openapi\npub struct A { pub p: Page<u8> }",
            ),
            (
                "b.rs",
                "/// @openapi\npub struct Page<T> { pub entries: Vec<T> }\n/// @openapi\npub struct B { pub p: Page<u8> }",
            ),
            (
                "c.rs",
                "/// @openapi\npub struct C { pub a: crate::a::Page<bool>, pub b: crate::b::Page<bool> }",
            ),
        ]);
        let schemas = &document["components"]["schemas"];
        assert_eq!(
            schemas["A"]["properties"]["p"],
            reference("crate.a.Page_u8")
        );
        assert_eq!(
            schemas["B"]["properties"]["p"],
            reference("crate.b.Page_u8")
        );
        assert_eq!(
            schemas["C"]["properties"]["a"],
            reference("crate.a.Page_bool")
        );
        assert_eq!(
            schemas["C"]["properties"]["b"],
            reference("crate.b.Page_bool")
        );
        assert!(
            schemas["crate.a.Page_u8"]["properties"]
                .get("items")
                .is_some()
        );
        assert!(
            schemas["crate.b.Page_u8"]["properties"]
                .get("entries")
                .is_some()
        );
        let messages: Vec<&str> = diagnostics.iter().map(|d| d.message()).collect();
        assert_eq!(
            codes(&diagnostics),
            [Code::DuplicateSchema; 2],
            "{messages:?}"
        );
        assert!(messages[0].starts_with("template `Page` is defined by several types"));
    }

    #[test]
    fn a_template_and_a_schema_of_one_name_do_not_collide() {
        let (document, diagnostics) = pipeline(&[
            (
                "a.rs",
                "/// @openapi\npub struct Page<T> { pub items: Vec<T> }",
            ),
            ("b.rs", "/// @openapi\npub struct Page { pub number: u32 }"),
            (
                "c.rs",
                "/// @openapi\npub struct C { pub list: Page<u8>, pub page: Page }",
            ),
        ]);
        assert!(diagnostics.is_empty(), "{diagnostics:?}");
        let schemas = &document["components"]["schemas"];
        assert_eq!(schemas["C"]["properties"]["list"], reference("Page_u8"));
        assert_eq!(schemas["C"]["properties"]["page"], reference("Page"));
    }

    #[test]
    fn an_ambiguous_template_or_type_argument_is_reported_and_not_instantiated() {
        let (document, diagnostics) = pipeline(&[
            (
                "a.rs",
                "/// @openapi\npub struct User { pub id: u64 }\n/// @openapi\npub struct Page<T> { pub items: Vec<T> }",
            ),
            (
                "b.rs",
                "/// @openapi\npub struct User { pub name: String }\n/// @openapi\npub struct Page<T> { pub entries: Vec<T> }",
            ),
            (
                "c.rs",
                "use crate::a::Page;\n/// @openapi\npub struct Listing { pub users: Page<Vec<User>> }",
            ),
            (
                "d.rs",
                "/// @openapi\npub struct Shelf { pub books: Page<u8> }",
            ),
            (
                "e.rs",
                "pub struct Page<T>(T);\n/// @openapi\npub struct Local { pub own: Option<Page<u8>> }",
            ),
        ]);
        let schemas = &document["components"]["schemas"];
        // Nothing is instantiated for a name that is not clear.
        let names: Vec<&String> = schemas.as_object().unwrap().keys().collect();
        assert!(
            names
                .iter()
                .all(|name| !name.contains("_User") && !name.ends_with("Page_u8")),
            "{names:?}"
        );
        assert_eq!(schemas["Listing"]["properties"]["users"], json!({}));
        assert_eq!(schemas["Shelf"]["properties"]["books"], json!({}));
        assert_eq!(
            schemas["Local"]["properties"]["own"],
            json!({ "anyOf": [{}, { "type": "null" }] })
        );
        let findings: Vec<(Code, &str)> = diagnostics
            .iter()
            .map(|d| (d.code(), d.message()))
            .filter(|(code, _)| *code == Code::AmbiguousTypeName)
            .collect();
        assert_eq!(findings.len(), 3, "{findings:?}");
        assert!(findings.iter().any(|(_, m)| *m == "`Page` is `crate::e::Page` here, which is not documented; document it with `@openapi`"));
        assert!(findings.iter().any(|(_, m)| m.starts_with(
            "`User` may be any of the documented types `crate::a::User`, `crate::b::User`"
        )));
        assert!(findings.iter().any(|(_, m)| m.starts_with(
            "`Page` may be any of the documented types `crate::a::Page`, `crate::b::Page`"
        )));
        // No follow-up findings about the names that were left out.
        assert!(
            !codes(&diagnostics).iter().any(|code| matches!(
                code,
                Code::UnresolvedReference | Code::DanglingReference | Code::UnknownBlueprint
            )),
            "{diagnostics:?}"
        );
    }

    #[test]
    fn an_unreadable_manifest_is_reported_once_and_the_directory_names_the_crate() {
        let project = Project::new();
        project
            .file("shop-api/Cargo.toml", "[package\nname = \"shop\"\n")
            .file("shop-api/src/lib.rs", &format!("{ROOT}mod a;\nmod b;"))
            .file(
                "shop-api/src/a.rs",
                "/// @openapi\npub struct User { pub id: u64 }",
            )
            .file(
                "shop-api/src/b.rs",
                "/// @openapi\npub struct User { pub name: String }",
            )
            .file("nameless/Cargo.toml", "[package]\nversion = \"1.0.0\"\n")
            .file(
                "nameless/src/lib.rs",
                "/// @openapi\npub struct Tag { pub name: String }",
            );
        let spec = Generator::new()
            .without_package()
            .input(project.path("shop-api/src"))
            .input(project.path("nameless/src"))
            .build()
            .unwrap();
        let schemas = &spec.document()["components"]["schemas"];
        assert!(schemas.get("shop_api.a.User").is_some(), "{schemas}");
        assert!(schemas.get("Tag").is_some(), "{schemas}");
        let unreadable: Vec<_> = spec
            .diagnostics()
            .iter()
            .filter(|d| d.code() == Code::UnreadableManifest)
            .collect();
        assert_eq!(unreadable.len(), 2, "{:?}", spec.diagnostics());
        let files: Vec<_> = unreadable
            .iter()
            .map(|d| d.location().unwrap().file().to_path_buf())
            .collect();
        assert!(
            files.contains(&project.path("shop-api/Cargo.toml")),
            "{files:?}"
        );
        assert!(
            files.contains(&project.path("nameless/Cargo.toml")),
            "{files:?}"
        );
        let messages: Vec<&str> = unreadable.iter().map(|d| d.message()).collect();
        assert!(
            messages
                .iter()
                .any(|m| m.contains("TOML parse error at line 1")
                    && m.ends_with("the crate is named `shop_api` after its directory")
                    && !m.contains('\n')),
            "{messages:?}"
        );
        assert!(
            messages.iter().any(|m| m.contains("has no `name`")
                && m.ends_with("the crate is named `nameless` after its directory")),
            "{messages:?}"
        );
    }

    #[test]
    fn a_file_that_declares_itself_ends() {
        // rustc rejects it ("circular modules"); the table stops moving it.
        let (document, _) = pipeline(&[(
            "src/lib.rs",
            "#[path = \"lib.rs\"]\nmod again;\n/// @openapi\npub struct Tag { pub name: String }",
        )]);
        assert!(document["components"]["schemas"].get("Tag").is_some());
    }

    #[test]
    fn crate_directories_are_identifiers() {
        let project = Project::new();
        project
            .file("2024-api/src/lib.rs", &format!("{ROOT}mod a;\nmod b;"))
            .file(
                "2024-api/src/a.rs",
                "/// @openapi\npub struct User { pub id: u64 }",
            )
            .file(
                "2024-api/src/b.rs",
                "/// @openapi\npub struct User { pub name: String }",
            );
        let (document, _) = document(&project, &["2024-api/src"]);
        assert!(
            document["components"]["schemas"]
                .get("_2024_api.a.User")
                .is_some(),
            "{document}"
        );
    }
}

#[test]
fn a_directory_named_like_a_module_file_is_a_module() {
    // Only the file itself may be `main.rs`, `lib.rs` or `mod.rs`.
    let (document, _) = pipeline(&[
        (
            "src/main/user.rs",
            "/// @openapi\npub struct User { pub id: u64 }",
        ),
        (
            "src/other.rs",
            "/// @openapi\npub struct User { pub name: String }",
        ),
    ]);
    let names: Vec<&String> = document["components"]["schemas"]
        .as_object()
        .unwrap()
        .keys()
        .collect();
    assert_eq!(names, ["crate.main.user.User", "crate.other.User"]);
}
