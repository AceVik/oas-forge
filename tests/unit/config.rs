//! `Config` parsing. Layered loading (`Config::load`) reads the real process
//! arguments and working directory, so it is covered by the `cli` suite.

use oas_forge::config::Config;
use std::path::PathBuf;

fn paths(items: &[&str]) -> Option<Vec<PathBuf>> {
    Some(items.iter().map(PathBuf::from).collect())
}

mod toml_file {
    use super::*;

    #[test]
    fn reads_all_fields() {
        let config: Config = toml::from_str(
            r#"
            input = ["src", "lib"]
            include = ["static/root.yaml"]
            output = ["openapi.yaml", "openapi.json"]
            output_schemas = ["schemas.yaml"]
            output_paths = ["paths.yaml"]
            output_fragments = ["fragments.yaml"]
            "#,
        )
        .unwrap();

        assert_eq!(config.input, paths(&["src", "lib"]));
        assert_eq!(config.include, paths(&["static/root.yaml"]));
        assert_eq!(config.output, paths(&["openapi.yaml", "openapi.json"]));
        assert_eq!(config.output_schemas, paths(&["schemas.yaml"]));
        assert_eq!(config.output_paths, paths(&["paths.yaml"]));
        assert_eq!(config.output_fragments, paths(&["fragments.yaml"]));
    }

    #[test]
    fn missing_fields_default_to_none() {
        let config: Config = toml::from_str("input = [\"src\"]").unwrap();
        assert_eq!(config.input, paths(&["src"]));
        assert!(config.output.is_none());
        assert!(config.include.is_none());
    }

    #[test]
    fn config_file_cannot_be_set_from_toml() {
        let config: Config = toml::from_str("config_file = \"other.toml\"").unwrap();
        assert!(config.config_file.is_none());
    }

    #[test]
    fn rejects_wrong_value_types() {
        assert!(toml::from_str::<Config>("input = 5").is_err());
    }
}

#[cfg(feature = "cli")]
mod command_line {
    use super::*;
    use clap::Parser;

    /// `parse_from` would exit the test process on invalid arguments.
    fn parse(args: &[&str]) -> Config {
        Config::try_parse_from(args).unwrap_or_else(|e| panic!("{e}"))
    }

    #[test]
    fn short_and_long_flags() {
        let config = parse(&[
            "oas-forge",
            "-i",
            "src",
            "--input",
            "lib",
            "--include",
            "root.yaml",
            "-I",
            "security.json",
            "-o",
            "a.yaml",
            "--output",
            "b.json",
            "--output-schemas",
            "s.yaml",
            "--output-paths",
            "p.yaml",
            "--output-fragments",
            "f.yaml",
            "--config",
            "custom.toml",
        ]);

        assert_eq!(config.input, paths(&["src", "lib"]));
        assert_eq!(config.include, paths(&["root.yaml", "security.json"]));
        assert_eq!(config.output, paths(&["a.yaml", "b.json"]));
        assert_eq!(config.output_schemas, paths(&["s.yaml"]));
        assert_eq!(config.output_paths, paths(&["p.yaml"]));
        assert_eq!(config.output_fragments, paths(&["f.yaml"]));
        assert_eq!(config.config_file, Some(PathBuf::from("custom.toml")));
    }

    #[test]
    fn out_is_an_alias_for_output() {
        let config = parse(&["oas-forge", "--out", "spec.yaml"]);
        assert_eq!(config.output, paths(&["spec.yaml"]));
    }

    #[test]
    fn everything_is_optional() {
        let config = parse(&["oas-forge"]);
        assert!(config.input.is_none());
        assert!(config.output.is_none());
        assert!(config.config_file.is_none());
    }

    #[test]
    fn unknown_flags_are_rejected() {
        assert!(Config::try_parse_from(["oas-forge", "--nope"]).is_err());
    }
}
