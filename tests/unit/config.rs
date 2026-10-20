//! `Config` parsing and layered loading (`Cargo.toml` < `openapi.toml`).
//! Command-line flags are covered by the `oas-forge-cli` crate.

use crate::common::Project;
use oas_forge::config::Config;
use oas_forge::error::Error;
use std::path::PathBuf;

fn paths(items: &[&str]) -> Option<Vec<PathBuf>> {
    Some(items.iter().map(PathBuf::from).collect())
}

const ALL_FIELDS: &str = r#"
input = ["src", "lib"]
include = ["static/root.yaml"]
output = ["openapi.yaml", "openapi.json"]
output_schemas = ["schemas.yaml"]
output_paths = ["paths.yaml"]
output_fragments = ["fragments.yaml"]
output_json_schema = ["schemas.json"]
output_openapi_30 = ["openapi-3.0.yaml"]
output_swagger2 = ["swagger.yaml"]
output_asyncapi = ["asyncapi.yaml"]
"#;

mod toml_format {
    use super::*;

    #[test]
    fn reads_all_fields() {
        let config = Config::from_toml_str(ALL_FIELDS).unwrap();

        assert_eq!(config.input, paths(&["src", "lib"]));
        assert_eq!(config.include, paths(&["static/root.yaml"]));
        assert_eq!(config.output, paths(&["openapi.yaml", "openapi.json"]));
        assert_eq!(config.output_schemas, paths(&["schemas.yaml"]));
        assert_eq!(config.output_paths, paths(&["paths.yaml"]));
        assert_eq!(config.output_fragments, paths(&["fragments.yaml"]));
        assert_eq!(config.output_json_schema, paths(&["schemas.json"]));
        assert_eq!(config.output_openapi_30, paths(&["openapi-3.0.yaml"]));
        assert_eq!(config.output_swagger2, paths(&["swagger.yaml"]));
        assert_eq!(config.output_asyncapi, paths(&["asyncapi.yaml"]));
        assert_eq!(config.outputs().count(), 9);
    }

    #[test]
    fn missing_fields_default_to_none() {
        let config = Config::from_toml_str("input = [\"src\"]").unwrap();
        assert_eq!(config.input, paths(&["src"]));
        assert!(config.output.is_none());
        assert!(config.include.is_none());
        assert_eq!(Config::from_toml_str("").unwrap(), Config::default());
    }

    #[test]
    fn rejects_unknown_keys_and_wrong_types() {
        for text in ["input = 5", "config_file = \"other.toml\"", "outputs = []"] {
            assert!(
                matches!(Config::from_toml_str(text), Err(Error::Config { .. })),
                "{text}"
            );
        }
    }
}

mod merging {
    use super::*;

    #[test]
    fn set_fields_replace_and_unset_fields_keep() {
        let mut base = Config::from_toml_str(ALL_FIELDS).unwrap();
        base.merge(Config::from_toml_str("output = [\"other.yaml\"]").unwrap());

        assert_eq!(base.output, paths(&["other.yaml"]));
        assert_eq!(base.input, paths(&["src", "lib"]));
    }

    #[test]
    fn outputs_lists_every_strategy() {
        let config = Config::from_toml_str(ALL_FIELDS).unwrap();
        let outputs: Vec<_> = config.outputs().cloned().collect();
        assert_eq!(
            outputs,
            [
                "openapi.yaml",
                "openapi.json",
                "schemas.yaml",
                "paths.yaml",
                "fragments.yaml",
                "schemas.json",
                "openapi-3.0.yaml",
                "swagger.yaml",
                "asyncapi.yaml"
            ]
            .map(PathBuf::from)
        );
    }
}

mod loading {
    use super::*;

    const MANIFEST: &str = r#"
[package]
name = "demo"
version = "0.1.0"

[package.metadata.oas-forge]
input = ["src"]
output = ["from-cargo.yaml"]
"#;

    #[test]
    fn empty_directory_yields_default() {
        let project = Project::new();
        assert_eq!(
            Config::load_from(project.root()).unwrap(),
            Config::default()
        );
    }

    #[test]
    fn reads_cargo_metadata() {
        let project = Project::new();
        project.file("Cargo.toml", MANIFEST);

        let config = Config::load_from(project.root()).unwrap();

        assert_eq!(config.input, paths(&["src"]));
        assert_eq!(config.output, paths(&["from-cargo.yaml"]));
    }

    #[test]
    fn manifest_without_metadata_is_fine() {
        let project = Project::new();
        project.file("Cargo.toml", "[package]\nname = \"demo\"\n");
        assert_eq!(
            Config::load_from(project.root()).unwrap(),
            Config::default()
        );
        project.file("Cargo.toml", "[workspace]\nmembers = []\n");
        assert_eq!(
            Config::load_from(project.root()).unwrap(),
            Config::default()
        );
    }

    #[test]
    fn openapi_toml_wins_over_cargo_metadata() {
        let project = Project::new();
        project
            .file("Cargo.toml", MANIFEST)
            .file("openapi.toml", "output = [\"from-openapi-toml.yaml\"]");

        let config = Config::load_from(project.root()).unwrap();

        assert_eq!(config.output, paths(&["from-openapi-toml.yaml"]));
        assert_eq!(config.input, paths(&["src"]));
    }

    #[test]
    fn broken_files_are_errors_naming_the_file() {
        for (file, content) in [("Cargo.toml", "[package\n"), ("openapi.toml", "input = 5")] {
            let project = Project::new();
            project.file(file, content);

            match Config::load_from(project.root()) {
                Err(Error::Config { file: path, .. }) => assert!(path.ends_with(file)),
                other => panic!("expected config error for {file}, got {other:?}"),
            }
        }
    }

    #[test]
    fn from_file_reports_missing_files() {
        let project = Project::new();
        let error = Config::from_file(project.path("missing.toml")).unwrap_err();
        assert!(error.to_string().contains("missing.toml"), "{error}");
    }

    #[test]
    fn load_uses_the_current_directory() {
        // The package root of this crate has no oas-forge metadata.
        assert_eq!(Config::load().unwrap(), Config::default());
    }
}

mod type_mappings {
    use super::*;
    use crate::common::ROOT_DOC;
    use oas_forge::Generator;
    use serde_json::json;

    #[test]
    fn types_are_read_as_schemas() {
        let config = Config::from_toml_str(
            "[types]\nUserId = { type = \"string\", format = \"uuid\" }\n\"geo::Point\" = { type = \"array\", maxItems = 2 }",
        )
        .unwrap();
        let types = config.types.unwrap();
        assert_eq!(
            types["UserId"],
            json!({ "type": "string", "format": "uuid" })
        );
        assert_eq!(
            types["geo::Point"],
            json!({ "type": "array", "maxItems": 2 })
        );
    }

    #[test]
    fn layers_merge_types_by_name() {
        let mut config =
            Config::from_toml_str("[types]\nA = { type = \"string\" }\nB = { type = \"string\" }")
                .unwrap();
        config.merge(Config::from_toml_str("[types]\nB = { type = \"integer\" }").unwrap());
        config.merge(Config::from_toml_str("input = [\"src\"]").unwrap());
        let types = config.types.unwrap();
        assert_eq!(types["A"], json!({ "type": "string" }));
        assert_eq!(types["B"], json!({ "type": "integer" }));
    }

    #[test]
    fn the_generator_uses_configured_types() {
        let project = Project::new();
        project
            .file("src/root.rs", ROOT_DOC)
            .file("src/a.rs", "/// @openapi\nstruct S { id: UserId }");
        let mut config =
            Config::from_toml_str("[types]\nUserId = { type = \"string\", format = \"uuid\" }")
                .unwrap();
        config.input = Some(vec![project.path("src")]);
        let spec = Generator::new().with_config(config).build().unwrap();
        assert_eq!(
            spec.document()["components"]["schemas"]["S"]["properties"]["id"],
            json!({ "type": "string", "format": "uuid" })
        );
    }

    #[test]
    fn variables_are_read_merged_and_used() {
        let mut config =
            Config::from_toml_str("[variables]\nSTAGE = \"dev\"\nREGION = \"eu\"").unwrap();
        config.merge(Config::from_toml_str("[variables]\nSTAGE = \"prod\"").unwrap());
        let variables = config.variables.clone().unwrap();
        assert_eq!(variables["STAGE"], "prod");
        assert_eq!(variables["REGION"], "eu");

        let project = Project::new();
        project.file(
            "src/root.rs",
            "/// @openapi\n/// openapi: 3.1.0\n/// info: { title: '{{STAGE}}-{{REGION}}', version: '1' }\nfn root() {}",
        );
        config.input = Some(vec![project.path("src")]);
        let spec = Generator::new()
            .var("REGION", "us")
            .with_config(config)
            .build()
            .unwrap();
        assert_eq!(
            spec.document()["info"]["title"],
            "prod-eu",
            "the config wins over earlier values"
        );
    }

    #[test]
    fn variables_must_be_strings() {
        assert!(Config::from_toml_str("[variables]\nN = 1").is_err());
    }
}
