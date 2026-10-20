//! Argument parsing and configuration layering without spawning the binary.

mod support;

use clap::Parser;
use oas_forge_cli::{Cli, run};
use std::path::PathBuf;
use support::{Project, ROOT_DOC};

fn parse(args: &[&str]) -> Cli {
    Cli::try_parse_from(args).unwrap_or_else(|e| panic!("{e}"))
}

fn paths(items: &[&str]) -> Option<Vec<PathBuf>> {
    Some(items.iter().map(PathBuf::from).collect())
}

#[test]
fn short_and_long_flags() {
    let cli = parse(&[
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

    assert_eq!(cli.input, paths(&["src", "lib"]));
    assert_eq!(cli.include, paths(&["root.yaml", "security.json"]));
    assert_eq!(cli.output, paths(&["a.yaml", "b.json"]));
    assert_eq!(cli.output_schemas, paths(&["s.yaml"]));
    assert_eq!(cli.output_paths, paths(&["p.yaml"]));
    assert_eq!(cli.output_fragments, paths(&["f.yaml"]));
    assert_eq!(cli.config, Some(PathBuf::from("custom.toml")));
}

#[test]
fn out_is_an_alias_for_output() {
    assert_eq!(
        parse(&["oas-forge", "--out", "spec.yaml"]).output,
        paths(&["spec.yaml"])
    );
}

#[test]
fn everything_is_optional() {
    let cli = parse(&["oas-forge"]);
    assert!(cli.input.is_none());
    assert!(cli.output.is_none());
    assert!(cli.config.is_none());
}

#[test]
fn unknown_flags_are_rejected() {
    assert!(Cli::try_parse_from(["oas-forge", "--nope"]).is_err());
}

#[test]
fn default_output_only_without_any_configured_output() {
    let project = Project::new();
    let config = parse(&["oas-forge"])
        .resolve_config(project.root())
        .unwrap();
    assert_eq!(config.output, paths(&["openapi.yaml"]));

    let config = parse(&["oas-forge", "--output-paths", "p.yaml"])
        .resolve_config(project.root())
        .unwrap();
    assert!(config.output.is_none());
}

#[test]
fn flags_override_config_file() {
    let project = Project::new();
    project.file(
        "custom.toml",
        "output = [\"from-file.yaml\"]\ninput = [\"src\"]",
    );

    let config = parse(&["oas-forge", "--config", "custom.toml", "-o", "cli.yaml"])
        .resolve_config(project.root())
        .unwrap();

    assert_eq!(config.output, paths(&["cli.yaml"]));
    assert_eq!(config.input, paths(&["src"]));
}

#[test]
fn missing_config_file_is_an_error() {
    let project = Project::new();
    let error = parse(&["oas-forge", "--config", "missing.toml"])
        .resolve_config(project.root())
        .unwrap_err();
    assert!(error.to_string().contains("missing.toml"), "{error}");
}

#[test]
fn run_returns_every_written_file() {
    let project = Project::new();
    project.file("src/main.rs", ROOT_DOC);
    let src = project.path("src").display().to_string();
    let out = project.path("out.yaml").display().to_string();
    let paths_out = project.path("paths.yaml").display().to_string();

    let outcome = run(
        &parse(&[
            "oas-forge",
            "-i",
            &src,
            "-o",
            &out,
            "--output-paths",
            &paths_out,
        ]),
        project.root(),
    )
    .unwrap();

    assert_eq!(
        outcome.outputs,
        vec![PathBuf::from(&out), PathBuf::from(&paths_out)]
    );
    assert!(outcome.diagnostics.is_empty(), "{:?}", outcome.diagnostics);
    assert_eq!(project.read_yaml("out.yaml")["info"]["title"], "Test API");
}

#[test]
fn export_flags_reach_the_configuration() {
    let cli = parse(&[
        "oas-forge",
        "--output-json-schema",
        "s.json",
        "--output-openapi-30",
        "v30.yaml",
        "--output-swagger2",
        "v2.yaml",
        "--output-asyncapi",
        "events.yaml",
    ]);
    let project = Project::new();
    let config = cli.resolve_config(project.root()).unwrap();
    assert_eq!(config.output_json_schema, paths(&["s.json"]));
    assert_eq!(config.output_openapi_30, paths(&["v30.yaml"]));
    assert_eq!(config.output_swagger2, paths(&["v2.yaml"]));
    assert_eq!(config.output_asyncapi, paths(&["events.yaml"]));
    assert!(config.output.is_none(), "an export counts as an output");
}

#[test]
fn run_writes_exports() {
    let project = Project::new();
    project.file("src/main.rs", ROOT_DOC);
    let src = project.path("src").display().to_string();
    let v30 = project.path("v30.json").display().to_string();
    let v2 = project.path("v2.yaml").display().to_string();
    let outcome = run(
        &parse(&[
            "oas-forge",
            "-i",
            &src,
            "--output-openapi-30",
            &v30,
            "--output-swagger2",
            &v2,
        ]),
        project.root(),
    )
    .unwrap();
    assert_eq!(outcome.outputs.len(), 2);
    assert_eq!(project.read_yaml("v30.json")["openapi"], "3.0.3");
    assert_eq!(project.read_yaml("v2.yaml")["swagger"], "2.0");
}
