use crate::common::{Project, snippet, yaml};
use oas_forge::index::Registry;
use oas_forge::scanner::{
    Snippet, preprocess_macros, scan_directories, substitute_smart_references,
};
use serde_json::{Value, json};
use std::collections::HashSet;
use std::path::{Path, PathBuf};

mod macros {
    use super::*;

    fn expand(content: &str, registry: &mut Registry) -> String {
        preprocess_macros(&snippet(content), registry).content
    }

    #[test]
    fn vec_shorthand_becomes_inline_array() {
        let out = expand("a: $Vec<Tag>\nb: [$Vec<X>, $Vec<Y>]", &mut Registry::new());
        assert_eq!(
            out,
            "a: { type: array, items: { $ref: \"#/components/schemas/Tag\" } }\n\
             b: [{ type: array, items: { $ref: \"#/components/schemas/X\" } }, \
             { type: array, items: { $ref: \"#/components/schemas/Y\" } }]"
        );
    }

    #[test]
    fn generics_are_instantiated_and_flattened() {
        let mut registry = Registry::new();
        registry.insert_blueprint("Page".into(), vec!["T".into()], "items: $T".into());

        let out = expand("$ref: $Page<User>", &mut registry);

        assert_eq!(out, "$ref: $Page_User");
        assert_eq!(registry.concrete_schemas["Page_User"], "items: $User");
    }

    #[test]
    fn unknown_insert_becomes_parameter_reference() {
        let mut registry = Registry::new();
        assert_eq!(
            expand("  @insert QueryParam", &mut registry),
            "  - $ref: \"#/components/parameters/QueryParam\""
        );
        assert_eq!(
            expand("    - @insert Limit", &mut registry),
            "    - $ref: \"#/components/parameters/Limit\""
        );
    }

    #[test]
    fn known_insert_is_left_for_the_preprocessor() {
        let mut registry = Registry::new();
        registry.insert_fragment("Known".into(), vec![], "a: b".into());
        assert_eq!(expand("  @insert Known", &mut registry), "  @insert Known");
    }

    #[test]
    fn extend_is_auto_quoted() {
        let mut registry = Registry::new();
        assert_eq!(
            expand("  @extend Secured(\"admin\")", &mut registry),
            "  x-openapi-extend: 'Secured(\"admin\")'"
        );
        assert_eq!(
            expand("  @extend 'User'", &mut registry),
            "  x-openapi-extend: '''User'''"
        );
    }

    #[test]
    fn snippet_metadata_is_preserved() {
        let input = Snippet {
            content: "a: 1".into(),
            file_path: PathBuf::from("src/api.rs"),
            line_number: 7,
            operation_id: Some("op".into()),
        };
        let out = preprocess_macros(&input, &mut Registry::new());
        assert_eq!(out.file_path, input.file_path);
        assert_eq!(out.line_number, 7);
        assert_eq!(out.operation_id.as_deref(), Some("op"));
    }
}

mod smart_references {
    use super::*;

    fn substitute(content: &str, known: &[&str]) -> String {
        let schemas: HashSet<String> = known.iter().map(|s| s.to_string()).collect();
        substitute_smart_references(content, &schemas)
    }

    #[test]
    fn known_schema_becomes_quoted_component_ref() {
        assert_eq!(
            substitute("$ref: $User", &["User"]),
            "$ref: \"#/components/schemas/User\""
        );
    }

    #[test]
    fn already_quoted_reference_is_not_quoted_twice() {
        assert_eq!(
            substitute("$ref: \"$User\"", &["User"]),
            "$ref: \"#/components/schemas/User\""
        );
    }

    #[test]
    fn only_whole_identifiers_are_replaced() {
        assert_eq!(
            substitute("$UserName $User_ $User", &["User"]),
            "$UserName $User_ \"#/components/schemas/User\""
        );
    }

    #[test]
    fn unknown_names_and_non_identifiers_are_untouched() {
        let text = "$Unknown costs $5, $ and $";
        assert_eq!(substitute(text, &["User"]), text);
    }

    #[test]
    fn underscore_identifiers_and_unicode_text() {
        assert_eq!(
            substitute("Größe: $_Internal", &["_Internal"]),
            "Größe: \"#/components/schemas/_Internal\""
        );
    }
}

mod scan {
    use super::*;

    fn scan(project: &Project) -> (Vec<Snippet>, Registry) {
        scan_directories(&[project.path("src")], &[]).expect("scan should succeed")
    }

    /// Parses every snippet that is valid YAML; raw fallbacks are skipped.
    fn documents(snippets: &[Snippet]) -> Vec<Value> {
        snippets
            .iter()
            .filter_map(|s| serde_yaml_ng::from_str(&s.content).ok())
            .collect()
    }

    fn find<'a>(docs: &'a [Value], pointer: &str) -> &'a Value {
        docs.iter()
            .find_map(|d| d.pointer(pointer))
            .unwrap_or_else(|| panic!("{pointer} not found in {docs:#?}"))
    }

    #[test]
    fn empty_directory_is_an_error() {
        let project = Project::new();
        project.dir("src");
        assert!(matches!(
            scan_directories(&[project.path("src")], &[]),
            Err(oas_forge::error::Error::NoFilesFound)
        ));
    }

    #[test]
    fn missing_directory_is_an_io_error() {
        let project = Project::new();
        assert!(matches!(
            scan_directories(&[project.path("nope")], &[]),
            Err(oas_forge::error::Error::Io(_))
        ));
    }

    #[test]
    fn invalid_rust_is_a_parse_error() {
        let project = Project::new();
        project.file("src/lib.rs", "fn {");
        assert!(matches!(
            scan_directories(&[project.path("src")], &[]),
            Err(oas_forge::error::Error::Parse { .. })
        ));
    }

    #[test]
    fn non_source_files_are_ignored() {
        let project = Project::new();
        project
            .file("src/README.md", "# docs")
            .file("src/LICENSE", "MIT")
            .file("src/notes.txt", "@openapi");
        let (snippets, _) = scan(&project);
        assert!(snippets.is_empty());
    }

    #[test]
    fn rust_schemas_carry_their_source_location() {
        let project = Project::new();
        project.file(
            "src/models.rs",
            "\n\n/// @openapi\npub struct User { pub id: i64 }",
        );

        let (snippets, registry) = scan(&project);

        assert_eq!(snippets.len(), 1);
        assert_eq!(snippets[0].file_path, project.path("src/models.rs"));
        assert_eq!(snippets[0].line_number, 3);
        assert!(snippets[0].operation_id.is_none());
        assert!(registry.schemas.contains_key("User"));
        assert_eq!(
            yaml(&snippets[0].content)["components"]["schemas"]["User"]["properties"]["id"],
            json!({ "type": "integer", "format": "int64" })
        );
    }

    #[test]
    fn routes_are_compiled_and_references_resolved() {
        let project = Project::new();
        project.file(
            "src/api.rs",
            r#"
            /// @openapi
            pub struct User { pub id: i64 }

            /// List users
            /// @route GET /users
            /// @return 200: Vec<User> "All users"
            pub fn list_users() {}
            "#,
        );

        let (snippets, _) = scan(&project);
        let route = snippets
            .iter()
            .find(|s| s.operation_id.as_deref() == Some("list_users"))
            .expect("route snippet");

        let op = &yaml(&route.content)["paths"]["/users"]["get"];
        assert_eq!(op["summary"], "List users");
        assert_eq!(
            op["responses"]["200"]["content"]["application/json"]["schema"],
            json!({ "type": "array", "items": { "$ref": "#/components/schemas/User" } })
        );
    }

    #[test]
    fn rejected_routes_keep_their_raw_dsl() {
        let project = Project::new();
        project.file("src/api.rs", "/// @route GET /items/{id}\nfn get_item() {}");

        let (snippets, _) = scan(&project);

        assert_eq!(snippets.len(), 1);
        assert!(snippets[0].content.contains("@route GET /items/{id}"));
    }

    #[test]
    fn fragments_and_blueprints_go_to_the_registry() {
        let project = Project::new();
        project.file(
            "src/lib.rs",
            r#"
            //! @openapi-fragment Common(code)
            //! description: Error {{code}}

            /// @openapi<T>
            /// properties:
            ///   data: { $ref: $T }
            pub struct Wrapper<T> { data: T }
            "#,
        );

        let (snippets, registry) = scan(&project);

        assert!(
            snippets.is_empty(),
            "templates are not emitted on their own"
        );
        assert_eq!(registry.fragments["Common"].params, ["code"]);
        assert_eq!(registry.blueprints["Wrapper"].params, ["T"]);
    }

    #[test]
    fn generic_usage_generates_concrete_schemas() {
        let project = Project::new();
        project.file(
            "src/lib.rs",
            r#"
            /// @openapi<T>
            /// type: object
            /// properties:
            ///   data:
            ///     $ref: $T
            struct Wrapper<T>(T);

            /// @openapi
            /// type: object
            struct User;

            /// @openapi
            /// paths:
            ///   /test:
            ///     get:
            ///       responses:
            ///         '200':
            ///           description: OK
            ///           content:
            ///             application/json:
            ///               schema:
            ///                 $ref: $Wrapper<User>
            fn handler() {}
            "#,
        );

        let (snippets, registry) = scan(&project);
        let docs = documents(&snippets);

        assert!(registry.concrete_schemas.contains_key("Wrapper_User"));
        assert_eq!(
            find(
                &docs,
                "/paths/~1test/get/responses/200/content/application~1json/schema"
            ),
            &json!({ "$ref": "#/components/schemas/Wrapper_User" })
        );
        assert_eq!(
            find(&docs, "/components/schemas/Wrapper_User/properties/data"),
            &json!({ "$ref": "#/components/schemas/User" })
        );
        let generated = snippets
            .iter()
            .find(|s| s.file_path == Path::new("<generated>"))
            .expect("generated snippet");
        assert_eq!(generated.line_number, 1);
    }

    #[test]
    fn fragments_are_expanded_in_routes_and_documents() {
        let project = Project::new();
        project.file(
            "src/lib.rs",
            r#"
            //! @openapi-fragment Paging
            //! - name: limit
            //!   in: query
            //!   schema: { type: integer }

            //! @openapi-fragment NotFound
            //! '404':
            //!   description: Not Found

            /// @route GET /list
            /// parameters:
            ///   @insert Paging
            fn list() {}

            /// @openapi
            /// paths:
            ///   /other:
            ///     get:
            ///       parameters:
            ///         @insert ExternalParam
            ///       responses:
            ///         @insert NotFound
            fn other() {}
            "#,
        );

        let (snippets, _) = scan(&project);
        let docs = documents(&snippets);

        assert_eq!(
            find(&docs, "/paths/~1list/get/parameters/0/name"),
            &json!("limit")
        );
        assert_eq!(
            find(&docs, "/paths/~1other/get/parameters"),
            &json!([{ "$ref": "#/components/parameters/ExternalParam" }])
        );
        assert_eq!(
            find(&docs, "/paths/~1other/get/responses/404/description"),
            &json!("Not Found")
        );
    }

    #[test]
    fn extend_with_quoted_arguments() {
        let project = Project::new();
        project.file(
            "src/lib.rs",
            r#"
            //! @openapi-fragment Secured(role)
            //! security:
            //!   - oidc: [{{role}}]

            /// @openapi
            /// paths:
            ///   /admin:
            ///     get:
            ///       @extend Secured("admin")
            ///       summary: Admin
            fn admin() {}
            "#,
        );

        let (snippets, _) = scan(&project);
        let docs = documents(&snippets);

        assert_eq!(
            find(&docs, "/paths/~1admin/get"),
            &json!({ "summary": "Admin", "security": [{ "oidc": ["admin"] }] })
        );
    }

    #[test]
    fn escapes_and_version_placeholder_are_finalized() {
        let project = Project::new();
        project.file(
            "src/lib.rs",
            "/// @openapi\n/// info:\n///   version: '{{CARGO_PKG_VERSION}}'\n///   description: costs \\$100\nfn root() {}",
        );

        let (snippets, _) = scan(&project);
        let info = &yaml(&snippets[0].content)["info"];

        let version = std::env::var("CARGO_PKG_VERSION").unwrap_or_else(|_| "0.0.0".into());
        assert_eq!(info["version"], json!(version));
        assert_eq!(info["description"], "costs $100");
    }

    mod static_files {
        use super::*;

        #[test]
        fn yaml_json_and_yml_files_are_included() {
            let project = Project::new();
            project
                .file("src/a.yaml", "paths:\n  /a: {}")
                .file("src/b.yml", "paths:\n  /b: {}")
                .file("src/c.json", r#"{"paths": {"/c": {}}}"#);

            let (snippets, _) = scan(&project);
            let docs = documents(&snippets);

            assert_eq!(docs.len(), 3);
            for path in ["/paths/~1a", "/paths/~1b", "/paths/~1c"] {
                find(&docs, path);
            }
            assert!(snippets.iter().all(|s| !s.content.starts_with("---")));
        }

        #[test]
        fn schema_names_enable_smart_references() {
            let project = Project::new();
            project
                .file(
                    "src/shared.yaml",
                    "components:\n  schemas:\n    Money: { type: string }",
                )
                .file(
                    "src/api.rs",
                    "/// @route GET /price\n/// @return 200: Money\nfn price() {}",
                );

            let (snippets, registry) = scan(&project);
            let docs = documents(&snippets);

            assert!(registry.schemas.contains_key("Money"));
            assert_eq!(
                find(
                    &docs,
                    "/paths/~1price/get/responses/200/content/application~1json/schema"
                ),
                &json!({ "$ref": "#/components/schemas/Money" })
            );
        }

        #[test]
        fn vendor_templates_hydrate_the_registry_and_are_stripped() {
            let project = Project::new();
            project.file(
                "shared.yaml",
                r#"
components:
  schemas:
    Shared: { type: object }
  x-oas-forge-templates:
    Page:
      params: [T]
      body: "items: $T"
  x-oas-forge-fragments:
    Err:
      params: [code]
      body: "description: {{code}}"
"#,
            );

            let (snippets, registry) =
                scan_directories(&[], &[project.path("shared.yaml")]).unwrap();

            assert_eq!(registry.blueprints["Page"].body, "items: $T");
            assert_eq!(registry.fragments["Err"].params, ["code"]);
            let doc = yaml(&snippets[0].content);
            assert_eq!(
                doc["components"],
                json!({ "schemas": { "Shared": { "type": "object" } } })
            );
        }

        #[test]
        fn malformed_vendor_extensions_are_ignored() {
            let project = Project::new();
            project.file(
                "src/shared.yaml",
                "components:\n  x-oas-forge-templates: [nope]\n  x-oas-forge-fragments: 5\n  schemas: [also-not-a-map]",
            );

            let (snippets, registry) = scan(&project);

            assert!(registry.blueprints.is_empty());
            assert!(registry.fragments.is_empty());
            assert_eq!(
                yaml(&snippets[0].content)["components"],
                json!({ "schemas": ["also-not-a-map"] })
            );
        }

        #[test]
        fn invalid_yaml_is_kept_verbatim() {
            let project = Project::new();
            project.file("src/broken.yaml", "a: : b");

            let (snippets, _) = scan(&project);

            assert_eq!(snippets[0].content, "a: : b");
            assert_eq!(snippets[0].line_number, 1);
        }

        #[test]
        fn missing_includes_are_skipped() {
            let project = Project::new();
            project.file("src/a.yaml", "a: 1");

            let (snippets, _) =
                scan_directories(&[project.path("src")], &[project.path("missing.yaml")]).unwrap();

            assert_eq!(snippets.len(), 1);
        }
    }
}
