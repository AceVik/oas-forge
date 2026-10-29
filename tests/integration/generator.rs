//! Output strategies and builder behaviour of `Generator`.

use crate::common::{Project, ROOT_DOC, yaml};
use oas_forge::Generator;
use oas_forge::config::Config;
use oas_forge::diagnostics::Code;
use oas_forge::error::Error;
use serde_json::json;
use std::path::PathBuf;

const ROUTE: &str = "/// @route GET /ping\n/// @return 200: \"pong\"\nfn ping() {}\n";
const MODEL: &str = "/// @openapi\nstruct User { name: String }\n";

fn project_with(sources: &str) -> Project {
    let project = Project::new();
    project.file("src/lib.rs", sources);
    project
}

mod validation {
    use super::*;

    #[test]
    fn at_least_one_output_is_required() {
        let project = project_with(ROOT_DOC);
        let error = Generator::new()
            .input(project.path("src"))
            .generate()
            .unwrap_err();

        assert!(matches!(error, Error::NoOutput), "{error:?}");
        assert!(error.to_string().contains("At least one output path"));
    }

    #[test]
    fn full_spec_requires_a_root_document() {
        let project = project_with(ROUTE);
        let result = Generator::new()
            .input(project.path("src"))
            .output(project.path("openapi.yaml"))
            .generate();

        assert!(matches!(result, Err(Error::NoRootFound)));
        assert!(!project.path("openapi.yaml").exists());
    }

    #[test]
    fn non_mapping_documents_are_rejected_with_their_location() {
        // Regression: such a document used to replace the whole merged spec.
        for (file, content) in [
            ("src/scalar.yaml", "just a string"),
            ("src/list.yaml", "- a\n- b"),
        ] {
            let project = project_with(ROOT_DOC);
            project.file(file, content);

            let result = Generator::new()
                .input(project.path("src"))
                .output_schemas(project.path("schemas.yaml"))
                .generate();

            match result {
                Err(Error::Yaml { location, .. }) => {
                    assert_eq!(location.file(), project.path(file));
                    assert_eq!(location.line(), 1);
                }
                other => panic!("expected a located error for {file}, got {other:?}"),
            }
            assert!(!project.path("schemas.yaml").exists(), "{file}");
        }
    }

    #[test]
    fn scan_errors_are_propagated() {
        let project = Project::new();
        project.dir("src");
        let result = Generator::new()
            .input(project.path("src"))
            .output_schemas(project.path("schemas.yaml"))
            .generate();

        assert!(matches!(result, Err(Error::NoFilesFound)));
    }

    #[test]
    fn write_errors_are_propagated() {
        let project = project_with(ROOT_DOC);
        project.dir("taken");

        let result = Generator::new()
            .input(project.path("src"))
            .output(project.path("taken"))
            .generate();

        assert!(matches!(result, Err(Error::Io { .. })));
    }
}

mod strategies {
    use super::*;

    #[test]
    fn full_spec_contains_everything() {
        let project = project_with(&format!("{ROOT_DOC}{ROUTE}{MODEL}"));

        Generator::new()
            .input(project.path("src"))
            .output(project.path("openapi.yaml"))
            .generate()
            .unwrap();

        let spec = project.read_yaml("openapi.yaml");
        assert_eq!(spec["info"]["title"], "Test API");
        assert_eq!(spec["paths"]["/ping"]["get"]["operationId"], "ping");
        assert_eq!(
            spec["components"]["schemas"]["User"]["properties"]["name"],
            json!({ "type": "string" })
        );
    }

    #[test]
    fn schemas_output_is_the_bare_schema_map() {
        let project = project_with(&format!("{ROUTE}{MODEL}"));

        Generator::new()
            .input(project.path("src"))
            .output_schemas(project.path("schemas.yaml"))
            .generate()
            .unwrap();

        assert_eq!(
            project.read_yaml("schemas.yaml"),
            json!({ "User": {
                "type": "object",
                "properties": { "name": { "type": "string" } },
                "required": ["name"]
            }})
        );
    }

    #[test]
    fn paths_output_is_the_bare_path_map() {
        let project = project_with(&format!("{ROUTE}{MODEL}"));

        Generator::new()
            .input(project.path("src"))
            .output_paths(project.path("paths.yaml"))
            .generate()
            .unwrap();

        let paths = project.read_yaml("paths.yaml");
        assert_eq!(paths.as_object().unwrap().len(), 1);
        assert_eq!(
            paths["/ping"]["get"]["responses"]["200"]["description"],
            "pong"
        );
    }

    #[test]
    fn partial_outputs_are_empty_maps_when_nothing_matches() {
        let project = project_with("/// @openapi\n/// tags: [{ name: Only }]\nfn tags() {}");

        Generator::new()
            .input(project.path("src"))
            .output_schemas(project.path("schemas.yaml"))
            .output_paths(project.path("paths.yaml"))
            .generate()
            .unwrap();

        assert_eq!(project.read_yaml("schemas.yaml"), json!({}));
        assert_eq!(project.read_yaml("paths.yaml"), json!({}));
    }

    #[test]
    fn fragments_output_is_headless() {
        let project = project_with(
            r#"
            /// @openapi
            /// openapi: 3.1.0
            /// info: { title: "Strip Me", version: "1" }
            /// servers: [{ url: "http://localhost" }]
            /// tags: [{ name: Kept }]
            fn root() {}

            /// @route GET /test
            fn test() {}
            "#,
        );

        Generator::new()
            .input(project.path("src"))
            .output_fragments(project.path("fragments.yaml"))
            .generate()
            .unwrap();

        let fragment = project.read_yaml("fragments.yaml");
        for stripped in ["openapi", "info", "servers"] {
            assert!(
                fragment.get(stripped).is_none(),
                "{stripped} must be stripped"
            );
        }
        assert_eq!(fragment["tags"], json!([{ "name": "Kept" }]));
        assert!(fragment["paths"]["/test"].is_object());
        assert!(fragment.get("components").is_none());
    }

    #[test]
    fn fragments_output_carries_templates_but_full_spec_does_not() {
        let project = project_with(&format!("{ROOT_DOC}{MODEL}"));
        project
            .file(
                "src/fragments.rs",
                "//! @openapi-fragment Err(code)\n//! description: {{code}}\n",
            )
            .file(
                "src/page.rs",
                "/// @openapi<T>\n/// properties:\n///   items: { $ref: $T }\nstruct Page<T>(T);\n",
            );

        Generator::new()
            .input(project.path("src"))
            .output(project.path("full.yaml"))
            .output_schemas(project.path("schemas.yaml"))
            .output_fragments(project.path("fragments.yaml"))
            .generate()
            .unwrap();

        let components = &project.read_yaml("fragments.yaml")["components"];
        assert_eq!(
            components["x-oas-forge-templates"]["Page"]["params"],
            json!(["T"])
        );
        assert_eq!(
            components["x-oas-forge-fragments"]["Err"],
            json!({ "params": ["code"], "body": "description: {{code}}" })
        );
        assert!(components["schemas"]["User"].is_object());

        for clean in ["full.yaml", "schemas.yaml"] {
            assert!(!project.read(clean).contains("x-oas-forge"), "{clean}");
        }
    }

    /// The wire format keys belong to included files: written in a block,
    /// they are reserved keys, removed and reported as an error.
    #[test]
    fn vendor_keys_in_a_block_are_rejected_and_empty_components_dropped() {
        let project = project_with(&format!(
            "{ROOT_DOC}\n/// @openapi\n/// components:\n///   x-oas-forge-templates: {{}}\n///   x-oas-forge-fragments: {{}}\nfn leak() {{}}\n"
        ));

        let generator = Generator::new()
            .input(project.path("src"))
            .output(project.path("openapi.yaml"));
        let spec = generator.build().unwrap();
        let reserved: Vec<&str> = spec
            .diagnostics()
            .iter()
            .filter(|d| d.code() == Code::ReservedKey && d.is_error())
            .map(|d| d.message())
            .collect();
        assert_eq!(reserved.len(), 2, "{:#?}", spec.diagnostics());
        assert!(
            reserved[0].contains("`x-oas-forge-templates`"),
            "{reserved:?}"
        );
        assert!(spec.document().get("components").is_none());
        assert!(matches!(generator.generate(), Err(Error::Diagnostics(_))));
        assert!(!project.path("openapi.yaml").exists());
    }

    #[test]
    fn all_strategies_can_run_together() {
        let project = project_with(&format!("{ROOT_DOC}{ROUTE}{MODEL}"));
        let outputs = ["full.yaml", "schemas.yaml", "paths.yaml", "frag.yaml"];

        Generator::new()
            .input(project.path("src"))
            .output(project.path(outputs[0]))
            .output_schemas(project.path(outputs[1]))
            .output_paths(project.path(outputs[2]))
            .output_fragments(project.path(outputs[3]))
            .generate()
            .unwrap();

        for output in outputs {
            assert!(project.path(output).exists(), "{output}");
        }
        assert!(project.read_yaml("schemas.yaml").get("User").is_some());
        assert!(project.read_yaml("schemas.yaml").get("/ping").is_none());
        assert!(project.read_yaml("frag.yaml").get("info").is_none());
    }
}

mod unusual_documents {
    use super::*;

    const BLUEPRINT: &str = "/// @openapi<T>\n/// type: object\nstruct Page<T>(T);\n";

    #[test]
    fn non_mapping_sections_are_written_as_is() {
        let project = project_with(
            "/// @openapi\n/// paths: [x]\n/// components:\n///   schemas: [y]\nfn odd() {}",
        );

        Generator::new()
            .input(project.path("src"))
            .output_schemas(project.path("schemas.yaml"))
            .output_paths(project.path("paths.yaml"))
            .generate()
            .unwrap();

        assert_eq!(project.read_yaml("schemas.yaml"), json!(["y"]));
        assert_eq!(project.read_yaml("paths.yaml"), json!(["x"]));
    }

    #[test]
    fn templates_are_not_injected_into_non_mapping_components() {
        let project = project_with(&format!(
            "/// @openapi\n/// components: 5\nfn odd() {{}}\n{BLUEPRINT}"
        ));

        Generator::new()
            .input(project.path("src"))
            .output_fragments(project.path("fragments.yaml"))
            .generate()
            .unwrap();

        assert_eq!(
            project.read_yaml("fragments.yaml"),
            json!({ "components": 5 })
        );
    }

    #[test]
    fn empty_output_path_is_an_io_error() {
        let project = project_with(ROOT_DOC);
        let result = Generator::new()
            .input(project.path("src"))
            .output("")
            .generate();
        assert!(matches!(result, Err(Error::Io { .. })));
    }
}

mod files {
    use super::*;

    #[test]
    fn json_extension_writes_pretty_json() {
        let project = project_with(ROOT_DOC);

        Generator::new()
            .input(project.path("src"))
            .output(project.path("openapi.json"))
            .generate()
            .unwrap();

        let text = project.read("openapi.json");
        let spec: serde_json::Value = serde_json::from_str(&text).expect("valid JSON");
        assert_eq!(spec["info"]["title"], "Test API");
        assert!(text.contains("\n  \"info\""), "pretty printed: {text}");
    }

    #[test]
    fn other_extensions_write_yaml() {
        let project = project_with(ROOT_DOC);
        let names = ["spec.yaml", "spec.yml", "spec.txt", "spec"];

        let mut generator = Generator::new().input(project.path("src"));
        for name in names {
            generator = generator.output(project.path(name));
        }
        generator.generate().unwrap();

        for name in names {
            let text = project.read(name);
            assert!(text.starts_with("openapi:"), "{name}: {text}");
            assert_eq!(yaml(&text)["info"]["title"], "Test API", "{name}");
        }
    }

    #[test]
    fn parent_directories_are_created() {
        let project = project_with(ROOT_DOC);

        Generator::new()
            .input(project.path("src"))
            .output(project.path("deeply/nested/dir/openapi.yaml"))
            .generate()
            .unwrap();

        assert!(project.path("deeply/nested/dir/openapi.yaml").exists());
    }

    #[test]
    fn included_files_are_merged() {
        let project = project_with(ROUTE);
        project.file(
            "static/root.yaml",
            "openapi: 3.1.0\ninfo:\n  title: From Include\n  version: '1'",
        );

        Generator::new()
            .input(project.path("src"))
            .include(project.path("static/root.yaml"))
            .output(project.path("openapi.yaml"))
            .generate()
            .unwrap();

        let spec = project.read_yaml("openapi.yaml");
        assert_eq!(spec["info"]["title"], "From Include");
        assert!(spec["paths"]["/ping"].is_object());
    }
}

mod configuration {
    use super::*;

    fn paths(project: &Project, names: &[&str]) -> Option<Vec<PathBuf>> {
        Some(names.iter().map(|n| project.path(n)).collect())
    }

    #[test]
    fn with_config_applies_every_field() {
        let project = project_with(&format!("{ROUTE}{MODEL}"));
        project.file(
            "static/root.yaml",
            "openapi: 3.1.0\ninfo: { title: Cfg, version: '1' }",
        );

        let mut config = Config::default();
        config.input = paths(&project, &["src"]);
        config.include = paths(&project, &["static/root.yaml"]);
        config.output = paths(&project, &["out/full.yaml"]);
        config.output_schemas = paths(&project, &["out/schemas.yaml"]);
        config.output_paths = paths(&project, &["out/paths.yaml"]);
        config.output_fragments = paths(&project, &["out/frag.yaml"]);
        Generator::new().with_config(config).generate().unwrap();

        assert_eq!(project.read_yaml("out/full.yaml")["info"]["title"], "Cfg");
        for output in ["out/schemas.yaml", "out/paths.yaml", "out/frag.yaml"] {
            assert!(project.path(output).exists(), "{output}");
        }
    }

    #[test]
    fn with_config_extends_builder_settings() {
        let project = project_with(ROOT_DOC);

        Generator::new()
            .output(project.path("from-builder.yaml"))
            .with_config({
                let mut config = Config::default();
                config.input = paths(&project, &["src"]);
                config.output = paths(&project, &["from-config.yaml"]);
                config
            })
            .generate()
            .unwrap();

        assert!(project.path("from-builder.yaml").exists());
        assert!(project.path("from-config.yaml").exists());
    }

    #[test]
    fn empty_config_changes_nothing() {
        let project = project_with(ROOT_DOC);

        Generator::new()
            .input(project.path("src"))
            .output(project.path("openapi.yaml"))
            .with_config(Config::default())
            .generate()
            .unwrap();

        assert!(project.path("openapi.yaml").exists());
    }
}
