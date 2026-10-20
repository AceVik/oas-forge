//! `src/generator.rs`: the builder, the pipeline run and its result.

use crate::common::{Project, ROOT_DOC, codes, pipeline, try_pipeline};
use oas_forge::config::Config;
use oas_forge::diagnostics::{Code, Severity};
use oas_forge::error::Error;
use oas_forge::output::OutputKind;
use oas_forge::{Generator, Spec};
use serde_json::{Value, json};

fn build(project: &Project) -> Spec {
    Generator::new()
        .input(project.path("src"))
        .build()
        .expect("build succeeds")
}

mod builder {
    use super::*;

    #[test]
    fn generate_needs_an_output() {
        let project = Project::new();
        project.file("src/root.rs", ROOT_DOC);
        let result = Generator::new().input(project.path("src")).generate();
        assert!(matches!(result, Err(Error::NoOutput)));
    }

    #[test]
    fn config_adds_inputs_and_every_output_kind() {
        let project = Project::new();
        project
            .file("src/root.rs", ROOT_DOC)
            .file("extra.yaml", "paths:\n  /extra: {}");
        let mut config = Config::default();
        config.input = Some(vec![project.path("src")]);
        config.include = Some(vec![project.path("extra.yaml")]);
        config.output = Some(vec![project.path("out/full.json")]);
        config.output_schemas = Some(vec![project.path("out/schemas.yaml")]);
        config.output_paths = Some(vec![project.path("out/paths.yaml")]);
        config.output_fragments = Some(vec![project.path("out/fragments.yaml")]);

        Generator::new().with_config(config).generate().unwrap();

        for file in ["full.json", "schemas.yaml", "paths.yaml", "fragments.yaml"] {
            assert!(project.path("out").join(file).is_file(), "{file}");
        }
        assert!(project.read_yaml("out/paths.yaml").get("/extra").is_some());
    }

    #[test]
    fn output_as_selects_the_strategy() {
        let project = Project::new();
        project
            .file("src/root.rs", ROOT_DOC)
            .file("src/a.rs", "/// @openapi\nstruct A { a: String }");
        Generator::new()
            .input(project.path("src"))
            .output_as(OutputKind::Schemas, project.path("schemas.json"))
            .generate()
            .unwrap();
        assert_eq!(
            project.read_yaml("schemas.json"),
            json!({ "A": { "type": "object", "properties": { "a": { "type": "string" } }, "required": ["a"] } })
        );
    }

    #[test]
    fn variables_fill_placeholders() {
        let project = Project::new();
        project.file(
            "src/lib.rs",
            "/// @openapi\n/// info:\n///   title: {{TITLE}}\n///   version: {{CARGO_PKG_VERSION}}\nfn root() {}",
        );
        let spec = Generator::new()
            .input(project.path("src"))
            .var("TITLE", "Named API")
            .var("CARGO_PKG_VERSION", "9.9.9")
            .build()
            .unwrap();
        assert_eq!(
            spec.document()["info"],
            json!({ "title": "Named API", "version": "9.9.9" })
        );
    }

    #[test]
    fn the_crate_version_is_predefined() {
        let (document, _) = pipeline(&[(
            "lib.rs",
            "/// @openapi\n/// info: { version: '{{CARGO_PKG_VERSION}}' }\nfn r() {}",
        )]);
        // `generate_from_sources` gets no variables, so the placeholder stays.
        assert_eq!(document["info"]["version"], "{{CARGO_PKG_VERSION}}");

        let project = Project::new();
        project.file(
            "src/lib.rs",
            "/// @openapi\n/// info: { version: {{CARGO_PKG_VERSION}} }\nfn r() {}",
        );
        let expected = std::env::var("CARGO_PKG_VERSION").unwrap_or_else(|_| "0.0.0".into());
        assert_eq!(
            build(&project).document()["info"]["version"],
            json!(expected)
        );
    }

    #[test]
    fn deny_warnings_turns_warnings_into_errors() {
        let project = Project::new();
        project
            .file("src/root.rs", ROOT_DOC)
            .file("src/a.rs", "/// @route GET /x\n/// @frobnicate\nfn x() {}");
        let generator = Generator::new()
            .input(project.path("src"))
            .output(project.path("api.yaml"));
        generator.generate().expect("warnings alone do not fail");

        let strict = generator.clone().deny_warnings(true);
        let spec = strict.build().unwrap();
        assert!(spec.diagnostics().has_errors());
        match strict.generate() {
            Err(Error::Diagnostics(found)) => {
                assert_eq!(found.len(), 1);
                assert_eq!(found[0].code(), Code::UnknownDirective);
                assert_eq!(found[0].severity(), Severity::Error);
            }
            other => panic!("expected diagnostics error, got {other:?}"),
        }
    }

    #[test]
    fn error_diagnostics_fail_generate_but_not_build() {
        let project = Project::new();
        project
            .file("src/root.rs", ROOT_DOC)
            .file("src/a.rs", "/// @route GET /items/{id}\nfn x() {}");
        let generator = Generator::new()
            .input(project.path("src"))
            .output(project.path("api.yaml"));

        let spec = generator.build().unwrap();
        assert_eq!(codes(spec.diagnostics()), [Code::UndeclaredPathParam]);
        assert!(matches!(generator.generate(), Err(Error::Diagnostics(_))));
        assert!(!project.path("api.yaml").exists(), "nothing is written");
    }

    #[test]
    fn max_file_size_limits_inputs() {
        let project = Project::new();
        project.file("src/root.rs", ROOT_DOC);
        let spec = Generator::new()
            .input(project.path("src"))
            .max_file_size(10)
            .build()
            .unwrap();
        assert_eq!(codes(spec.diagnostics()), [Code::FileTooLarge]);
        assert!(spec.document().as_object().unwrap().is_empty());
    }

    #[test]
    fn outputs_are_not_read_back_as_inputs() {
        let project = Project::new();
        project.file("src/root.rs", ROOT_DOC);
        let generator = Generator::new()
            .input(project.path("src"))
            .output(project.path("src/openapi.yaml"));
        generator.generate().unwrap();
        // A second run would see two roots if the output were read back.
        let spec = generator.build().unwrap();
        assert_eq!(codes(spec.diagnostics()), [Code::OutputNotRead]);
        generator.generate().unwrap();
    }

    #[test]
    fn cargo_directives_can_be_printed() {
        let project = Project::new();
        project
            .file("src/root.rs", ROOT_DOC)
            .file("src/a.rs", "/// @route GET /x\n/// @frobnicate\nfn x() {}");
        Generator::new()
            .input(project.path("src"))
            .output(project.path("api.yaml"))
            .emit_cargo_directives(true)
            .generate()
            .unwrap();
    }
}

mod spec {
    use super::*;

    #[test]
    fn exposes_document_registry_diagnostics_and_watched_inputs() {
        let project = Project::new();
        project
            .file("src/root.rs", ROOT_DOC)
            .file("src/t.rs", "/// @openapi-fragment F\n/// a: 1\nfn f() {}")
            .file("base.yaml", "paths: {}");
        let spec = Generator::new()
            .input(project.path("src"))
            .include(project.path("base.yaml"))
            .include(project.path("missing.yaml"))
            .build()
            .unwrap();
        assert_eq!(spec.document()["info"]["title"], "Test API");
        assert!(spec.registry().fragment("F").is_some());
        assert_eq!(codes(spec.diagnostics()), [Code::MissingInclude]);
        assert_eq!(
            spec.watched(),
            [project.path("src"), project.path("base.yaml")]
        );
    }
}

mod failures {
    use super::*;

    #[test]
    fn empty_input_directory() {
        let project = Project::new();
        project.dir("src");
        assert!(matches!(
            Generator::new().input(project.path("src")).build(),
            Err(Error::NoFilesFound)
        ));
    }

    #[test]
    fn missing_input_directory() {
        let project = Project::new();
        assert!(matches!(
            Generator::new().input(project.path("nope")).build(),
            Err(Error::Io { .. })
        ));
    }

    #[test]
    fn invalid_rust() {
        let result = try_pipeline(&[("lib.rs", "fn {")]);
        assert!(matches!(result, Err(Error::Parse { .. })), "{result:?}");
    }

    #[test]
    fn invalid_yaml_names_its_location() {
        let result = try_pipeline(&[(
            "lib.rs",
            "\n/// @openapi\n/// paths:\n///   a: : b\nfn f() {}",
        )]);
        match result {
            Err(Error::Yaml {
                location, context, ..
            }) => {
                assert_eq!(location.to_string(), "lib.rs:4", "the offending line");
                assert!(context.contains("  03 | paths:"), "{context}");
                assert!(context.contains("> 04 |   a: : b"), "{context}");
            }
            other => panic!("expected a YAML error, got {other:?}"),
        }
        assert!(matches!(
            try_pipeline(&[("broken.yaml", "a: : b")]),
            Err(Error::Yaml { .. })
        ));
    }

    #[test]
    fn snippets_must_be_mappings() {
        // Regression: non-mapping snippets were silently dropped by the merger.
        for (text, found) in [
            ("- a", "a sequence"),
            ("text", "a string"),
            ("5", "a number"),
            ("true", "a boolean"),
        ] {
            let result =
                try_pipeline(&[("lib.rs", &format!("/// @openapi\n/// {text}\nfn f() {{}}"))]);
            match result {
                Err(Error::Yaml { message, .. }) => assert!(message.ends_with(found), "{message}"),
                other => panic!("expected a YAML error for {text:?}, got {other:?}"),
            }
        }
    }

    #[test]
    fn multiple_roots_name_every_location() {
        let result = try_pipeline(&[
            ("a.rs", ROOT_DOC),
            ("b.yaml", "openapi: 3.1.0\ninfo: { title: B, version: '1' }"),
        ]);
        match result {
            Err(Error::MultipleRootsFound { locations }) => {
                let locations: Vec<_> = locations.iter().map(ToString::to_string).collect();
                assert_eq!(locations, ["a.rs:3", "b.yaml:1"]);
            }
            other => panic!("expected multiple roots, got {other:?}"),
        }
    }
}

mod pipeline_stages {
    use super::*;

    #[test]
    fn schemas_and_routes_are_merged_with_resolved_references() {
        let (document, diagnostics) = pipeline(&[(
            "api.rs",
            r#"
            /// @openapi
            pub struct User { pub id: i64 }

            /// List users
            /// @route GET /users
            /// @return 200: Vec<User> "All users"
            pub fn list_users() {}
            "#,
        )]);
        assert!(diagnostics.is_empty(), "{diagnostics:?}");
        assert_eq!(
            document["components"]["schemas"]["User"]["properties"]["id"],
            json!({ "type": "integer", "format": "int64" })
        );
        let operation = &document["paths"]["/users"]["get"];
        assert_eq!(operation["summary"], "List users");
        assert_eq!(
            operation["responses"]["200"]["content"]["application/json"]["schema"],
            json!({ "type": "array", "items": { "$ref": "#/components/schemas/User" } })
        );
    }

    #[test]
    fn rejected_routes_are_errors_and_leave_no_trace() {
        // Changed in 0.2: the raw DSL text is no longer emitted as YAML.
        let (document, diagnostics) =
            pipeline(&[("api.rs", "/// @route GET /items/{id}\nfn get_item() {}")]);
        assert!(document.get("paths").is_none());
        assert_eq!(codes(&diagnostics), [Code::UndeclaredPathParam]);
    }

    #[test]
    fn templates_are_not_emitted_on_their_own() {
        let (document, _) = pipeline(&[(
            "lib.rs",
            r#"
            //! @openapi-fragment Common(code)
            //! description: Error {{code}}

            /// @openapi<T>
            /// properties:
            ///   data: { $ref: $T }
            pub struct Wrapper<T> { data: T }
            "#,
        )]);
        assert_eq!(document, json!({}));
    }

    #[test]
    fn generic_usage_generates_concrete_schemas() {
        let (document, diagnostics) = pipeline(&[(
            "lib.rs",
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
        )]);
        assert!(diagnostics.is_empty(), "{diagnostics:?}");
        assert_eq!(
            document.pointer("/paths/~1test/get/responses/200/content/application~1json/schema"),
            Some(&json!({ "$ref": "#/components/schemas/Wrapper_User" }))
        );
        assert_eq!(
            document.pointer("/components/schemas/Wrapper_User/properties/data"),
            Some(&json!({ "$ref": "#/components/schemas/User" }))
        );
    }

    #[test]
    fn fragments_are_expanded_in_routes_and_documents() {
        let (document, _) = pipeline(&[(
            "lib.rs",
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
        )]);
        assert_eq!(
            document.pointer("/paths/~1list/get/parameters/0/name"),
            Some(&json!("limit"))
        );
        assert_eq!(
            document.pointer("/paths/~1other/get/parameters"),
            Some(&json!([{ "$ref": "#/components/parameters/ExternalParam" }]))
        );
        assert_eq!(
            document.pointer("/paths/~1other/get/responses/404/description"),
            Some(&json!("Not Found"))
        );
    }

    #[test]
    fn fragments_defined_later_in_the_file_are_found() {
        let (document, diagnostics) = pipeline(&[
            (
                "a.rs",
                "/// @openapi\n/// paths:\n///   /a:\n///     @insert Item\nfn a() {}",
            ),
            (
                "b.rs",
                "/// @openapi-fragment Item\n/// get: { summary: From b }\nfn b() {}",
            ),
        ]);
        assert!(diagnostics.is_empty(), "{diagnostics:?}");
        assert_eq!(document["paths"]["/a"]["get"]["summary"], "From b");
    }

    #[test]
    fn extend_merges_fragments_and_local_values_win() {
        let (document, _) = pipeline(&[(
            "lib.rs",
            r#"
            //! @openapi-fragment Secured(role)
            //! security:
            //!   - oidc: [{{role}}]
            //! summary: From fragment

            /// @openapi
            /// paths:
            ///   /admin:
            ///     get:
            ///       @extend Secured("admin")
            ///       summary: Admin
            fn admin() {}

            /// @route GET /routed
            /// @extend Secured("user")
            fn routed() {}
            "#,
        )]);
        assert_eq!(
            document["paths"]["/admin"]["get"],
            json!({ "summary": "Admin", "security": [{ "oidc": ["admin"] }] })
        );
        assert_eq!(
            document["paths"]["/routed"]["get"]["security"],
            json!([{ "oidc": ["user"] }])
        );
        assert!(
            !serde_json::to_string(&document)
                .unwrap()
                .contains("x-oas-forge-extend")
        );
    }

    #[test]
    fn escaped_dollars_survive_reference_resolution() {
        let (document, _) = pipeline(&[(
            "lib.rs",
            "/// @openapi\n/// info:\n///   description: costs \\$100 for \\$User\n/// x-ref: \\$User\nfn root() {}\n/// @openapi\nstruct User;",
        )]);
        assert_eq!(document["info"]["description"], "costs $100 for $User");
        assert_eq!(document["x-ref"], "$User");
    }

    #[test]
    fn named_schemas_are_parsed_as_any_yaml_value() {
        let (document, _) = pipeline(&[(
            "lib.rs",
            "//! @openapi-type Flag\n//! true\n//! @openapi-type Empty\n//! ~",
        )]);
        assert_eq!(document["components"]["schemas"], json!({ "Flag": true }));
    }

    #[test]
    fn duplicate_schemas_are_merged_and_reported() {
        let (document, diagnostics) = pipeline(&[
            ("a.rs", "/// @openapi\nstruct User { id: u64 }"),
            ("b.rs", "/// @openapi\nstruct User { name: String }"),
        ]);
        let user = &document["components"]["schemas"]["User"];
        assert_eq!(user["properties"].as_object().unwrap().len(), 2);
        assert_eq!(user["required"], json!(["id", "name"]));
        assert_eq!(codes(&diagnostics), [Code::DuplicateSchema]);
        assert_eq!(
            diagnostics
                .iter()
                .next()
                .unwrap()
                .location()
                .unwrap()
                .to_string(),
            "b.rs:1"
        );
    }

    #[test]
    fn conflicting_values_are_reported_with_the_later_location() {
        let (document, diagnostics) = pipeline(&[
            ("a.yaml", "paths: { /a: { summary: A } }"),
            ("b.yaml", "paths: { /a: { summary: B } }"),
        ]);
        assert_eq!(document["paths"]["/a"]["summary"], "B");
        let conflict = diagnostics.iter().next().unwrap();
        assert_eq!(conflict.code(), Code::MergeConflict);
        assert_eq!(conflict.location().unwrap().to_string(), "b.yaml:1");
    }

    #[test]
    fn the_root_is_the_base_of_the_merge() {
        let (document, _) = pipeline(&[
            ("a.yaml", "paths: { /a: {} }\nx-first: 1"),
            (
                "root.yaml",
                "openapi: 3.1.0\ninfo: { title: T, version: '1' }\nx-root: 1",
            ),
        ]);
        let keys: Vec<_> = document.as_object().unwrap().keys().cloned().collect();
        assert_eq!(keys, ["openapi", "info", "x-root", "paths", "x-first"]);
    }

    #[test]
    fn top_level_tags_become_unique_objects() {
        let (document, _) = pipeline(&[
            (
                "a.yaml",
                "tags: [Users, { name: Admin, description: Staff }]",
            ),
            ("b.yaml", "tags: [Users, Admin]"),
        ]);
        assert_eq!(
            document["tags"],
            json!([{ "name": "Users" }, { "name": "Admin", "description": "Staff" }])
        );
    }

    #[test]
    fn lints_report_dangling_references_and_duplicate_operation_ids() {
        let (_, diagnostics) = pipeline(&[(
            "lib.rs",
            r##"
            /// @route GET /a
            /// @return 200: $Missing
            fn same() {}

            /// @openapi
            /// paths:
            ///   /b:
            ///     get: { operationId: same, responses: {} }
            ///     x-ignored: { $ref: "#/components/schemas/Ignored" }
            /// x-oas-forge-note: { $ref: "#/components/schemas/Internal" }
            /// x-external: { $ref: "other.yaml#/components/schemas/X" }
            /// x-encoded: { $ref: "#/components/schemas/%7BId%7D" }
            /// components:
            ///   schemas:
            ///     "{Id}": { type: string }
            fn docs() {}
            "##,
        )]);
        assert_eq!(
            codes(&diagnostics),
            [
                Code::DanglingReference,
                Code::DanglingReference,
                Code::DuplicateOperationId
            ]
        );
        let messages: Vec<_> = diagnostics
            .iter()
            .map(|d| d.message().to_string())
            .collect();
        assert!(
            messages[0].contains("Missing") && messages[1].contains("Ignored"),
            "{messages:?}"
        );
    }
}

mod static_files {
    use super::*;

    #[test]
    fn yaml_yml_and_json_files_are_included() {
        let (document, _) = pipeline(&[
            ("a.yaml", "paths:\n  /a: {}"),
            ("b.yml", "paths:\n  /b: {}"),
            ("c.json", r#"{"paths": {"/c": {}}}"#),
            ("empty.yaml", ""),
        ]);
        let paths: Vec<_> = document["paths"]
            .as_object()
            .unwrap()
            .keys()
            .cloned()
            .collect();
        assert_eq!(paths, ["/a", "/b", "/c"]);
    }

    #[test]
    fn schema_names_enable_smart_references() {
        let (document, diagnostics) = pipeline(&[
            (
                "shared.yaml",
                "components:\n  schemas:\n    Money: { type: string }",
            ),
            (
                "api.rs",
                "/// @route GET /price\n/// @return 200: Money\nfn price() {}",
            ),
        ]);
        assert!(diagnostics.is_empty(), "{diagnostics:?}");
        assert_eq!(
            document.pointer("/paths/~1price/get/responses/200/content/application~1json/schema"),
            Some(&json!({ "$ref": "#/components/schemas/Money" }))
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
  x-oas-forge-format: 1
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
        project.file(
            "src/api.rs",
            "/// @route GET /x\n/// @return 200: $Page<Shared>\n/// responses:\n///   '500':\n///     @insert Err(\"Boom\")\nfn x() {}",
        );
        let spec = Generator::new()
            .input(project.path("src"))
            .include(project.path("shared.yaml"))
            .build()
            .unwrap();
        assert!(spec.diagnostics().is_empty(), "{:?}", spec.diagnostics());
        let document = spec.document();
        assert_eq!(
            document["components"]["schemas"],
            json!({ "Shared": { "type": "object" }, "Page_Shared": { "items": "#/components/schemas/Shared" } })
        );
        assert_eq!(
            document["paths"]["/x"]["get"]["responses"]["500"]["description"],
            "Boom"
        );
        assert!(spec.registry().blueprint("Page").is_some());
    }

    #[test]
    fn malformed_vendor_extensions_are_ignored() {
        let (document, _) = pipeline(&[(
            "shared.yaml",
            "components:\n  x-oas-forge-templates: [nope]\n  x-oas-forge-fragments: 5\n  schemas: [also-not-a-map]",
        )]);
        assert_eq!(
            document["components"],
            json!({ "schemas": ["also-not-a-map"] })
        );
    }

    #[test]
    fn included_files_are_documents_not_roots_of_their_own_registry() {
        let (document, _): (Value, _) =
            pipeline(&[("only.yaml", "components:\n  x-oas-forge-format: 1")]);
        assert_eq!(document, json!({}));
    }
}

mod types_and_dialects {
    use super::*;

    #[test]
    fn pattern_constants_are_resolved_across_files() {
        let (document, diagnostics) = pipeline(&[
            (
                "a.rs",
                "/// @openapi\nstruct S {\n  #[validate(regex(path = *SLUG))]\n  slug: String,\n  #[garde(pattern(MISSING))]\n  other: String,\n  #[validate(regex(path = *SLUG))]\n  #[validate(contains(pattern = \"-\"))]\n  both: String,\n}",
            ),
            (
                "b.rs",
                "static SLUG: LazyLock<Regex> = LazyLock::new(|| Regex::new(\"^[a-z-]+$\").unwrap());",
            ),
        ]);
        let properties = &document["components"]["schemas"]["S"]["properties"];
        assert_eq!(
            properties["slug"],
            json!({ "type": "string", "pattern": "^[a-z-]+$" })
        );
        assert_eq!(properties["other"], json!({ "type": "string" }));
        assert_eq!(
            properties["both"],
            json!({ "type": "string", "pattern": "-", "allOf": [{ "pattern": "^[a-z-]+$" }] })
        );
        assert_eq!(codes(&diagnostics), [Code::UnresolvedPattern]);
        assert!(
            diagnostics
                .iter()
                .next()
                .unwrap()
                .message()
                .contains("MISSING")
        );
    }

    #[test]
    fn patterns_in_generic_instances_are_resolved() {
        let (document, diagnostics) = pipeline(&[(
            "a.rs",
            "static ID: LazyLock<Regex> = LazyLock::new(|| Regex::new(\"^i$\").unwrap());\n/// @openapi<T>\nstruct Page<T> {\n  #[validate(regex(path = *ID))]\n  cursor: String,\n  items: Vec<T>,\n}\n/// @openapi\n/// paths: { /p: { get: { responses: { '200': { description: x, content: { a/b: { schema: { $ref: $Page<String> } } } } } } } }\nfn f() {}",
        )]);
        assert!(diagnostics.is_empty(), "{diagnostics:?}");
        assert_eq!(
            document["components"]["schemas"]["Page_String"]["properties"]["cursor"]["pattern"],
            "^i$"
        );
    }

    #[test]
    fn mapped_types_are_used_by_structs_aliases_and_routes() {
        let project = Project::new();
        project.file("src/root.rs", ROOT_DOC).file(
            "src/api.rs",
            "/// @openapi\nstruct User { id: UserId, tags: Vec<geo::Point> }\ntype Owner = UserId;\n/// @route GET /users/{id: UserId}\n/// @return 200: UserId\nfn get() {}",
        );
        let spec = Generator::new()
            .input(project.path("src"))
            .map_type("UserId", json!({ "type": "string", "format": "uuid" }))
            .map_type(
                "geo::Point",
                json!({ "type": "array", "items": { "type": "number" } }),
            )
            .build()
            .unwrap();
        let document = spec.document();
        let uuid = json!({ "type": "string", "format": "uuid" });
        assert_eq!(
            document["components"]["schemas"]["User"]["properties"]["id"],
            uuid
        );
        assert_eq!(document["components"]["schemas"]["Owner"], uuid);
        assert_eq!(
            document["components"]["schemas"]["User"]["properties"]["tags"]["items"]["type"],
            "array"
        );
        let operation = &document["paths"]["/users/{id}"]["get"];
        assert_eq!(operation["parameters"][0]["schema"], uuid);
        assert_eq!(
            operation["responses"]["200"]["content"]["application/json"]["schema"],
            uuid
        );
        assert!(spec.diagnostics().is_empty(), "{:?}", spec.diagnostics());
    }

    #[test]
    fn openapi_30_roots_get_30_schemas() {
        let (document, diagnostics) = pipeline(&[
            (
                "root.yaml",
                "openapi: 3.0.3\ninfo: { title: T, version: '1' }",
            ),
            (
                "a.rs",
                "/// @openapi\nstruct S { a: Option<String>, b: Option<Other>, t: (u8, u8) }\n/// @openapi\nstruct Other { x: u8 }",
            ),
        ]);
        let properties = &document["components"]["schemas"]["S"]["properties"];
        assert_eq!(
            properties["a"],
            json!({ "type": "string", "nullable": true })
        );
        assert_eq!(
            properties["b"],
            json!({ "allOf": [{ "$ref": "#/components/schemas/Other" }], "nullable": true })
        );
        assert!(properties["t"].get("prefixItems").is_none());
        assert_eq!(codes(&diagnostics), [Code::LossyLowering]);
    }

    #[test]
    fn openapi_31_roots_keep_2020_12_schemas() {
        let (document, _) = pipeline(&[
            (
                "root.yaml",
                "openapi: 3.1.0\ninfo: { title: T, version: '1' }",
            ),
            ("a.rs", "/// @openapi\nstruct S { a: Option<String> }"),
        ]);
        assert_eq!(
            document["components"]["schemas"]["S"]["properties"]["a"],
            json!({ "type": ["string", "null"] })
        );
    }
}

mod operations {
    use super::*;

    const MODELS: &str = "/// @openapi\nstruct Paging {\n  /// Page.\n  page: Option<u32>,\n  size: u8,\n}\n/// @openapi\nstruct Ids { org: String, id: u64 }\n/// @openapi\nstruct UserId(u64);";

    #[test]
    fn struct_expansions_become_parameters() {
        let (document, diagnostics) = pipeline(&[
            ("models.rs", MODELS),
            (
                "api.rs",
                "/// @route GET /orgs/{org}/users/{id}\n/// @path-params $Ids\n/// @query-params $Paging\n/// @param query size: u8 \"Explicit\"\nfn get() {}",
            ),
        ]);
        assert!(diagnostics.is_empty(), "{diagnostics:?}");
        let parameters = &document["paths"]["/orgs/{org}/users/{id}"]["get"]["parameters"];
        let names: Vec<_> = parameters
            .as_array()
            .unwrap()
            .iter()
            .map(|p| {
                format!(
                    "{}:{}:{}",
                    p["in"].as_str().unwrap(),
                    p["name"].as_str().unwrap(),
                    p["required"]
                )
            })
            .collect();
        assert_eq!(
            names,
            [
                "path:org:true",
                "path:id:true",
                "query:page:false",
                "query:size:true"
            ]
        );
        assert_eq!(
            parameters[3]["description"], "Explicit",
            "explicit parameters win"
        );
        assert_eq!(
            parameters[2],
            json!({
                "name": "page", "in": "query", "required": false, "description": "Page.",
                "schema": { "type": "integer", "format": "int64", "minimum": 0, "maximum": 4294967295u32 },
            }),
            "parameters are not nullable"
        );
    }

    #[test]
    fn unknown_structs_and_missing_placeholders_are_reported() {
        let (document, diagnostics) = pipeline(&[
            ("models.rs", MODELS),
            (
                "api.rs",
                "/// @route GET /a/{x}\n/// @path-params $Ids\n/// @query-params $Nope\nfn a() {}",
            ),
        ]);
        assert_eq!(
            codes(&diagnostics),
            [
                Code::UnresolvedReference,
                Code::UnresolvedExpansion,
                // `{x}` has no parameter; `org` and `id` are not in the path.
                Code::PathParameterMismatch,
                Code::PathParameterMismatch,
                Code::PathParameterMismatch,
            ]
        );
        let operation = &document["paths"]["/a/{x}"]["get"];
        assert!(
            !serde_json::to_string(operation)
                .unwrap()
                .contains("x-oas-forge-expand")
        );
    }

    #[test]
    fn inferred_path_newtypes_fall_back_to_a_single_parameter() {
        let (document, diagnostics) = pipeline(&[
            ("models.rs", MODELS),
            (
                "api.rs",
                "/// @route GET /users/{user}\nasync fn get(Path(id): Path<UserId>) {}",
            ),
        ]);
        assert!(diagnostics.is_empty(), "{diagnostics:?}");
        assert_eq!(
            document["paths"]["/users/{user}"]["get"]["parameters"],
            json!([{ "name": "user", "in": "path", "required": true, "schema": { "$ref": "#/components/schemas/UserId" } }])
        );
    }

    #[test]
    fn signature_mismatches_are_reported() {
        let (_, diagnostics) = pipeline(&[(
            "api.rs",
            "/// @route GET /a/{x}/{y}\nasync fn a(Path(x): Path<(u8, u8, u8)>) {}",
        )]);
        assert_eq!(
            codes(&diagnostics),
            [
                Code::SignatureMismatch,
                Code::UndeclaredPathParam,
                Code::UndeclaredPathParam
            ]
        );
    }

    #[test]
    fn empty_and_free_form_structs_expand_cleanly() {
        let (document, diagnostics) = pipeline(&[(
            "api.rs",
            "/// @openapi\nstruct Empty {}\n/// @openapi-type Loose\n/// type: object\n/// properties: { any: true }\nfn loose() {}\n/// @route GET /a\n/// @query-params $Empty\nfn a() {}\n/// @route GET /b\n/// @query-params $Loose\nfn b() {}",
        )]);
        assert!(diagnostics.is_empty(), "{diagnostics:?}");
        assert!(document["paths"]["/a"]["get"].get("parameters").is_none());
        assert_eq!(
            document["paths"]["/b"]["get"]["parameters"],
            json!([{ "name": "any", "in": "query", "required": false, "schema": true }])
        );
    }

    #[test]
    fn inferred_parameters_replace_a_malformed_list() {
        let (document, _) = pipeline(&[
            ("models.rs", MODELS),
            (
                "api.rs",
                "/// @route GET /a\n/// parameters: oops\nasync fn a(Query(p): Query<Paging>) {}",
            ),
        ]);
        let parameters = document["paths"]["/a"]["get"]["parameters"]
            .as_array()
            .unwrap()
            .clone();
        assert_eq!(parameters.len(), 2, "{parameters:?}");
    }

    #[test]
    fn route_defaults_apply_operation_fields() {
        let (document, diagnostics) = pipeline(&[(
            "api.rs",
            "/// @route-defaults\n/// @deprecated\n/// @external-docs https://example.com \"Docs\"\nmod old {\n  /// @route GET /a\n  fn a() {}\n  /// @route GET /b\n  /// @external-docs https://example.com/b\n  fn b() {}\n}",
        )]);
        assert!(diagnostics.is_empty(), "{diagnostics:?}");
        let a = &document["paths"]["/a"]["get"];
        assert_eq!(a["deprecated"], true);
        assert_eq!(
            a["externalDocs"],
            json!({ "url": "https://example.com", "description": "Docs" })
        );
        let b = &document["paths"]["/b"]["get"];
        assert_eq!(b["deprecated"], true);
        assert_eq!(
            b["externalDocs"],
            json!({ "url": "https://example.com/b" }),
            "the route wins"
        );
    }

    #[test]
    fn query_parameters_in_yaml_stop_query_inference() {
        let (document, diagnostics) = pipeline(&[
            ("models.rs", MODELS),
            (
                "api.rs",
                "/// @route GET /a\n/// parameters:\n///   - { name: q, in: query, schema: { type: string } }\nasync fn a(Query(p): Query<Paging>) {}\n/// @route GET /b\n/// parameters:\n///   - { name: X-Trace, in: header, schema: { type: string } }\nasync fn b(Query(p): Query<Paging>) {}",
            ),
        ]);
        assert!(diagnostics.is_empty(), "{diagnostics:?}");
        let names = |path: &str| -> Vec<String> {
            document["paths"][path]["get"]["parameters"]
                .as_array()
                .unwrap()
                .iter()
                .map(|p| p["name"].as_str().unwrap().to_string())
                .collect()
        };
        assert_eq!(names("/a"), ["q"]);
        assert_eq!(names("/b"), ["X-Trace", "page", "size"]);
    }

    #[test]
    fn inference_can_be_switched_off() {
        let project = Project::new();
        project
            .file("src/root.rs", ROOT_DOC)
            .file("src/api.rs", "/// @openapi\nstruct User { id: u8 }\n/// @route GET /me\nasync fn me() -> Json<User> { todo!() }");
        let build = |infer: bool| {
            Generator::new()
                .input(project.path("src"))
                .infer_signatures(infer)
                .build()
                .unwrap()
                .document()["paths"]["/me"]["get"]["responses"]
                .clone()
        };
        assert!(build(true).get("200").is_some());
        assert_eq!(
            build(false),
            json!({ "default": { "description": "Default response" } })
        );
    }

    #[test]
    fn module_tags_reach_every_kind_of_operation() {
        let (document, _) = pipeline(&[(
            "api.rs",
            "/// tags: [M]\nmod m {\n  /// @openapi\n  /// paths:\n  ///   /a:\n  ///     query: { responses: {} }\n  ///     additionalOperations: { PURGE: { responses: {} } }\n  /// webhooks:\n  ///   hook: { post: { responses: {} } }\n  fn a() {}\n}",
        )]);
        assert_eq!(document["paths"]["/a"]["query"]["tags"], json!(["M"]));
        assert_eq!(
            document["paths"]["/a"]["additionalOperations"]["PURGE"]["tags"],
            json!(["M"])
        );
        assert_eq!(document["webhooks"]["hook"]["post"]["tags"], json!(["M"]));
    }

    #[test]
    fn duplicate_operation_ids_are_found_everywhere() {
        let (_, diagnostics) = pipeline(&[(
            "api.rs",
            "/// @route PURGE /a\n/// @operation-id same\nfn a() {}\n/// @webhook hook\n/// @operation-id same\nfn b() {}",
        )]);
        assert_eq!(codes(&diagnostics), [Code::DuplicateOperationId]);
    }

    #[test]
    fn openapi_30_roots_drop_webhooks_and_32_features() {
        let (document, diagnostics) = pipeline(&[
            (
                "root.yaml",
                "openapi: 3.0.3\ninfo: { title: T, version: '1' }",
            ),
            (
                "api.rs",
                "/// @webhook hook\nfn a() {}\n/// @route GET /s\n/// @param querystring q: String\n/// @return 200: stream<String>\nfn b() {}",
            ),
        ]);
        assert!(document.get("webhooks").is_none());
        let operation = &document["paths"]["/s"]["get"];
        assert!(operation.get("parameters").is_none() || operation["parameters"] == json!([]));
        assert_eq!(
            operation["responses"]["200"]["content"]["text/event-stream"],
            json!({})
        );
        assert_eq!(codes(&diagnostics), [Code::UnsupportedFeature; 3]);
    }
}

mod auto_generics {
    use super::*;

    const PAGE: &str = r#"
        /// A page.
        /// @openapi
        struct Page<T> { items: Vec<T> }

        mod models {
            /// @openapi
            pub struct User { name: String }
        }
    "#;

    #[test]
    fn generic_structs_are_instantiated_where_they_are_used() {
        let (document, diagnostics) = pipeline(&[(
            "lib.rs",
            &format!(
                "{PAGE}\n/// @openapi\nstruct Holder {{\n  numbers: Page<u64>,\n  lists: Page<Vec<models::User>>,\n  optional: Page<Option<models::User>>,\n  nested: Page<Page<models::User>>,\n}}"
            ),
        )]);
        assert!(diagnostics.is_empty(), "{diagnostics:?}");
        let schemas = document["components"]["schemas"].as_object().unwrap();
        let mut names: Vec<_> = schemas.keys().collect();
        names.sort();
        assert_eq!(
            names,
            [
                "Holder",
                "Page_Option_User",
                "Page_Page_User",
                "Page_User",
                "Page_Vec_User",
                "Page_u64",
                "User"
            ]
        );
        assert_eq!(
            schemas["Holder"]["properties"]["lists"],
            json!({ "$ref": "#/components/schemas/Page_Vec_User" })
        );
        assert_eq!(
            schemas["Page_u64"],
            json!({
                "type": "object",
                "properties": {
                    "items": {
                        "type": "array",
                        "items": { "type": "integer", "format": "int64", "minimum": 0 }
                    }
                },
                "required": ["items"],
                "description": "A page."
            })
        );
        assert_eq!(
            schemas["Page_Option_User"]["properties"]["items"]["items"],
            json!({ "anyOf": [{ "$ref": "#/components/schemas/User" }, { "type": "null" }] })
        );
        assert_eq!(
            schemas["Page_Page_User"]["properties"]["items"]["items"],
            json!({ "$ref": "#/components/schemas/Page_User" })
        );
    }

    #[test]
    fn unused_generic_structs_are_not_emitted() {
        let (document, diagnostics) = pipeline(&[("lib.rs", PAGE)]);
        assert!(diagnostics.is_empty(), "{diagnostics:?}");
        let schemas = document["components"]["schemas"].as_object().unwrap();
        assert_eq!(schemas.keys().collect::<Vec<_>>(), ["User"]);
    }

    #[test]
    fn generic_enums_are_instantiated_too() {
        let (document, diagnostics) = pipeline(&[(
            "lib.rs",
            "/// @openapi\nenum Either<L, R> { Left(L), Right(R) }\n/// @openapi\nstruct S { e: Either<String, u8> }",
        )]);
        assert!(diagnostics.is_empty(), "{diagnostics:?}");
        assert_eq!(
            document["components"]["schemas"]["Either_String_u8"]["oneOf"][0]["properties"]["Left"],
            json!({ "type": "string" })
        );
    }

    #[test]
    fn routes_and_inferred_signatures_share_the_instances() {
        let (document, diagnostics) = pipeline(&[(
            "lib.rs",
            &format!(
                "{PAGE}\n/// @route GET /a\n/// @return 200: $Page<User>\nfn a() {{}}\n/// @route GET /b\nfn b() -> Json<Page<models::User>> {{ todo!() }}"
            ),
        )]);
        assert!(diagnostics.is_empty(), "{diagnostics:?}");
        let schema = |path: &str| {
            document["paths"][path]["get"]["responses"]["200"]["content"]["application/json"]["schema"].clone()
        };
        assert_eq!(
            schema("/a"),
            json!({ "$ref": "#/components/schemas/Page_User" })
        );
        assert_eq!(schema("/a"), schema("/b"));
        assert_eq!(
            document["components"]["schemas"].as_object().unwrap().len(),
            2,
            "Page_User and User"
        );
    }
}

mod package_root {
    use super::*;
    use oas_forge::Package;

    fn package() -> Package {
        let mut package = Package::new("shop", "2.1.0");
        package.description = Some("A shop".into());
        package
    }

    #[test]
    fn a_missing_root_is_generated_from_the_package() {
        let project = Project::new();
        project.file("src/lib.rs", "/// @route GET /a\nfn a() {}");
        let spec = Generator::new()
            .input(project.path("src"))
            .package(package())
            .build()
            .unwrap();
        assert_eq!(spec.document()["openapi"], oas_forge::package::ROOT_VERSION);
        assert_eq!(
            spec.document()["info"],
            json!({ "title": "shop", "version": "2.1.0", "description": "A shop" })
        );
        assert!(spec.document()["paths"]["/a"]["get"].is_object());
        let notes: Vec<_> = spec.diagnostics().iter().collect();
        assert_eq!(notes.len(), 1);
        assert_eq!(notes[0].code(), Code::GeneratedRoot);
        assert_eq!(notes[0].severity(), Severity::Note);
        assert!(notes[0].message().contains("`shop`"));
    }

    #[test]
    fn the_note_survives_deny_warnings() {
        let project = Project::new();
        project.file("src/lib.rs", "/// @route GET /a\nfn a() {}");
        let generator = Generator::new()
            .input(project.path("src"))
            .output(project.path("api.yaml"))
            .package(package())
            .deny_warnings(true);
        generator.generate().expect("notes never fail a run");
        assert_eq!(project.read_yaml("api.yaml")["info"]["title"], "shop");
    }

    #[test]
    fn a_root_in_the_sources_wins() {
        let project = Project::new();
        project.file("src/root.rs", ROOT_DOC);
        let spec = Generator::new()
            .input(project.path("src"))
            .package(package())
            .build()
            .unwrap();
        assert_eq!(spec.document()["info"]["title"], "Test API");
        assert_eq!(spec.document()["openapi"], "3.1.0");
        assert!(spec.diagnostics().is_empty());
    }

    #[test]
    fn partial_roots_are_completed() {
        let project = Project::new();
        project.file(
            "src/lib.rs",
            "/// @openapi\n/// info:\n///   description: Ours\n///   contact: { name: Team }\nfn docs() {}",
        );
        let spec = Generator::new()
            .input(project.path("src"))
            .package(package())
            .build()
            .unwrap();
        assert_eq!(
            spec.document()["info"],
            json!({
                "title": "shop",
                "version": "2.1.0",
                "description": "Ours",
                "contact": { "name": "Team" }
            })
        );
    }

    #[test]
    fn the_package_defines_the_cargo_variables() {
        let project = Project::new();
        project.file(
            "src/lib.rs",
            "/// @openapi\n/// info: { description: '{{CARGO_PKG_NAME}} {{CARGO_PKG_VERSION_MAJOR}} [{{CARGO_PKG_LICENSE}}] {{STAGE}}' }\nfn docs() {}",
        );
        let description = |generator: Generator| {
            generator
                .input(project.path("src"))
                .build()
                .unwrap()
                .document()["info"]["description"]
                .clone()
        };
        assert_eq!(
            description(Generator::new().var("STAGE", "dev").package(package())),
            "shop 2 [] dev",
            "the package replaces values from the environment"
        );
        assert_eq!(
            description(
                Generator::new()
                    .package(package())
                    .var("CARGO_PKG_NAME", "renamed")
                    .var("STAGE", "x")
            ),
            "renamed 2 [] x",
            "later variables win"
        );
    }

    #[test]
    fn without_a_package_a_full_output_needs_a_root() {
        let project = Project::new();
        project.file("src/lib.rs", "/// @route GET /a\nfn a() {}");
        let result = Generator::new()
            .input(project.path("src"))
            .output(project.path("api.yaml"))
            .package(package())
            .without_package()
            .generate();
        assert!(matches!(result, Err(Error::NoRootFound)), "{result:?}");
    }
}

mod checks {
    use super::*;

    #[test]
    fn check_lists_missing_and_outdated_outputs() {
        let project = Project::new();
        project.file("src/root.rs", ROOT_DOC);
        let generator = Generator::new()
            .input(project.path("src"))
            .output(project.path("api.yaml"))
            .output_schemas(project.path("schemas.json"));
        let outdated = generator.build().unwrap().check().unwrap();
        assert_eq!(
            outdated,
            [project.path("api.yaml"), project.path("schemas.json")]
        );

        generator.generate().unwrap();
        assert!(generator.build().unwrap().check().unwrap().is_empty());

        project.file("schemas.json", "{}");
        assert_eq!(
            generator.build().unwrap().check().unwrap(),
            [project.path("schemas.json")]
        );
    }

    #[test]
    fn check_fails_for_outputs_that_cannot_be_rendered() {
        let project = Project::new();
        project.file("src/lib.rs", "/// @openapi\nstruct S { a: u8 }");
        let spec = Generator::new()
            .input(project.path("src"))
            .output(project.path("api.yaml"))
            .without_package()
            .build()
            .unwrap();
        assert!(matches!(spec.check(), Err(Error::NoRootFound)));
    }

    #[test]
    fn check_does_not_write() {
        let project = Project::new();
        project.file("src/root.rs", ROOT_DOC);
        let spec = Generator::new()
            .input(project.path("src"))
            .output(project.path("api.yaml"))
            .build()
            .unwrap();
        spec.check().unwrap();
        assert!(!project.path("api.yaml").exists());
    }
}

mod output_builders {
    use super::*;

    #[test]
    fn every_export_has_a_builder_method() {
        let project = Project::new();
        project.file("src/root.rs", ROOT_DOC).file(
            "src/events.rs",
            "/// @asyncapi\n/// asyncapi: 3.0.0\n/// info: { title: E, version: '1' }\nfn events() {}\n/// @channel ping\n/// @send Ping: String\nfn ping() {}",
        );
        Generator::new()
            .input(project.path("src"))
            .output_json_schema(project.path("schemas.json"))
            .output_openapi_30(project.path("v30.yaml"))
            .output_swagger2(project.path("v2.yaml"))
            .output_asyncapi(project.path("events.yaml"))
            .generate()
            .unwrap();
        assert!(project.read_yaml("schemas.json")["$schema"].is_string());
        assert_eq!(project.read_yaml("v30.yaml")["openapi"], "3.0.3");
        assert_eq!(project.read_yaml("v2.yaml")["swagger"], "2.0");
        assert_eq!(project.read_yaml("events.yaml")["asyncapi"], "3.0.0");
    }
}

mod empty_blocks {
    use super::*;

    #[test]
    fn empty_documents_and_rejected_channels_leave_no_trace() {
        let (document, diagnostics) = pipeline(&[
            (
                "root.yaml",
                "openapi: 3.1.0\ninfo: { title: T, version: '1' }",
            ),
            (
                "lib.rs",
                "/// @openapi\n/// # only a comment\nfn a() {}\n/// @asyncapi\n/// # nothing\nfn b() {}\n/// @channel\n/// @send M: String\nfn c() {}",
            ),
        ]);
        assert_eq!(codes(&diagnostics), [Code::InvalidRoute]);
        assert_eq!(
            document,
            json!({ "openapi": "3.1.0", "info": { "title": "T", "version": "1" } })
        );
    }
}
