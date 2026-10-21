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
            .file("src/a.rs", "/// @route GET /x\n/// @frobnicate\n/// @description {{env:HOME}} {{env:OAS_FORGE_UNSET_X}} {{env:HOME}}\nfn x() {}");
        let generator = Generator::new()
            .input(project.path("src"))
            .output(project.path("api.yaml"))
            .emit_cargo_directives(true);
        generator.generate().unwrap();

        let spec = generator.build().unwrap();
        assert_eq!(spec.watched_env(), ["HOME", "OAS_FORGE_UNSET_X"]);
        let mut out = Vec::new();
        spec.write_cargo_directives(&mut out).unwrap();
        let text = String::from_utf8(out).unwrap();
        let lines: Vec<&str> = text.lines().collect();
        assert_eq!(
            lines[0],
            format!("cargo:rerun-if-changed={}", project.path("src").display())
        );
        assert_eq!(lines[1], "cargo:rerun-if-env-changed=HOME");
        assert_eq!(lines[2], "cargo:rerun-if-env-changed=OAS_FORGE_UNSET_X");
        // Variables are substituted before the DSL is compiled.
        assert!(
            lines[3].starts_with("cargo:warning=warning[OF3009]"),
            "{text}"
        );
        assert!(
            lines[4].starts_with("cargo:warning=warning[OF2006]"),
            "{text}"
        );
        assert_eq!(lines.len(), 5, "{text}");
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
        // `$Missing` is reported where it is written and, as a pointer that
        // leads nowhere, by the lints.
        assert_eq!(
            codes(&diagnostics),
            [
                Code::UnresolvedReference,
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
            messages[0].contains("$Missing")
                && messages[1].contains("#/components/schemas/Missing")
                && messages[2].contains("Ignored"),
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
            json!({
                "Shared": { "type": "object" },
                "Page_Shared": { "items": { "$ref": "#/components/schemas/Shared" } }
            })
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
    fn references_that_are_no_names_are_reported() {
        let (document, diagnostics) = pipeline(&[(
            "lib.rs",
            "/// @openapi\n/// openapi: 3.1.0\n/// info: { title: T, version: '1' }\n/// components:\n///   schemas:\n///     A: { properties: { b: { $ref: '$Bad Name' }, c: { description: '$Money matters' } } }\nfn root() {}",
        )]);
        let properties = &document["components"]["schemas"]["A"]["properties"];
        assert_eq!(properties["b"], json!({ "$ref": "$Bad Name" }));
        assert_eq!(
            properties["c"]["description"], "$Money matters",
            "text stays text"
        );
        assert_eq!(codes(&diagnostics), [Code::UnresolvedReference]);
    }

    #[test]
    fn mappings_to_references_apply_to_smart_references_too() {
        let project = Project::new();
        project.file(
            "src/root.rs",
            "/// @openapi\n/// openapi: 3.1.0\n/// info: { title: T, version: '1' }\n/// components:\n///   schemas:\n///     Cents: { type: integer }\n///     Order:\n///       type: object\n///       properties:\n///         total: { $ref: $Money }\nfn root() {}\n/// @openapi\nstruct Invoice { total: Money }",
        );
        let spec = Generator::new()
            .input(project.path("src"))
            .map_type("Money", json!({ "$ref": "#/components/schemas/Cents" }))
            .build()
            .unwrap();
        let schemas = &spec.document()["components"]["schemas"];
        let cents = json!({ "$ref": "#/components/schemas/Cents" });
        assert_eq!(schemas["Invoice"]["properties"]["total"], cents);
        assert_eq!(
            schemas["Order"]["properties"]["total"], cents,
            "`$Money` in YAML"
        );
        assert!(spec.diagnostics().is_empty(), "{:?}", spec.diagnostics());
    }

    #[test]
    fn mapped_types_are_used_by_structs_aliases_and_routes() {
        let project = Project::new();
        project.file("src/root.rs", ROOT_DOC).file(
            "src/api.rs",
            "/// @openapi\nstruct User { id: UserId, tags: Vec<geo::Point> }\n/// @openapi\ntype Owner = UserId;\n/// @route GET /users/{id: UserId}\n/// @return 200: UserId\nfn get() {}",
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

    /// Lines pasted by `@insert` shift the block; findings after them still
    /// name their own source line.
    #[test]
    fn findings_after_an_insert_point_at_their_line() {
        let source = "//! @openapi-fragment Paging\n//! - name: page\n//!   in: query\n//! - name: size\n//!   in: query\n\n/// List\n/// @route GET /items\n/// parameters:\n///   @insert Paging\n/// @bogus directive\nfn list() {}\n";
        let (_, diagnostics) = pipeline(&[("api.rs", source)]);
        let unknown: Vec<usize> = diagnostics
            .iter()
            .filter(|d| d.code() == Code::UnknownDirective)
            .filter_map(|d| d.location().map(|l| l.line()))
            .collect();
        assert_eq!(unknown, vec![11], "{diagnostics:?}");
    }

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
    fn route_defaults_are_expanded_once_per_module() {
        let (document, diagnostics) = pipeline(&[(
            "api.rs",
            "/// Outer.\n/// @route-defaults\n/// @server https://{{env:OAS_FORGE_UNSET_TEST_VAR}}.example.com\n/// @insert Missing(1)\nmod api {\n  /// @route GET /a\n  fn a() {}\n  /// @route GET /b\n  fn b() {}\n  /// @route-defaults\n  /// @tag Inner\n  mod inner {\n    /// @route GET /c\n    fn c() {}\n  }\n}",
        )]);
        let found: Vec<String> = diagnostics
            .iter()
            .map(|d| format!("{} {}", d.code(), d.location().unwrap()))
            .collect();
        // Once each, at the line in the module's docs, not once per route.
        assert_eq!(found, ["OF3001 api.rs:4", "OF3009 api.rs:3"]);
        for path in ["/a", "/b", "/c"] {
            assert_eq!(
                document["paths"][path]["get"]["servers"][0]["url"],
                "https://{{env:OAS_FORGE_UNSET_TEST_VAR}}.example.com",
                "{path}"
            );
        }
    }

    #[test]
    fn inner_defaults_win_over_outer_ones_and_the_route_over_both() {
        let (document, diagnostics) = pipeline(&[(
            "api.rs",
            "//! @openapi-fragment Limited(n)\n//! x-limit: {{n}}\n//! x-from: '{{n}}'\n\n/// @route-defaults\n/// x-rate: 10\n/// @extend Limited(1)\nmod api {\n  /// @route GET /a\n  fn a() {}\n  /// @route-defaults\n  /// x-rate: 20\n  /// @extend Limited(2)\n  mod inner {\n    /// @route GET /b\n    fn b() {}\n    /// @route GET /c\n    /// x-rate: 30\n    /// @extend Limited(3)\n    fn c() {}\n  }\n}",
        )]);
        assert!(diagnostics.is_empty(), "{diagnostics:?}");
        let values = |path: &str| {
            let op = &document["paths"][path]["get"];
            (op["x-rate"].clone(), op["x-limit"].clone())
        };
        assert_eq!(values("/a"), (json!(10), json!(1)));
        assert_eq!(values("/b"), (json!(20), json!(2)));
        assert_eq!(values("/c"), (json!(30), json!(3)));
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
    fn query_parameters_in_yaml_win_over_inferred_ones() {
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
        // Single parameters written in the block do not stop the inference
        // of the handler's `Query<Paging>`.
        assert_eq!(names("/a"), ["q", "page", "size"]);
        assert_eq!(names("/b"), ["X-Trace", "page", "size"]);
    }

    #[test]
    fn flattened_structs_are_expanded_too() {
        let (document, diagnostics) = pipeline(&[
            ("models.rs", MODELS),
            (
                "api.rs",
                "/// @openapi\n#[derive(serde::Deserialize)]\nstruct Sorting { sort: String }\n/// @openapi\n#[derive(serde::Deserialize)]\nstruct Filter {\n  q: Option<String>,\n  #[serde(flatten)]\n  paging: Paging,\n  #[serde(flatten)]\n  sorting: Option<Sorting>,\n}\n/// @route GET /a\nasync fn a(Query(f): Query<Filter>) {}",
            ),
        ]);
        assert!(diagnostics.is_empty(), "{diagnostics:?}");
        let names: Vec<String> = document["paths"]["/a"]["get"]["parameters"]
            .as_array()
            .unwrap()
            .iter()
            .map(|p| format!("{}:{}", p["name"].as_str().unwrap(), p["required"]))
            .collect();
        // `sort` is required in `Sorting`, but the flattened field is optional.
        assert_eq!(names, ["q:false", "page:false", "size:true", "sort:false"]);
    }

    #[test]
    fn optional_query_structs_make_every_parameter_optional() {
        let (document, diagnostics) = pipeline(&[
            ("models.rs", MODELS),
            (
                "api.rs",
                "/// @route GET /a\n/// @query-param page: u32 required \"Explicit\"\nasync fn a(q: Option<Query<Paging>>) {}",
            ),
        ]);
        assert!(diagnostics.is_empty(), "{diagnostics:?}");
        let required: Vec<String> = document["paths"]["/a"]["get"]["parameters"]
            .as_array()
            .unwrap()
            .iter()
            .map(|p| format!("{}:{}", p["name"].as_str().unwrap(), p["required"]))
            .collect();
        // `size` is required in `Paging`, but the whole query may be missing;
        // explicit parameters still win.
        assert_eq!(required, ["page:true", "size:false"]);
    }

    #[test]
    fn cyclic_flattening_terminates() {
        let (document, _) = pipeline(&[(
            "api.rs",
            "//! @openapi-type A\n//! type: object\n//! properties: { a: { type: string } }\n//! allOf: [{ $ref: $B }]\n\n//! @openapi-type B\n//! type: object\n//! properties: { b: { type: string } }\n//! allOf: [{ $ref: $A }]\n\n/// @route GET /a\n/// @query-params $A\nfn a() {}",
        )]);
        let names: Vec<_> = document["paths"]["/a"]["get"]["parameters"]
            .as_array()
            .unwrap()
            .iter()
            .map(|p| p["name"].clone())
            .collect();
        assert_eq!(names, [json!("a"), json!("b")]);
    }

    #[test]
    fn qualified_struct_paths_are_expanded() {
        let (document, diagnostics) = pipeline(&[
            ("models.rs", &format!("pub mod models {{\n{MODELS}\n}}")),
            (
                "api.rs",
                "/// @route GET /orgs/{org}/users/{id}\nasync fn a(Path(p): Path<models::Ids>, Query(q): axum::extract::Query<crate::models::Paging>) {}\n/// @route GET /b\n/// @query-params models::Paging\nfn b() {}",
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
        assert_eq!(
            names("/orgs/{org}/users/{id}"),
            ["org", "id", "page", "size"]
        );
        assert_eq!(names("/b"), ["page", "size"]);
    }

    #[test]
    fn unresolved_expansions_are_reported_at_their_annotation() {
        let (_, diagnostics) = pipeline(&[(
            "api.rs",
            "\n/// Get\n/// @route GET /a\n/// @query-params $Nope\nfn a() {}\n/// @route GET /b\nasync fn b(Query(q): Query<Gone>) {}",
        )]);
        let located: Vec<String> = diagnostics
            .iter()
            .filter(|d| d.code() == Code::UnresolvedExpansion)
            .map(|d| d.location().map(ToString::to_string).unwrap_or_default())
            .collect();
        assert_eq!(located, ["api.rs:4", "api.rs:6"]);
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

mod proto_inputs {
    use super::*;

    #[test]
    fn proto_includes_are_watched_by_cargo() {
        let project = Project::new();
        project
            .file("src/root.rs", ROOT_DOC)
            .file("proto/chat.proto", "message Hello {}");
        let spec = Generator::new()
            .input(project.path("src"))
            .include(project.path("proto/chat.proto"))
            .build()
            .unwrap();
        assert!(spec.diagnostics().is_empty(), "{:#?}", spec.diagnostics());
        let mut out = Vec::new();
        spec.write_cargo_directives(&mut out).unwrap();
        let out = String::from_utf8(out).unwrap();
        assert!(
            out.contains(&format!(
                "cargo:rerun-if-changed={}\n",
                project.path("proto/chat.proto").display()
            )),
            "{out}"
        );
        // A `.proto` file is no OpenAPI snippet.
        assert_eq!(spec.document()["info"]["title"], "Test API");
        assert!(spec.document().get("paths").is_none());
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

mod dsl_references {
    use super::*;

    #[test]
    fn dsl_types_follow_renamed_schemas() {
        let (document, diagnostics) = pipeline(&[(
            "api.rs",
            "/// @openapi rename \"Member\"\nstruct TeamMember { id: u8 }\n/// @route GET /a\n/// @return 200: $TeamMember\n/// @return 201: TeamMember\nfn a() {}",
        )]);
        assert!(diagnostics.is_empty(), "{diagnostics:?}");
        let responses = &document["paths"]["/a"]["get"]["responses"];
        for code in ["200", "201"] {
            assert_eq!(
                responses[code]["content"]["application/json"]["schema"],
                json!({ "$ref": "#/components/schemas/Member" }),
                "{code}"
            );
        }
    }

    const ROUTE: &str = "/// @route POST /a/{p: Duration}\n/// @query-param q: Duration default=5 \"Wait\"\n/// @query-param w: std::time::Duration\n/// @header-param X-Id: Uuid\n/// @return-header 200 X-Took: Duration\n/// @body Value\n/// @return 200: Vec<Duration>\n/// @return 201: HashMap<String, Option<Duration>>\nfn a() {}\n/// @route GET /b\nasync fn b() -> Json<Duration> { todo!() }\n/// @channel ticks\n/// @send Tick: Duration\nfn tick() {}";

    #[test]
    fn documented_types_win_over_bare_built_in_names() {
        let (document, diagnostics) = pipeline(&[
            ("api.rs", ROUTE),
            (
                "models.rs",
                "/// @openapi\nstruct Duration { minutes: u32 }\n/// @openapi rename \"AnyValue\"\nstruct Value { raw: String }",
            ),
        ]);
        assert!(diagnostics.is_empty(), "{diagnostics:?}");
        let duration = json!({ "$ref": "#/components/schemas/Duration" });
        let op = &document["paths"]["/a/{p}"]["post"];
        let schema = |i: usize| op["parameters"][i]["schema"].clone();
        assert_eq!(schema(0), duration);
        // Flags apply to the documented schema too.
        assert_eq!(
            schema(1),
            json!({ "$ref": "#/components/schemas/Duration", "default": 5 })
        );
        // Paths keep the built-in type; undocumented names too.
        assert_eq!(schema(2)["type"], "object");
        assert_eq!(schema(3), json!({ "type": "string", "format": "uuid" }));
        let response = &op["responses"]["200"];
        assert_eq!(response["headers"]["X-Took"]["schema"], duration);
        assert_eq!(
            response["content"]["application/json"]["schema"],
            json!({ "type": "array", "items": duration })
        );
        assert_eq!(
            op["responses"]["201"]["content"]["application/json"]["schema"]["additionalProperties"],
            json!({ "anyOf": [duration, { "type": "null" }] })
        );
        assert_eq!(
            op["requestBody"]["content"]["application/json"]["schema"],
            json!({ "$ref": "#/components/schemas/AnyValue" })
        );
        assert_eq!(
            document["paths"]["/b"]["get"]["responses"]["200"]["content"]["application/json"]["schema"],
            duration
        );
        assert!(!document.to_string().contains("x-oas-forge-builtin"));
    }

    #[test]
    fn without_documented_types_built_in_names_are_unchanged() {
        let (document, diagnostics) = pipeline(&[("api.rs", ROUTE)]);
        assert!(diagnostics.is_empty(), "{diagnostics:?}");
        let op = &document["paths"]["/a/{p}"]["post"];
        assert_eq!(op["parameters"][0]["schema"]["type"], "object");
        assert_eq!(op["parameters"][1]["schema"]["default"], 5);
        assert_eq!(
            op["requestBody"]["content"]["application/json"]["schema"],
            json!({})
        );
        assert!(!document.to_string().contains("x-oas-forge-builtin"));
    }

    #[test]
    fn yaml_merged_into_a_built_in_schema_keeps_its_order() {
        let (document, _) = pipeline(&[(
            "api.rs",
            "/// @route GET /a\n/// @query-param d: Uuid\n/// parameters:\n///   - { name: d, in: query, schema: { description: Wait, example: 1 } }\nfn a() {}",
        )]);
        let schema = document["paths"]["/a"]["get"]["parameters"][0]["schema"].clone();
        let keys: Vec<_> = schema.as_object().unwrap().keys().cloned().collect();
        assert_eq!(keys, ["type", "format", "description", "example"]);
    }
}

mod review_fixes {
    use super::*;
    use oas_forge::Package;

    #[test]
    fn findings_of_imported_templates_are_reported() {
        let (_, diagnostics) = pipeline(&[(
            "shared.yaml",
            "components:\n  x-oas-forge-format: 99\n  x-oas-forge-templates:\n    Page: { params: [T], body: 'properties: [unclosed' }",
        )]);
        assert_eq!(
            codes(&diagnostics),
            [Code::UnknownTemplateFormat, Code::InvalidFragment]
        );
    }

    #[test]
    fn tags_with_the_same_name_are_merged() {
        let (document, diagnostics) = pipeline(&[
            ("a.yaml", "tags: [Users, Admin]"),
            (
                "b.yaml",
                "tags: [{ name: Users, description: Everything about users }]",
            ),
        ]);
        assert!(diagnostics.is_empty(), "{diagnostics:?}");
        assert_eq!(
            document["tags"],
            json!([
                { "name": "Users", "description": "Everything about users" },
                { "name": "Admin" }
            ])
        );

        let (document, diagnostics) = pipeline(&[
            ("a.yaml", "tags: [{ name: Users, description: First }]"),
            ("b.yaml", "tags: [{ name: Users, description: Second }]"),
        ]);
        assert_eq!(
            document["tags"],
            json!([{ "name": "Users", "description": "Second" }])
        );
        assert_eq!(codes(&diagnostics), [Code::MergeConflict]);
        assert!(
            diagnostics
                .iter()
                .any(|d| d.message().contains("`/tags/Users/description`")),
            "{diagnostics:?}"
        );
    }

    #[test]
    fn configuration_files_are_watched() {
        let project = Project::new();
        project
            .file("src/root.rs", ROOT_DOC)
            .file(
                "Cargo.toml",
                "[package]\nname = \"demo\"\n[package.metadata.oas-forge.variables]\nSTAGE = \"dev\"\n",
            )
            .file("openapi.toml", "[variables]\nSTAGE = \"prod\"\n");
        let config = Config::load_from(project.root()).unwrap();
        let spec = Generator::new()
            .input(project.path("src"))
            .with_config(config)
            .build()
            .unwrap();
        assert_eq!(
            spec.watched(),
            [
                project.path("src"),
                project.path("Cargo.toml"),
                project.path("openapi.toml"),
            ]
        );

        // A manifest without settings is not a configuration file.
        project.file("Cargo.toml", "[package]\nname = \"demo\"\n");
        let config = Config::load_from(project.root()).unwrap();
        assert_eq!(config.files(), [project.path("openapi.toml")]);
    }

    #[test]
    fn the_generated_root_does_not_leak_into_fragments() {
        let mut package = Package::new("lib-crate", "1.0.0");
        package.homepage = Some("https://lib.example.com".into());
        let project = Project::new();
        project.file("src/lib.rs", "/// @openapi\nstruct A { a: u8 }");
        let spec = Generator::new()
            .input(project.path("src"))
            .package(package.clone())
            .build()
            .unwrap();
        assert_eq!(
            spec.render(OutputKind::Full).unwrap()["externalDocs"]["url"],
            "https://lib.example.com"
        );
        let fragments = spec.render(OutputKind::Fragments).unwrap();
        assert!(fragments.get("externalDocs").is_none(), "{fragments:#}");
        assert!(fragments["components"]["schemas"].get("A").is_some());

        // Values written in the sources stay.
        project.file(
            "src/docs.yaml",
            "externalDocs: { url: 'https://own.example.com' }",
        );
        let spec = Generator::new()
            .input(project.path("src"))
            .package(package)
            .build()
            .unwrap();
        assert_eq!(
            spec.render(OutputKind::Fragments).unwrap()["externalDocs"]["url"],
            "https://own.example.com"
        );
    }
}

#[test]
fn configured_outputs_are_rendered_once_and_reused() {
    let project = Project::new();
    project.file("src/root.rs", ROOT_DOC);
    let spec = Generator::new()
        .input(project.path("src"))
        .output(project.path("api.yaml"))
        .build()
        .unwrap();
    assert_eq!(
        spec.render(OutputKind::Full).unwrap(),
        spec.render_with_diagnostics(OutputKind::Full).unwrap().0
    );
}

/// `@route-defaults` of modules in files of their own (`mod name;`).
mod file_module_defaults {
    use super::*;

    const ROOT: &str = "//! @openapi\n//! openapi: 3.1.0\n//! info: { title: T, version: '1' }\n//! components:\n//!   securitySchemes:\n//!     bearer: { type: http, scheme: bearer }\npub mod routes;\n";

    fn operation<'d>(document: &'d Value, path: &str) -> &'d Value {
        let operation = &document["paths"][path]["get"];
        assert!(operation.is_object(), "no GET {path} in {document:#}");
        operation
    }

    #[test]
    fn both_forms_apply_to_the_routes_of_the_module_file() {
        let files = [
            ("src/lib.rs", ROOT),
            (
                "src/routes/mod.rs",
                "/// Deck routes.\n/// @route-defaults\n/// @tag Decks\n/// @security bearer\npub mod decks;\npub mod social;\n",
            ),
            (
                "src/routes/decks.rs",
                "/// List decks\n/// @route GET /decks\npub async fn list() {}\n",
            ),
            (
                "src/routes/social.rs",
                "//! Social routes.\n//! @route-defaults\n//! @prefix /social\n//! @tag Social\n//! @security bearer\n\n/// List devices\n/// @route GET /devices\npub async fn devices() {}\n",
            ),
        ];
        let (document, diagnostics) = pipeline(&files);
        assert!(diagnostics.is_empty(), "{diagnostics:?}");
        let decks = operation(&document, "/decks");
        assert_eq!(decks["tags"], json!(["Decks"]));
        assert_eq!(decks["security"], json!([{ "bearer": [] }]));
        let devices = operation(&document, "/social/devices");
        assert_eq!(devices["tags"], json!(["Social"]));
        assert_eq!(devices["security"], json!([{ "bearer": [] }]));
        // Files are linked after extraction: their order does not matter.
        let mut reversed = files;
        reversed.reverse();
        assert_eq!(pipeline(&reversed).0, document);
    }

    #[test]
    fn defaults_nest_along_the_module_tree() {
        let (document, diagnostics) = pipeline(&[
            (
                "src/lib.rs",
                &ROOT.replace(
                    "pub mod routes;",
                    "/// @route-defaults\n/// @prefix /api\n/// @tag Api\n/// @problem 401 \"Not signed in\"\npub mod routes;",
                ),
            ),
            // Defaults in a file without routes apply to its modules.
            (
                "src/routes/mod.rs",
                "//! @route-defaults\n//! @prefix /v1\n//! @security bearer\n\npub mod decks;\n",
            ),
            (
                "src/routes/decks.rs",
                "//! @route-defaults\n//! @tag Decks\n\n/// @route GET /decks\nfn list() {}\n\npub mod cards {\n    /// @route GET /cards\n    /// @security none\n    fn cards() {}\n}\n",
            ),
        ]);
        assert!(diagnostics.is_empty(), "{diagnostics:?}");
        let decks = operation(&document, "/api/v1/decks");
        // The innermost `@tag` replaces the outer ones.
        assert_eq!(decks["tags"], json!(["Decks"]));
        assert_eq!(decks["security"], json!([{ "bearer": [] }]));
        assert!(decks["responses"]["401"].is_object());
        let cards = operation(&document, "/api/v1/cards");
        assert_eq!(cards["tags"], json!(["Decks"]));
        assert_eq!(cards["security"], json!([]));
    }

    #[test]
    fn path_attributes_and_inline_modules_are_followed() {
        let (document, diagnostics) = pipeline(&[
            (
                "src/lib.rs",
                &ROOT.replace(
                    "pub mod routes;",
                    "/// @route-defaults\n/// @tag Legacy\n#[path = \"old/handlers.rs\"]\npub mod legacy;\n/// @route-defaults\n/// @prefix /api\npub mod api {\n    /// @route-defaults\n    /// @tag V2\n    pub mod v2;\n}",
                ),
            ),
            (
                "src/old/handlers.rs",
                "/// @route GET /old\nfn old() {}\n",
            ),
            ("src/api/v2.rs", "/// @route GET /items\nfn items() {}\n"),
            // Not declared by any module: only its own defaults.
            (
                "src/legacy.rs",
                "//! @route-defaults\n//! @tag Stray\n\n/// @route GET /stray\nfn stray() {}\n",
            ),
        ]);
        assert!(diagnostics.is_empty(), "{diagnostics:?}");
        assert_eq!(operation(&document, "/old")["tags"], json!(["Legacy"]));
        assert_eq!(operation(&document, "/api/items")["tags"], json!(["V2"]));
        assert_eq!(operation(&document, "/stray")["tags"], json!(["Stray"]));
    }

    #[test]
    fn a_file_declared_twice_belongs_to_the_first_declaration() {
        let files = [
            (
                "src/a.rs",
                "/// @route-defaults\n/// @tag First\n#[path = \"shared.rs\"]\nmod shared;\n",
            ),
            (
                "src/b.rs",
                "/// @route-defaults\n/// @tag Second\n#[path = \"shared.rs\"]\nmod shared;\n",
            ),
            ("src/shared.rs", "/// @route GET /x\nfn x() {}\n"),
        ];
        let (document, _) = pipeline(&files);
        assert_eq!(operation(&document, "/x")["tags"], json!(["First"]));
        let mut reversed = files;
        reversed.reverse();
        assert_eq!(pipeline(&reversed).0, document);
    }

    #[test]
    fn a_module_that_declares_its_own_file_ends_the_chain() {
        let (document, _) = pipeline(&[(
            "src/lib.rs",
            &ROOT.replace(
                "pub mod routes;",
                "//! @route-defaults\n//! @prefix /api\n\n#[path = \"lib.rs\"]\nmod again;\n\n/// @route GET /x\nfn x() {}",
            ),
        )]);
        assert!(operation(&document, "/api/x").is_object());
    }
}

/// Responses with one status from fragments, defaults and the block.
mod merged_responses {
    use super::*;

    const FRAGMENT: &str = "//! @openapi-fragment SessionRefusalsAnyTerms\n//! @return 403: $ErrorBody \"The account is banned\"\n\n/// @openapi\npub struct ErrorBody { pub message: String }\n";

    #[test]
    fn inserted_lines_and_the_block_combine_their_descriptions() {
        let (document, diagnostics) = pipeline(&[(
            "api.rs",
            &format!(
                "{FRAGMENT}\n/// @route POST /sessions\n/// @insert SessionRefusalsAnyTerms\n/// @return 403: $ErrorBody \"The password is wrong\"\nfn login() {{}}\n"
            ),
        )]);
        assert!(diagnostics.is_empty(), "{diagnostics:?}");
        let response = &document["paths"]["/sessions"]["post"]["responses"]["403"];
        assert_eq!(
            response["description"],
            "- The account is banned\n- The password is wrong"
        );
        assert_eq!(
            response["content"]["application/json"]["schema"],
            json!({ "$ref": "#/components/schemas/ErrorBody" })
        );
    }

    #[test]
    fn inserted_lines_in_defaults_combine_with_the_block() {
        let (document, diagnostics) = pipeline(&[(
            "api.rs",
            &format!(
                "{FRAGMENT}\n/// @route-defaults\n/// @insert SessionRefusalsAnyTerms\nmod sessions {{\n  /// @route POST /sessions\n  /// @return 403: $ErrorBody \"The password is wrong\"\n  fn login() {{}}\n  /// @route DELETE /sessions\n  fn logout() {{}}\n}}\n"
            ),
        )]);
        assert!(diagnostics.is_empty(), "{diagnostics:?}");
        let paths = &document["paths"]["/sessions"];
        assert_eq!(
            paths["post"]["responses"]["403"]["description"],
            "- The account is banned\n- The password is wrong"
        );
        assert_eq!(
            paths["delete"]["responses"]["403"]["description"],
            "The account is banned"
        );
    }
}

/// Findings about `@route-defaults` blocks: once per module, at their lines.
mod route_defaults_findings {
    use super::*;

    fn found(diagnostics: &oas_forge::Diagnostics) -> Vec<String> {
        diagnostics
            .iter()
            .map(|d| format!("{} {}", d.code(), d.location().unwrap()))
            .collect()
    }

    #[test]
    fn directive_and_yaml_findings_are_reported_once_at_their_line() {
        let (document, diagnostics) = pipeline(&[(
            "api.rs",
            "//! @openapi-fragment Tags\n//! @tag Api\n//! @tag Shared\n\n/// @route-defaults\n/// @insert Tags\n/// @return oops\nmod api {\n  /// @route GET /a\n  fn a() {}\n  /// @route GET /b\n  fn b() {}\n  /// @route-defaults\n  /// x-limit: [1, 2\n  mod inner {\n    /// @route GET /c\n    fn c() {}\n  }\n}",
        )]);
        // Not once per route, and not at the route: the `@return` line comes
        // after the two lines `@insert` pastes.
        assert_eq!(found(&diagnostics), ["OF2004 api.rs:7", "OF2005 api.rs:14"]);
        for path in ["/a", "/b", "/c"] {
            assert_eq!(
                document["paths"][path]["get"]["tags"],
                json!(["Api", "Shared"]),
                "{path}"
            );
        }
    }

    #[test]
    fn findings_of_file_module_defaults_point_at_the_declaring_file() {
        let (_, diagnostics) = pipeline(&[
            (
                "src/routes/mod.rs",
                "/// @route-defaults\n/// @return oops\npub mod decks;\n",
            ),
            (
                "src/routes/decks.rs",
                "//! @route-defaults\n//! @frobnicate\n\n/// @route GET /a\nfn a() {}\n/// @route GET /b\nfn b() {}\n",
            ),
        ]);
        assert_eq!(
            found(&diagnostics),
            ["OF2004 src/routes/mod.rs:2", "OF2006 src/routes/decks.rs:2"]
        );
    }

    #[test]
    fn expansion_markers_of_defaults_point_at_their_line() {
        let (_, diagnostics) = pipeline(&[(
            "api.rs",
            "/// @route-defaults\n/// @query-params $Missing\nmod api {\n  /// @route GET /a\n  fn a() {}\n  /// @route GET /b\n  fn b() {}\n}",
        )]);
        // One finding per operation, each at the line of the defaults.
        let expansions: Vec<String> = found(&diagnostics)
            .into_iter()
            .filter(|f| f.starts_with(Code::UnresolvedExpansion.as_str()))
            .collect();
        assert_eq!(expansions, ["OF2008 api.rs:2", "OF2008 api.rs:2"]);
    }
}

mod scopes {
    use super::*;
    use oas_forge::Package;

    /// A gateway with public and admin routes and a feedback service whose
    /// `GET /health` collides with the gateway's.
    const SERVICES: &str = r#"
/// @openapi
/// @scope public
/// openapi: 3.1.0
/// info: { title: Public, version: 1.0.0 }
/// tags:
///   - { name: Users, description: People }
///   - { name: Reports }
fn public_root() {}

/// @openapi
/// @scope admin
/// openapi: 3.1.0
/// info: { title: Admin, version: 1.0.0 }
fn admin_root() {}

/// @openapi
/// @scope feedback
/// openapi: 3.1.0
/// info: { title: Feedback, version: 1.0.0 }
fn feedback_root() {}

/// @openapi
/// components:
///   securitySchemes:
///     bearer: { type: http, scheme: bearer }
fn shared() {}

/// A user.
/// @openapi
pub struct User { pub id: u64, pub team: Team }

/// A team.
/// @openapi
pub struct Team { pub name: String }

/// An audit entry.
/// @openapi
pub struct Entry { pub at: String }

/// Gateway endpoints.
/// @route-defaults
/// @scope public
pub mod gateway {
    /// Health
    /// @route GET /health
    pub async fn health() {}

    /// List users
    /// @route GET /users
    /// @tag Users
    /// @return 200: Vec<$User>
    pub async fn list_users() {}

    /// Admin endpoints.
    /// @route-defaults
    /// @scope admin
    pub mod admin {
        /// Read the audit log
        /// @route GET /audit
        /// @return 200: Vec<$Entry>
        /// @security bearer
        pub async fn audit() {}
    }
}

/// Feedback service.
/// @route-defaults
/// @scope feedback
pub mod feedback {
    /// Feedback health
    /// @route GET /health
    pub async fn feedback_health() {}
}
"#;

    fn services() -> Project {
        let project = Project::new();
        project.file("src/lib.rs", SERVICES);
        project
    }

    fn scoped(project: &Project) -> Generator {
        Generator::new()
            .input(project.path("src"))
            .without_package()
            .output_scoped("public", OutputKind::Full, project.path("out/public.yaml"))
            .output_scoped("admin", OutputKind::Full, project.path("out/admin.yaml"))
            .output_scoped(
                "feedback",
                OutputKind::Full,
                project.path("out/feedback.yaml"),
            )
    }

    #[test]
    fn each_scope_merges_its_own_snippets_and_the_shared_ones() {
        let project = services();
        let spec = scoped(&project).build().unwrap();
        assert!(spec.diagnostics().is_empty(), "{:#?}", spec.diagnostics());

        let public = spec.render_scoped(OutputKind::Full, "public").unwrap();
        assert_eq!(public["info"]["title"], "Public");
        assert_eq!(public["paths"]["/health"]["get"]["operationId"], "health");
        assert!(public["paths"].get("/audit").is_none());
        // Only what the scope uses: `User` and `Team`, the used tag; the
        // security scheme stays.
        assert_eq!(
            public["components"]["schemas"]
                .as_object()
                .map(|s| s.keys().cloned().collect::<Vec<_>>()),
            Some(vec!["Team".to_string(), "User".to_string()])
        );
        assert_eq!(
            public["tags"],
            json!([{ "name": "Users", "description": "People" }])
        );
        assert!(public["components"]["securitySchemes"]["bearer"].is_object());

        let admin = spec.render_scoped(OutputKind::Full, "admin").unwrap();
        assert_eq!(admin["info"]["title"], "Admin");
        assert_eq!(
            admin["paths"]
                .as_object()
                .map(|p| p.keys().cloned().collect::<Vec<_>>()),
            Some(vec!["/audit".to_string()])
        );
        assert!(admin["components"]["schemas"]["Entry"].is_object());
        assert!(admin["components"]["schemas"].get("User").is_none());

        // The colliding `GET /health` of the feedback service.
        let feedback = spec.render_scoped(OutputKind::Full, "feedback").unwrap();
        assert_eq!(
            feedback["paths"]["/health"]["get"]["operationId"],
            "feedback_health"
        );
        // Nothing is left of the schemas; the security scheme stays.
        assert_eq!(
            feedback["components"],
            json!({ "securitySchemes": { "bearer": { "type": "http", "scheme": "bearer" } } })
        );
        assert!(feedback.get("tags").is_none());
    }

    #[test]
    fn write_and_check_cover_scoped_outputs() {
        let project = services();
        let generator = scoped(&project);
        let paths = [
            project.path("out/public.yaml"),
            project.path("out/admin.yaml"),
            project.path("out/feedback.yaml"),
        ];
        assert_eq!(generator.build().unwrap().check().unwrap(), paths);
        generator.generate().unwrap();
        assert_eq!(
            project.read_yaml("out/admin.yaml")["info"]["title"],
            "Admin"
        );
        assert!(generator.build().unwrap().check().unwrap().is_empty());
        project.file("out/feedback.yaml", "openapi: 3.1.0\n");
        assert_eq!(
            generator.build().unwrap().check().unwrap(),
            [project.path("out/feedback.yaml")]
        );
    }

    #[test]
    fn scoped_outputs_are_not_read_as_inputs() {
        let project = services();
        let spec = Generator::new()
            .input(project.path("src"))
            .without_package()
            .output_scoped("admin", OutputKind::Full, project.path("src/admin.yaml"))
            .build()
            .unwrap();
        // Not written yet: nothing to skip. Once written, it is skipped.
        assert!(spec.diagnostics().is_empty());
        spec.write().unwrap();
        let spec = Generator::new()
            .input(project.path("src"))
            .without_package()
            .output_scoped("admin", OutputKind::Full, project.path("src/admin.yaml"))
            .build()
            .unwrap();
        assert_eq!(codes(spec.diagnostics()), [Code::OutputNotRead]);
    }

    #[test]
    fn only_scoped_outputs_leave_the_default_document_unbuilt() {
        let project = services();
        let spec = scoped(&project).build().unwrap();
        // Merging every snippet would find three roots.
        assert_eq!(spec.document(), &json!({}));
        assert!(matches!(
            spec.render(OutputKind::Full),
            Err(Error::MultipleRootsFound { .. })
        ));
        // Building it is an error.
        let error = scoped(&project)
            .output(project.path("out/all.yaml"))
            .build()
            .unwrap_err();
        assert!(matches!(error, Error::MultipleRootsFound { locations } if locations.len() == 3));
    }

    #[test]
    fn the_default_scope_applies_to_outputs_without_their_own() {
        let project = services();
        let generator = Generator::new()
            .input(project.path("src"))
            .without_package()
            .output(project.path("admin.json"))
            .output_scoped(
                "public",
                OutputKind::Paths,
                project.path("public-paths.json"),
            )
            .scope("admin");
        let spec = generator.build().unwrap();
        assert_eq!(spec.document()["info"]["title"], "Admin");
        assert_eq!(
            spec.render(OutputKind::Full).unwrap()["info"]["title"],
            "Admin"
        );
        generator.generate().unwrap();
        assert_eq!(project.read_yaml("admin.json")["info"]["title"], "Admin");
        assert!(project.read_yaml("public-paths.json")["/users"].is_object());
    }

    #[test]
    fn the_scope_of_a_configuration_is_the_default_scope() {
        let project = services();
        let mut config = Config::default();
        config.scope = Some("feedback".into());
        let spec = Generator::new()
            .input(project.path("src"))
            .without_package()
            .with_config(config)
            .build()
            .unwrap();
        assert_eq!(spec.document()["info"]["title"], "Feedback");
        // A configuration without scope keeps the scope set before.
        let spec = Generator::new()
            .input(project.path("src"))
            .without_package()
            .scope("admin")
            .with_config(Config::default())
            .build()
            .unwrap();
        assert_eq!(spec.document()["info"]["title"], "Admin");
    }

    #[test]
    fn without_a_scope_every_snippet_is_merged() {
        let project = Project::new();
        project.file("src/root.rs", ROOT_DOC).file(
            "src/lib.rs",
            "/// A\n/// @route GET /a\n/// @scope public\npub async fn a() {}\n\
             /// B\n/// @route GET /b\n/// @scope admin\npub async fn b() {}\n",
        );
        let spec = Generator::new()
            .input(project.path("src"))
            .without_package()
            .build()
            .unwrap();
        let paths = spec.document()["paths"].as_object().unwrap();
        assert_eq!(paths.keys().collect::<Vec<_>>(), ["/a", "/b"]);
    }

    #[test]
    fn scopes_not_built_are_merged_on_demand() {
        let project = services();
        let spec = Generator::new()
            .input(project.path("src"))
            .without_package()
            .scope("public")
            .build()
            .unwrap();
        let admin = spec.render_scoped(OutputKind::Full, "admin").unwrap();
        assert_eq!(admin["info"]["title"], "Admin");
        // The default scope by name is the built document.
        assert_eq!(
            spec.render_scoped(OutputKind::Full, "public").unwrap(),
            spec.render(OutputKind::Full).unwrap()
        );
        // A scope without snippets of its own has the shared ones only.
        let other = spec.render_scoped(OutputKind::Fragments, "other").unwrap();
        assert!(other.get("paths").is_none());
    }

    #[test]
    fn scope_lines_of_yaml_blocks_are_taken_out() {
        let project = Project::new();
        project.file(
            "src/lib.rs",
            "/// @openapi\n/// @scope admin, ops\n/// paths:\n///   /admin: {}\nfn admin() {}\n\
             /// @openapi\n/// paths:\n///   /shared: {}\nfn shared() {}\n",
        );
        let generator = Generator::new()
            .input(project.path("src"))
            .without_package();
        let spec = generator.clone().build().unwrap();
        assert!(spec.diagnostics().is_empty(), "{:#?}", spec.diagnostics());
        let all = spec.render(OutputKind::Paths).unwrap();
        assert_eq!(all, json!({ "/admin": {}, "/shared": {} }));
        let ops = spec.render_scoped(OutputKind::Paths, "ops").unwrap();
        assert_eq!(ops, json!({ "/admin": {}, "/shared": {} }));
        let public = spec.render_scoped(OutputKind::Paths, "public").unwrap();
        assert_eq!(public, json!({ "/shared": {} }));
    }

    #[test]
    fn invalid_scope_lines_of_yaml_blocks_are_reported_where_they_are() {
        let project = Project::new();
        project.file(
            "src/lib.rs",
            "/// @openapi\n/// paths:\n///   /a: {}\n/// @scope a b\nfn a() {}\n",
        );
        let spec = Generator::new()
            .input(project.path("src"))
            .without_package()
            .build()
            .unwrap();
        assert_eq!(codes(spec.diagnostics()), [Code::InvalidDirective]);
        let finding = spec.diagnostics().iter().next().unwrap();
        assert_eq!(finding.location().map(|l| l.line()), Some(4));
        // The block is shared.
        assert_eq!(
            spec.render_scoped(OutputKind::Paths, "x").unwrap(),
            json!({ "/a": {} })
        );
    }

    #[test]
    fn yaml_errors_after_a_scope_line_keep_their_line() {
        let line_of_error = |second_line: &str| {
            let project = Project::new();
            project.file(
                "src/lib.rs",
                &format!("/// @openapi\n/// {second_line}\n/// paths:\n///   /a: [\nfn a() {{}}\n"),
            );
            let error = Generator::new()
                .input(project.path("src"))
                .without_package()
                .build()
                .unwrap_err();
            let Error::Yaml { location, .. } = error else {
                panic!("expected a YAML error, got {error:?}");
            };
            location.line()
        };
        // The same line as with a YAML comment in place of the scope line.
        assert_eq!(line_of_error("@scope admin"), line_of_error("# admin"));
    }

    #[test]
    fn findings_are_reported_once_across_scopes() {
        let project = Project::new();
        project.file(
            "src/lib.rs",
            r#"
/// @openapi
/// @scope a
/// openapi: 3.0.3
/// info: { title: A, version: 1.0.0 }
fn a_root() {}

/// @openapi
/// @scope b
/// openapi: 3.0.3
/// info: { title: B, version: 1.0.0 }
fn b_root() {}

/// Shared
/// @route GET /shared
/// @return 200: $Missing
pub async fn shared() {}

/// Only in b
/// @route GET /b
/// @scope b
/// @return 200: $Gone
pub async fn only_b() {}
"#,
        );
        let spec = Generator::new()
            .input(project.path("src"))
            .without_package()
            .output_scoped("a", OutputKind::Full, project.path("a.yaml"))
            .output_scoped("b", OutputKind::Full, project.path("b.yaml"))
            .build()
            .unwrap();
        let messages: Vec<String> = spec
            .diagnostics()
            .iter()
            .map(|d| format!("{}: {}", d.code(), d.message()))
            .collect();
        let dangling: Vec<&String> = messages
            .iter()
            .filter(|m| m.starts_with("OF5001"))
            .collect();
        assert_eq!(dangling.len(), 2, "{messages:#?}");
        assert!(dangling[0].contains("Missing") && dangling[1].contains("Gone"));
    }

    #[test]
    fn pruning_keeps_what_the_scope_reaches() {
        let project = Project::new();
        project.file(
            "src/lib.rs",
            r#"
/// @openapi
/// openapi: 3.2.0
/// info: { title: Api, version: 1.0.0 }
/// tags:
///   - { name: Admin }
///   - { name: Users, parent: People }
///   - { name: People }
///   - { name: Unused }
/// components:
///   securitySchemes:
///     key: { type: apiKey, in: header, name: X-Key }
///   parameters:
///     Page: { name: page, in: query, schema: { type: integer } }
///     Unused: { name: unused, in: query, schema: { type: integer } }
///   responses:
///     NotFound: { description: Missing, content: { application/json: { schema: { $ref: '#/components/schemas/Problem' } } } }
///   schemas:
///     Problem: { type: object }
///     Lonely: { type: object }
///     Cycle: { type: object, properties: { next: { $ref: '#/components/schemas/Cycle' } } }
///   x-internal:
///     anything: true
fn root() {}

/// Users
/// @route GET /users
/// @tag Users
/// @scope public
/// parameters: [{ $ref: '#/components/parameters/Page' }]
/// responses: { '404': { $ref: '#/components/responses/NotFound' } }
pub async fn users() {}

/// Admin
/// @route GET /admin
/// @tag Admin
/// @scope admin
pub async fn admin() {}
"#,
        );
        let spec = Generator::new()
            .input(project.path("src"))
            .without_package()
            .build()
            .unwrap();
        let public = spec.render_scoped(OutputKind::Full, "public").unwrap();
        assert_eq!(
            public["components"],
            json!({
                "securitySchemes": { "key": { "type": "apiKey", "in": "header", "name": "X-Key" } },
                "parameters": { "Page": { "name": "page", "in": "query", "schema": { "type": "integer" } } },
                "responses": { "NotFound": { "description": "Missing", "content": { "application/json": { "schema": { "$ref": "#/components/schemas/Problem" } } } } },
                "schemas": { "Problem": { "type": "object" } },
                "x-internal": { "anything": true },
            })
        );
        // The used tag and its parent.
        assert_eq!(
            public["tags"],
            json!([{ "name": "Users", "parent": "People" }, { "name": "People" }])
        );
        let admin = spec.render_scoped(OutputKind::Full, "admin").unwrap();
        assert_eq!(admin["tags"], json!([{ "name": "Admin" }]));
        assert_eq!(
            admin["components"],
            json!({
                "securitySchemes": { "key": { "type": "apiKey", "in": "header", "name": "X-Key" } },
                "x-internal": { "anything": true },
            })
        );
        // Without a scope nothing is pruned.
        let all = spec.render(OutputKind::Full).unwrap();
        assert_eq!(all["tags"].as_array().map(Vec::len), Some(4));
        assert!(all["components"]["schemas"]["Lonely"].is_object());
    }

    #[test]
    fn empty_sections_are_kept_when_nothing_was_pruned() {
        let project = Project::new();
        project.file(
            "src/lib.rs",
            "/// @openapi\n/// openapi: 3.1.0\n/// info: { title: Api, version: 1.0.0 }\n\
             /// tags: []\n/// components: { schemas: {} }\nfn root() {}\n",
        );
        let spec = Generator::new()
            .input(project.path("src"))
            .without_package()
            .scope("any")
            .build()
            .unwrap();
        assert_eq!(spec.document()["tags"], json!([]));
        assert_eq!(spec.document()["components"], json!({ "schemas": {} }));
    }

    #[test]
    fn asyncapi_documents_are_scoped_too() {
        let project = Project::new();
        project.file(
            "src/lib.rs",
            r#"
/// @asyncapi
/// @scope public
/// asyncapi: 3.0.0
/// info: { title: Public events, version: 1.0.0 }
fn public_events() {}

/// @asyncapi
/// @scope admin
/// asyncapi: 3.0.0
/// info: { title: Admin events, version: 1.0.0 }
/// components:
///   messages:
///     Unused: { payload: { type: string } }
fn admin_events() {}

/// A job, used by no route.
/// @openapi
pub struct Job { pub id: u64, pub step: Step }

/// A step of a job.
/// @openapi
pub struct Step { pub name: String }

/// Shared ticks
/// @channel ticks
/// @send Tick: { type: integer }
pub fn tick() {}

/// Admin endpoints.
/// @route-defaults
/// @scope admin
pub mod admin {
    /// Job updates
    /// @channel jobs
    /// @send JobChanged: $Job
    pub fn job_changed() {}
}
"#,
        );
        let spec = Generator::new()
            .input(project.path("src"))
            .without_package()
            .scope("public")
            .build()
            .unwrap();
        let public = spec.render_scoped(OutputKind::AsyncApi, "public").unwrap();
        assert_eq!(public["info"]["title"], "Public events");
        assert_eq!(
            public["channels"]
                .as_object()
                .map(|c| c.keys().cloned().collect::<Vec<_>>()),
            Some(vec!["ticks".to_string()])
        );
        assert!(public["components"].get("schemas").is_none());

        let admin = spec.render_scoped(OutputKind::AsyncApi, "admin").unwrap();
        assert_eq!(admin["info"]["title"], "Admin events");
        assert!(admin["channels"]["jobs"].is_object());
        assert!(admin["operations"]["job_changed"].is_object());
        // The schemas of the message survive although no route uses them;
        // the message nothing refers to is pruned.
        let schemas = admin["components"]["schemas"].as_object().unwrap();
        assert_eq!(schemas.keys().collect::<Vec<_>>(), ["Job", "Step"]);
        let messages = admin["components"]["messages"].as_object().unwrap();
        assert_eq!(messages.keys().collect::<Vec<_>>(), ["JobChanged", "Tick"]);
        // The OpenAPI document of the scope has no use for them.
        let fragments = spec.render_scoped(OutputKind::Fragments, "admin").unwrap();
        assert!(fragments.get("components").is_none(), "{fragments:#}");
    }

    #[test]
    fn every_scope_gets_a_root_from_the_package() {
        let project = services();
        project.file(
            "src/lib.rs",
            "/// A\n/// @route GET /a\n/// @scope a\npub async fn a() {}\n",
        );
        let spec = Generator::new()
            .input(project.path("src"))
            .package(Package::new("shop", "1.0.0"))
            .output_scoped("a", OutputKind::Full, project.path("a.yaml"))
            .output_scoped("b", OutputKind::Full, project.path("b.yaml"))
            .build()
            .unwrap();
        // The generated-root note, once.
        assert_eq!(codes(spec.diagnostics()), [Code::GeneratedRoot]);
        let b = spec.render_scoped(OutputKind::Full, "b").unwrap();
        assert_eq!(b["info"]["title"], "shop");
    }
}
