//! Type aliases: exported with `@openapi`, transparent without it.

use super::kinds;
use crate::common::{codes, extract, extract_with_diagnostics, pipeline, schema, schema_names};
use oas_forge::diagnostics::{Code, Diagnostics};
use oas_forge::extract::Item;
use serde_json::{Value, json};

#[test]
fn alias_is_reflected_with_overrides() {
    let items = extract(
        r#"
        /// @openapi
        /// format: uuid
        /// description: User ID Alias
        type UserId = String;
        "#,
    );
    assert_eq!(
        schema(&items, "UserId"),
        json!({ "type": "string", "format": "uuid", "description": "User ID Alias" })
    );
}

#[test]
fn doc_lines_before_tag_become_description() {
    let items =
        extract("/// Identifier of a user.\n/// @openapi format: uuid\ntype UserId = String;");
    assert_eq!(
        schema(&items, "UserId"),
        json!({ "type": "string", "format": "uuid", "description": "Identifier of a user." })
    );
}

#[test]
fn exported_alias_to_custom_type_is_a_smart_ref() {
    let items = extract(
        "/// @openapi\n#[allow(dead_code)]\n#[doc(hidden)]\n#[doc = include_str!(\"x.md\")]\n#[doc = 5]\ntype Owner = User;",
    );
    assert_eq!(schema(&items, "Owner"), json!({ "$ref": "$User" }));
}

#[test]
fn renamed_aliases_keep_their_rust_name() {
    let items = extract("/// @openapi rename \"OwnerRef\"\ntype Owner = User;");
    match &items[0] {
        Item::Schema(s) => assert_eq!(
            (s.name.as_str(), s.rust_name.as_str()),
            ("OwnerRef", "Owner")
        ),
        other => panic!("expected a schema, got {other:?}"),
    }
}

#[test]
fn invalid_override_is_reported_and_ignored() {
    let (items, diagnostics) =
        extract_with_diagnostics("/// @openapi\n/// format: [unclosed\ntype Id = u64;");
    assert_eq!(
        schema(&items, "Id"),
        json!({ "type": "integer", "format": "int64", "minimum": 0 })
    );
    assert_eq!(codes(&diagnostics), [Code::InvalidOverride]);
}

#[test]
fn optional_aliases_are_nullable() {
    let items = extract("/// @openapi\ntype MaybeName = Option<String>;");
    assert_eq!(
        schema(&items, "MaybeName"),
        json!({ "type": ["string", "null"] })
    );
}

#[test]
fn generic_exported_aliases_are_blueprints() {
    let items = extract("/// @openapi\npub type Paged<T> = Page<T>;");
    let paged = crate::extract::blueprint(&items, "Paged");
    assert_eq!(paged.params, ["T"]);
    assert_eq!(paged.schema, json!({ "$ref": "$Page<$T>" }));
}

#[test]
fn a_blueprint_header_declares_a_template() {
    let items = extract(
        "/// @openapi<T>\n/// type: object\n/// properties:\n///   data: { $ref: $T }\npub type Envelope<T> = Inner<T>;\n/// @openapi<T> description: Any\npub type Plain = serde_json::Value;",
    );
    let envelope = crate::extract::blueprint(&items, "Envelope");
    assert_eq!(envelope.params, ["T"]);
    assert_eq!(
        envelope.schema,
        json!({ "type": "object", "properties": { "data": { "$ref": "$T" } } })
    );
    let plain = crate::extract::blueprint(&items, "Plain");
    assert_eq!(plain.params, ["T"]);
    assert_eq!(plain.schema, json!({ "description": "Any" }));
}

#[test]
fn a_blueprint_alias_is_instantiated() {
    let (document, diagnostics) = pipeline(&[(
        "lib.rs",
        r#"
        /// @openapi<T>
        /// type: object
        /// properties:
        ///   data: { $ref: $T }
        pub type Envelope<T> = Inner<T>;
        /// @openapi
        pub struct User { pub id: u64 }
        /// @openapi
        pub struct H { pub e: Envelope<User> }
        "#,
    )]);
    assert!(diagnostics.is_empty(), "{diagnostics:?}");
    assert_eq!(
        document["components"]["schemas"]["Envelope_User"],
        json!({
            "type": "object",
            "properties": { "data": { "$ref": "#/components/schemas/User" } },
        })
    );
}

/// The schemas of a pipeline run that must not report anything.
fn schemas(files: &[(&str, &str)]) -> Value {
    let (document, diagnostics) = pipeline(files);
    assert!(diagnostics.is_empty(), "{diagnostics:?}");
    document["components"]["schemas"].clone()
}

/// The names of the component schemas, sorted.
fn names(schemas: &Value) -> Vec<String> {
    let mut names: Vec<String> = schemas
        .as_object()
        .map(|map| map.keys().cloned().collect())
        .unwrap_or_default();
    names.sort();
    names
}

mod transparent {
    use super::*;

    #[test]
    fn unannotated_aliases_are_extracted_as_aliases() {
        let (items, diagnostics) = extract_with_diagnostics(
            "/// Developer notes.\npub(crate) type Admin = Arc<Console>;\ntype Pair<A, B> = (A, B);\n/// @openapi skip\ntype Hidden = u8;",
        );
        assert!(diagnostics.is_empty(), "{diagnostics:?}");
        assert!(schema_names(&items).is_empty());
        assert_eq!(kinds(&items), ["alias:Admin", "alias:Pair", "alias:Hidden"]);
        let Item::Alias(admin) = &items[0] else {
            panic!("expected an alias, got {:?}", items[0]);
        };
        // No description: the alias stands for its target.
        assert_eq!(admin.schema, json!({ "$ref": "$Console" }));
        assert!(admin.params.is_empty());
        assert_eq!(admin.location.line(), 1, "where its doc comment starts");
        let Item::Alias(pair) = &items[1] else {
            panic!("expected an alias, got {:?}", items[1]);
        };
        assert_eq!(pair.params, ["A", "B"]);
    }

    #[test]
    fn unused_aliases_produce_nothing() {
        let (document, diagnostics) = pipeline(&[(
            "lib.rs",
            "pub(crate) type Admin = Arc<Console>;\ntype Tell = UnboundedSender<Envelope>;\ntype Answer<T> = Result<T, Refusal>;\n/// @openapi\npub struct Ping { pub ok: bool }",
        )]);
        assert!(diagnostics.is_empty(), "{diagnostics:?}");
        assert_eq!(
            names(&document["components"]["schemas"]),
            ["Ping"],
            "no alias becomes a component"
        );
    }

    #[test]
    fn a_field_of_an_alias_type_gets_the_target_schema() {
        let schemas = schemas(&[(
            "lib.rs",
            r#"
            pub type Stamp = chrono::DateTime<chrono::Utc>;
            /// The id.
            pub type Id = u64;
            /// @openapi
            pub struct Log {
                pub at: Stamp,
                /// When it ends.
                pub until: Option<Stamp>,
                pub ids: Vec<Id>,
            }
            "#,
        )]);
        assert_eq!(names(&schemas), ["Log"]);
        let log = &schemas["Log"]["properties"];
        assert_eq!(
            log["at"],
            json!({ "type": "string", "format": "date-time" })
        );
        assert_eq!(
            log["until"],
            json!({
                "anyOf": [{ "type": "string", "format": "date-time" }, { "type": "null" }],
                "description": "When it ends.",
            })
        );
        assert_eq!(
            log["ids"]["items"],
            json!({ "type": "integer", "format": "int64", "minimum": 0 })
        );
    }

    #[test]
    fn an_alias_of_a_documented_type_is_a_reference_to_it() {
        let schemas = schemas(&[
            ("a.rs", "pub type Owner = User;"),
            ("b.rs", "/// @openapi\npub struct User { pub id: u64 }"),
            (
                "c.rs",
                "/// @openapi\npub struct Repo {\n    /// Who owns it.\n    pub owner: Owner,\n}",
            ),
        ]);
        assert_eq!(names(&schemas), ["Repo", "User"]);
        assert_eq!(
            schemas["Repo"]["properties"]["owner"],
            json!({ "$ref": "#/components/schemas/User", "description": "Who owns it." })
        );
    }

    #[test]
    fn overrides_of_the_field_win_over_the_target() {
        let schemas = schemas(&[(
            "lib.rs",
            "type Name = String;\n/// @openapi\npub struct S {\n    /// @openapi\n    /// format: email\n    pub name: Name,\n}",
        )]);
        assert_eq!(
            schemas["S"]["properties"]["name"],
            json!({ "type": "string", "format": "email" })
        );
    }

    #[test]
    fn chains_of_aliases_are_followed_in_the_files_of_the_aliases() {
        let schemas = schemas(&[
            (
                "a.rs",
                "pub type Outer = Inner;\ntype Inner = Vec<Id>;\ntype Id = String;",
            ),
            (
                "b.rs",
                "type Id = u8;\n/// @openapi\npub struct S { pub outer: Outer, pub id: Id }",
            ),
        ]);
        // `Id` in `a.rs` is the `Id` of `a.rs`, also when `Outer` is used in `b.rs`.
        let s = &schemas["S"]["properties"];
        assert_eq!(
            s["outer"],
            json!({ "type": "array", "items": { "type": "string" } })
        );
        assert_eq!(s["id"]["maximum"], 255);
    }

    #[test]
    fn identical_aliases_in_several_files_are_one() {
        let refusal = "type Refusal = (StatusCode, Json<ErrorBody>);";
        let schemas = schemas(&[
            ("a.rs", refusal),
            ("b.rs", refusal),
            (
                "c.rs",
                "/// @openapi\npub struct ErrorBody { pub error: String }\n/// @openapi\npub struct StatusCode(u16);\n/// @openapi\npub struct Report { pub refusal: Refusal }",
            ),
        ]);
        assert_eq!(names(&schemas), ["ErrorBody", "Report", "StatusCode"]);
        assert_eq!(
            schemas["Report"]["properties"]["refusal"]["prefixItems"],
            json!([
                { "$ref": "#/components/schemas/StatusCode" },
                { "$ref": "#/components/schemas/ErrorBody" },
            ])
        );
    }

    #[test]
    fn an_ambiguous_name_is_reported_and_not_resolved() {
        let (document, diagnostics) = pipeline(&[
            (
                "a.rs",
                "type Refusal = (u16, String);\n/// @openapi\npub struct A { pub r: Refusal }",
            ),
            ("b.rs", "type Refusal = u8;"),
            (
                "c.rs",
                "/// @openapi\npub struct C {\n    pub r: Refusal,\n    pub again: Refusal,\n}",
            ),
        ]);
        let schemas = &document["components"]["schemas"];
        // In its own file, the local alias wins.
        assert_eq!(schemas["A"]["properties"]["r"]["type"], "array");
        assert_eq!(
            schemas["C"]["properties"]["r"],
            json!({ "$ref": "#/components/schemas/Refusal" })
        );
        assert_eq!(
            codes(&diagnostics),
            [Code::AmbiguousAlias, Code::DanglingReference]
        );
        let finding = diagnostics.iter().next().map(ToString::to_string);
        assert_eq!(
            finding.as_deref(),
            Some(
                "warning[OF3012]: `Refusal` names type aliases with different targets (a.rs:1, b.rs:1); the reference is not resolved (c.rs:1)"
            )
        );
    }

    #[test]
    fn a_documented_schema_wins_over_an_alias_of_another_file() {
        let schemas = schemas(&[
            (
                "a.rs",
                "pub type Owner = Team;\n/// @openapi\npub struct Team { pub id: u8 }\n/// @openapi\npub struct Local { pub owner: Owner }",
            ),
            (
                "b.rs",
                "/// @openapi\npub struct Owner { pub name: String }",
            ),
            ("c.rs", "/// @openapi\npub struct Repo { pub owner: Owner }"),
        ]);
        assert_eq!(
            schemas["Repo"]["properties"]["owner"],
            json!({ "$ref": "#/components/schemas/Owner" })
        );
        // The alias of the file wins over the documented schema.
        assert_eq!(
            schemas["Local"]["properties"]["owner"],
            json!({ "$ref": "#/components/schemas/Team" })
        );
    }

    #[test]
    fn an_alias_of_a_type_with_its_own_name_is_that_type() {
        let schemas = schemas(&[
            (
                "api.rs",
                "pub type User = crate::models::User;\n/// @openapi\npub struct Repo { pub owner: User }",
            ),
            ("models.rs", "/// @openapi\npub struct User { pub id: u64 }"),
        ]);
        assert_eq!(
            schemas["Repo"]["properties"]["owner"],
            json!({ "$ref": "#/components/schemas/User" })
        );
    }

    #[test]
    fn cycles_are_reported_once() {
        let (document, diagnostics) = pipeline(&[(
            "lib.rs",
            "type A = B;\ntype B = A;\n/// @openapi\npub struct S { pub a: A, pub b: Vec<A> }\n/// @openapi\npub struct T { pub a: A }",
        )]);
        assert_eq!(
            document["components"]["schemas"]["S"]["properties"]["a"],
            json!({ "$ref": "#/components/schemas/A" })
        );
        let cycles: Vec<String> = diagnostics
            .iter()
            .filter(|d| d.code() == Code::AliasCycle)
            .map(ToString::to_string)
            .collect();
        assert_eq!(
            cycles,
            [
                "warning[OF3013]: type alias `A` refers to itself (`A` → `B` → `A`); it is not resolved (lib.rs:1)"
            ]
        );
        assert!(
            !codes(&diagnostics).contains(&Code::UnresolvedReference),
            "{diagnostics:?}"
        );
    }

    #[test]
    fn generic_aliases_are_substituted_where_used() {
        let schemas = schemas(&[(
            "lib.rs",
            r#"
            pub type Result<T> = std::result::Result<T, AppError>;
            pub type Paged<T> = Page<T>;
            pub type List<T> = Vec<T>;
            pub type Pages<T> = Paged<List<T>>;
            /// @openapi
            pub struct Page<T> { pub items: Vec<T> }
            /// @openapi
            pub struct User { pub id: u64 }
            /// @openapi
            pub struct H {
                pub p: Paged<User>,
                pub l: List<User>,
                pub n: Pages<User>,
            }
            "#,
        )]);
        // An alias as a type argument stands for its target: the instance
        // is named as for `Page<Vec<User>>`.
        assert_eq!(
            names(&schemas),
            ["H", "Page_User", "Page_Vec_User", "User"],
            "no `Paged_User`, no `Result`"
        );
        let h = &schemas["H"]["properties"];
        assert_eq!(h["p"], json!({ "$ref": "#/components/schemas/Page_User" }));
        assert_eq!(
            h["l"],
            json!({ "type": "array", "items": { "$ref": "#/components/schemas/User" } })
        );
        assert_eq!(
            h["n"],
            json!({ "$ref": "#/components/schemas/Page_Vec_User" })
        );
        assert_eq!(
            schemas["Page_Vec_User"]["properties"]["items"]["items"],
            json!({ "type": "array", "items": { "$ref": "#/components/schemas/User" } })
        );
    }

    #[test]
    fn an_alias_of_an_option_is_optional_like_an_option() {
        let (document, diagnostics) = pipeline(&[
            (
                "lib.rs",
                r#"
            pub type MaybeId = Option<u64>;
            pub type Chained = MaybeId;
            pub type Maybe<T> = Option<T>;
            pub type Full = std::option::Option<bool>;
            // No `std::option::Option`: not optional.
            pub type NotStd = crate::option::Option<u8>;
            pub type Odd = std::other::Option<u8>;
            use crate::types::*;
            /// @openapi
            pub struct User { pub id: u64 }
            /// @openapi
            #[derive(Serialize, Deserialize)]
            pub struct Both {
                pub id: MaybeId,
                pub chained: Chained,
                pub user: Maybe<User>,
                pub full: Full,
                pub note: models::Note,
                pub plain: Option<u64>,
                pub glob: GlobId,
                pub required: u64,
                pub not_std: NotStd,
                pub odd: Odd,
            }
            /// @openapi
            #[derive(Serialize)]
            pub struct Out {
                pub id: MaybeId,
                #[serde(skip_serializing_if = "Option::is_none")]
                pub skipped: MaybeId,
            }
            /// List
            /// @route GET /items
            /// @query-param id: MaybeId "Filter"
            /// @return 204:
            pub async fn list() {}
            "#,
            ),
            // The target of an alias is read in its own file.
            (
                "models.rs",
                "pub type Note = Option<Text>;\n/// @openapi\npub struct Text { pub body: String }",
            ),
            ("types.rs", "pub type GlobId = Option<u32>;"),
        ]);
        assert!(diagnostics.is_empty(), "{diagnostics:?}");
        let schemas = &document["components"]["schemas"];
        let both = &schemas["Both"];
        assert_eq!(both["required"], json!(["required", "not_std", "odd"]));
        let properties = &both["properties"];
        assert_eq!(
            properties["id"],
            json!({ "type": ["integer", "null"], "format": "int64", "minimum": 0 })
        );
        assert_eq!(properties["chained"], properties["id"]);
        assert_eq!(properties["plain"], properties["id"]);
        let nullable = |name: &str| json!({ "anyOf": [{ "$ref": format!("#/components/schemas/{name}") }, { "type": "null" }] });
        assert_eq!(properties["user"], nullable("User"));
        assert_eq!(properties["note"], nullable("Text"));
        assert_eq!(properties["full"], json!({ "type": ["boolean", "null"] }));
        assert_eq!(
            properties["glob"],
            json!({ "type": ["integer", "null"], "format": "int64", "minimum": 0, "maximum": u32::MAX })
        );
        // An output type requires every field it writes; `skip_serializing_if`
        // leaves `None` out, so that value is not nullable.
        let out = &schemas["Out"];
        assert_eq!(out["required"], json!(["id"]));
        assert_eq!(
            out["properties"]["skipped"],
            json!({ "type": "integer", "format": "int64", "minimum": 0 })
        );
        let parameter = &document["paths"]["/items"]["get"]["parameters"][0];
        assert_eq!(parameter["required"], json!(false));
    }

    #[test]
    fn aliases_as_type_arguments_name_the_instance_by_their_target() {
        let schemas = schemas(&[
            (
                "lib.rs",
                r#"
            pub type Owner = User;
            pub type Chain = Owner;
            pub type Maybe = Option<u64>;
            pub type Pair = (u8, u8);
            /// @openapi
            pub struct Page<T> { pub items: Vec<T> }
            /// @openapi
            pub struct User { pub id: u64 }
            /// @openapi
            pub struct H {
                pub a: Page<Owner>,
                pub b: Page<Chain>,
                pub c: Page<User>,
                pub d: Page<Maybe>,
                pub e: Page<Pair>,
                pub f: Page<models::Member>,
            }
            "#,
            ),
            // The target of an alias is read in its own file.
            (
                "models.rs",
                "pub type Member = Account;\n/// @openapi\npub struct Account { pub id: u64 }",
            ),
        ]);
        assert_eq!(
            names(&schemas),
            [
                "Account",
                "H",
                "Page_Account",
                "Page_Option_u64",
                "Page_Pair",
                "Page_User",
                "User"
            ]
        );
        let h = &schemas["H"]["properties"];
        for field in ["a", "b", "c"] {
            assert_eq!(
                h[field],
                json!({ "$ref": "#/components/schemas/Page_User" }),
                "{field}"
            );
        }
        assert_eq!(
            schemas["Page_Option_u64"]["properties"]["items"]["items"]["type"],
            json!(["integer", "null"])
        );
    }

    #[test]
    fn alias_cycles_as_type_arguments_keep_their_name() {
        let (document, diagnostics) = pipeline(&[(
            "lib.rs",
            r#"
            pub type A = B;
            pub type B = A;
            pub type Owner = User;
            /// @openapi
            pub struct Page<T> { pub items: Vec<T> }
            /// @openapi
            pub struct Pair<L, R> { pub left: L, pub right: R }
            /// @openapi
            pub struct User { pub id: u64 }
            /// @openapi
            pub struct H { pub cycle: Page<A>, pub twice: Pair<Owner, Owner> }
            "#,
        )]);
        let h = &document["components"]["schemas"]["H"]["properties"];
        assert_eq!(h["cycle"], json!({ "$ref": "#/components/schemas/Page_A" }));
        assert_eq!(
            h["twice"],
            json!({ "$ref": "#/components/schemas/Pair_User_User" })
        );
        assert!(
            codes(&diagnostics).contains(&Code::AliasCycle),
            "{diagnostics:?}"
        );
    }

    #[test]
    fn an_alias_of_a_generic_type_is_instantiated() {
        let schemas = schemas(&[(
            "lib.rs",
            "type Users = Page<User>;\n/// @openapi\npub struct Page<T> { pub items: Vec<T> }\n/// @openapi\npub struct User { pub id: u64 }\n/// @openapi\npub struct H { pub users: Users }",
        )]);
        assert_eq!(
            schemas["H"]["properties"]["users"],
            json!({ "$ref": "#/components/schemas/Page_User" })
        );
    }

    #[test]
    fn generic_alias_cycles_are_reported() {
        let (_, diagnostics) = pipeline(&[(
            "lib.rs",
            "type A<T> = B<T>;\ntype B<T> = A<T>;\n/// @openapi\npub struct S { pub a: A<u8> }",
        )]);
        assert!(
            codes(&diagnostics).contains(&Code::AliasCycle),
            "{diagnostics:?}"
        );
    }

    #[test]
    fn a_generic_alias_with_other_arguments_is_not_replaced() {
        let (document, diagnostics) = pipeline(&[(
            "lib.rs",
            "type Pair<A, B> = (A, B);\n/// @openapi\npub struct S { pub p: Pair<u8> }",
        )]);
        assert_eq!(
            document["components"]["schemas"]["S"]["properties"]["p"],
            json!({ "$ref": "#/components/schemas/Pair_u8" })
        );
        assert!(
            codes(&diagnostics).contains(&Code::UnknownBlueprint),
            "{diagnostics:?}"
        );
    }

    #[test]
    fn an_alias_of_the_file_replaces_a_built_in_name() {
        let schemas = schemas(&[
            (
                "a.rs",
                "type Duration = u32;\n/// @openapi\npub struct Job { pub timeout: Duration }",
            ),
            (
                "b.rs",
                "/// @openapi\npub struct Other { pub timeout: Duration }",
            ),
        ]);
        assert_eq!(
            schemas["Job"]["properties"]["timeout"]["format"], "int64",
            "the alias"
        );
        assert_eq!(
            schemas["Other"]["properties"]["timeout"]["required"],
            json!(["secs", "nanos"]),
            "an alias in another file does not replace a built-in name"
        );
    }

    #[test]
    fn routes_and_yaml_blocks_follow_aliases() {
        let (document, diagnostics) = pipeline(&[
            (
                "a.rs",
                "pub type Owner = User;\npub type Stamp = chrono::DateTime<chrono::Utc>;",
            ),
            (
                "b.rs",
                r#"
                /// @openapi
                pub struct User { pub id: u64 }
                /// Get the owner
                /// @route GET /owner
                /// @return 200: Owner
                pub async fn owner() {}
                /// @openapi-type Event
                /// type: object
                /// properties:
                ///   at: { $ref: $Stamp, description: When }
                fn _schemas() {}
                "#,
            ),
        ]);
        assert!(
            diagnostics.iter().all(|d| d.code() == Code::GeneratedRoot),
            "{diagnostics:?}"
        );
        assert_eq!(
            document["paths"]["/owner"]["get"]["responses"]["200"]["content"]["application/json"]["schema"],
            json!({ "$ref": "#/components/schemas/User" })
        );
        assert_eq!(
            document["components"]["schemas"]["Event"]["properties"]["at"],
            json!({ "type": "string", "format": "date-time", "description": "When" })
        );
    }

    #[test]
    fn single_file_extraction_reports_nothing_for_aliases() {
        let mut diagnostics = Diagnostics::new();
        let items = oas_forge::extract::extract_str(
            std::path::Path::new("lib.rs"),
            "type Listening = TapIo<tokio::net::TcpListener, fn(&mut tokio::net::TcpStream)>;",
            oas_forge::extract::Options::new(),
            &mut diagnostics,
        )
        .unwrap();
        assert_eq!(kinds(&items), ["alias:Listening"]);
        assert!(diagnostics.is_empty(), "{diagnostics:?}");
    }

    #[test]
    fn an_alias_that_cannot_be_documented_is_reported_where_used() {
        let (document, diagnostics) = pipeline(&[
            (
                "main.rs",
                "type Listening = axum::serve::TapIo<tokio::net::TcpListener, fn(&mut tokio::net::TcpStream)>;\ntype Clock = Arc<dyn Fn() -> OffsetDateTime + Send + Sync>;",
            ),
            (
                "server.rs",
                "/// @openapi\npub struct Server { pub listening: Listening, pub again: Option<Listening> }\n/// @openapi\npub struct Other { pub listening: Listening }",
            ),
        ]);
        let schemas = &document["components"]["schemas"];
        assert_eq!(schemas["Server"]["properties"]["listening"], json!({}));
        assert_eq!(schemas["Other"]["properties"]["listening"], json!({}));
        // Reported once, where the type is written; the unused `Clock` is not.
        let findings: Vec<String> = diagnostics.iter().map(ToString::to_string).collect();
        assert_eq!(
            findings,
            [
                "warning[OF1010]: `fn(&mut tokio::net::TcpStream)` cannot be documented (a function pointer, trait object, `impl Trait` or macro); its schema is `{}` (main.rs:1)"
            ]
        );
    }

    #[test]
    fn an_exported_alias_that_cannot_be_documented_is_reported() {
        let (items, diagnostics) = extract_with_diagnostics(
            "/// @openapi\npub type Handler = Box<dyn Fn(Request) -> Response>;",
        );
        assert_eq!(schema(&items, "Handler"), json!({}));
        assert_eq!(codes(&diagnostics), [Code::UndocumentableType]);
    }
}
