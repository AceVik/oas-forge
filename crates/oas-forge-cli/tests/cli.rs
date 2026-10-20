//! End-to-end tests of the `oas-forge` binary: exit codes, console output and
//! configuration layering (Cargo.toml < openapi.toml < --config < CLI flags).

mod support;

use std::process::{Command, Output};
use support::{Project, ROOT_DOC};

fn oas_forge(project: &Project, args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_oas-forge"))
        .args(args)
        .current_dir(project.root())
        .env("RUST_LOG", "off")
        .output()
        .expect("run oas-forge binary")
}

fn stdout(output: &Output) -> String {
    String::from_utf8_lossy(&output.stdout).into_owned()
}

fn stderr(output: &Output) -> String {
    String::from_utf8_lossy(&output.stderr).into_owned()
}

fn api_project() -> Project {
    let project = Project::new();
    project.file("src/main.rs", ROOT_DOC);
    project
}

mod usage {
    use super::*;

    #[test]
    fn help_lists_all_options() {
        let output = oas_forge(&Project::new(), &["--help"]);

        assert!(output.status.success());
        let help = stdout(&output);
        for flag in [
            "--input",
            "--include",
            "--output",
            "--output-schemas",
            "--output-paths",
            "--output-fragments",
            "--config",
        ] {
            assert!(help.contains(flag), "missing {flag} in:\n{help}");
        }
    }

    #[test]
    fn version_matches_the_crate() {
        let output = oas_forge(&Project::new(), &["--version"]);
        assert!(output.status.success());
        assert_eq!(
            stdout(&output).trim(),
            format!("oas-forge {}", env!("CARGO_PKG_VERSION"))
        );
    }

    #[test]
    fn unknown_flags_fail() {
        let output = oas_forge(&Project::new(), &["--bogus"]);
        assert_eq!(output.status.code(), Some(2));
        assert!(stderr(&output).contains("--bogus"));
    }
}

mod generation {
    use super::*;

    #[test]
    fn writes_the_requested_output() {
        let project = api_project();

        let output = oas_forge(&project, &["-i", "src", "-o", "openapi.yaml"]);

        assert!(output.status.success(), "{}", stderr(&output));
        assert!(stdout(&output).contains("SUCCESS:"));
        assert_eq!(
            project.read_yaml("openapi.yaml")["info"]["title"],
            "Test API"
        );
    }

    #[test]
    fn defaults_to_openapi_yaml_without_any_output() {
        // Regression: the documented default was only printed, never written.
        let project = api_project();

        let output = oas_forge(&project, &["-i", "src"]);

        assert!(output.status.success(), "{}", stderr(&output));
        assert_eq!(
            project.read_yaml("openapi.yaml")["info"]["title"],
            "Test API"
        );
        assert!(stdout(&output).contains("\"openapi.yaml\""));
    }

    #[test]
    fn empty_output_lists_count_as_unconfigured() {
        let project = api_project();
        project.file(
            "openapi.toml",
            "input = [\"src\"]\noutput = []\noutput_paths = []",
        );

        let output = oas_forge(&project, &[]);

        assert!(output.status.success(), "{}", stderr(&output));
        assert!(project.path("openapi.yaml").exists());
    }

    #[test]
    fn partial_outputs_work_without_full_spec() {
        let project = Project::new();
        project.file("src/models.rs", "/// @openapi\nstruct User { id: i32 }");

        let output = oas_forge(&project, &["-i", "src", "--output-schemas", "schemas.yaml"]);

        assert!(output.status.success(), "{}", stderr(&output));
        assert!(project.read_yaml("schemas.yaml")["User"].is_object());
        assert!(
            !project.path("openapi.yaml").exists(),
            "the default only applies when no output is configured"
        );
        assert!(!stdout(&output).contains("openapi.yaml"));
    }

    #[test]
    fn success_message_lists_every_written_file() {
        let project = api_project();

        let output = oas_forge(
            &project,
            &[
                "-i",
                "src",
                "-o",
                "full.yaml",
                "--output-schemas",
                "schemas.yaml",
                "--output-paths",
                "paths.yaml",
                "--output-fragments",
                "fragments.yaml",
            ],
        );

        assert!(output.status.success(), "{}", stderr(&output));
        let stdout = stdout(&output);
        for file in ["full.yaml", "schemas.yaml", "paths.yaml", "fragments.yaml"] {
            assert!(stdout.contains(file), "{file} missing in: {stdout}");
        }
    }

    #[test]
    fn include_has_a_short_flag() {
        let project = Project::new();
        project
            .file("src/routes.rs", "/// @route GET /x\nfn x() {}")
            .file(
                "static/root.yaml",
                "openapi: 3.1.0\ninfo: { title: Included, version: '1' }",
            );

        let output = oas_forge(&project, &["-i", "src", "-I", "static/root.yaml"]);

        assert!(output.status.success(), "{}", stderr(&output));
        let spec = project.read_yaml("openapi.yaml");
        assert_eq!(spec["info"]["title"], "Included");
        assert!(spec["paths"]["/x"].is_object());
    }

    #[test]
    fn failures_exit_non_zero_with_a_message() {
        let project = Project::new();
        project.file("src/routes.rs", "/// @route GET /x\nfn x() {}");

        let output = oas_forge(&project, &["-i", "src", "-o", "openapi.yaml"]);

        assert_eq!(output.status.code(), Some(1));
        assert!(stderr(&output).contains("ERROR:"));
        assert!(stderr(&output).contains("No Root OpenAPI definition found"));
        assert!(!project.path("openapi.yaml").exists());
    }

    #[test]
    fn version_placeholder_uses_the_callers_package_version() {
        let project = Project::new();
        project.file(
            "src/main.rs",
            "/// @openapi\n/// openapi: 3.1.0\n/// info: { title: T, version: '{{CARGO_PKG_VERSION}}' }\nfn root() {}",
        );
        let run = |version: Option<&str>| {
            let mut cmd = Command::new(env!("CARGO_BIN_EXE_oas-forge"));
            cmd.args(["-i", "src", "-o", "openapi.yaml"])
                .current_dir(project.root());
            match version {
                Some(v) => cmd.env("CARGO_PKG_VERSION", v),
                None => cmd.env_remove("CARGO_PKG_VERSION"),
            };
            assert!(cmd.output().unwrap().status.success());
            project.read_yaml("openapi.yaml")["info"]["version"].clone()
        };

        assert_eq!(run(Some("9.8.7")), "9.8.7");
        assert_eq!(run(None), "0.0.0");
    }
}

mod configuration {
    use super::*;

    const CARGO_TOML: &str = r#"
[package]
name = "demo"
version = "0.1.0"

[package.metadata.oas-forge]
input = ["src"]
output = ["from-cargo.yaml"]
"#;

    fn run_ok(project: &Project, args: &[&str]) {
        let output = oas_forge(project, args);
        assert!(output.status.success(), "{}", stderr(&output));
    }

    #[test]
    fn cargo_metadata_is_used_by_default() {
        let project = api_project();
        project.file("Cargo.toml", CARGO_TOML);

        run_ok(&project, &[]);

        assert!(project.path("from-cargo.yaml").exists());
    }

    #[test]
    fn openapi_toml_overrides_cargo_metadata() {
        let project = api_project();
        project
            .file("Cargo.toml", CARGO_TOML)
            .file("openapi.toml", "output = [\"from-openapi-toml.yaml\"]");

        run_ok(&project, &[]);

        assert!(project.path("from-openapi-toml.yaml").exists());
        assert!(!project.path("from-cargo.yaml").exists());
    }

    #[test]
    fn explicit_config_file_overrides_openapi_toml() {
        let project = api_project();
        project
            .file("Cargo.toml", CARGO_TOML)
            .file("openapi.toml", "output = [\"from-openapi-toml.yaml\"]")
            .file(
                "config/custom.toml",
                "output_schemas = [\"schemas.yaml\"]\noutput = [\"from-custom.yaml\"]",
            );

        run_ok(&project, &["--config", "config/custom.toml"]);

        assert!(project.path("from-custom.yaml").exists());
        assert!(project.path("schemas.yaml").exists());
        assert!(!project.path("from-openapi-toml.yaml").exists());
    }

    #[test]
    fn command_line_flags_override_every_file() {
        let project = api_project();
        project
            .file("Cargo.toml", CARGO_TOML)
            .file("openapi.toml", "output = [\"from-openapi-toml.yaml\"]")
            .file("custom.toml", "output = [\"from-custom.yaml\"]");

        run_ok(
            &project,
            &["--config", "custom.toml", "--out", "from-cli.yaml"],
        );

        assert!(project.path("from-cli.yaml").exists());
        for unused in [
            "from-cargo.yaml",
            "from-openapi-toml.yaml",
            "from-custom.yaml",
        ] {
            assert!(!project.path(unused).exists(), "{unused}");
        }
    }

    #[test]
    fn every_field_can_come_from_a_config_file() {
        let project = Project::new();
        project
            .file("api/routes.rs", "/// @route GET /x\nfn x() {}")
            .file(
                "static/root.yaml",
                "openapi: 3.1.0\ninfo: { title: Cfg, version: '1' }",
            )
            .file(
                "openapi.toml",
                r#"
input = ["api"]
include = ["static/root.yaml"]
output = ["out/full.yaml"]
output_schemas = ["out/schemas.yaml"]
output_paths = ["out/paths.yaml"]
output_fragments = ["out/fragments.yaml"]
"#,
            );

        run_ok(&project, &[]);

        assert_eq!(project.read_yaml("out/full.yaml")["info"]["title"], "Cfg");
        assert!(project.read_yaml("out/paths.yaml")["/x"].is_object());
        assert!(project.path("out/schemas.yaml").exists());
        assert!(project.path("out/fragments.yaml").exists());
    }
}

mod ci {
    use super::*;

    #[test]
    fn help_lists_the_new_options() {
        let help = stdout(&oas_forge(&Project::new(), &["--help"]));
        for flag in [
            "--output-json-schema",
            "--output-openapi-30",
            "--output-swagger2",
            "--output-asyncapi",
            "--var",
            "--check",
            "--deny-warnings",
            "--strict",
            "--message-format",
        ] {
            assert!(help.contains(flag), "missing {flag} in:\n{help}");
        }
    }

    #[test]
    fn check_fails_until_the_output_is_written() {
        let project = api_project();
        let outdated = oas_forge(&project, &["-i", "src", "-o", "openapi.yaml", "--check"]);
        assert!(!outdated.status.success());
        assert!(
            stderr(&outdated).contains("openapi.yaml is not up to date"),
            "{}",
            stderr(&outdated)
        );
        assert!(
            !project.path("openapi.yaml").exists(),
            "--check writes nothing"
        );

        assert!(
            oas_forge(&project, &["-i", "src", "-o", "openapi.yaml"])
                .status
                .success()
        );
        let current = oas_forge(&project, &["-i", "src", "-o", "openapi.yaml", "--check"]);
        assert!(current.status.success(), "{}", stderr(&current));
        assert!(stdout(&current).contains("up to date"));
    }

    #[test]
    fn deny_warnings_and_json_messages() {
        let project = api_project();
        project.file(
            "src/api.rs",
            "/// @route GET /x\n/// @frobnicate\nfn x() {}",
        );
        let lenient = oas_forge(
            &project,
            &["-i", "src", "-o", "a.yaml", "--message-format", "json"],
        );
        assert!(lenient.status.success());
        let line = stdout(&lenient)
            .lines()
            .next()
            .unwrap_or_default()
            .to_string();
        let finding: serde_json::Value = serde_json::from_str(&line).expect("JSON line");
        assert_eq!(finding["code"], "OF2006");
        assert_eq!(finding["severity"], "warning");
        assert_eq!(finding["line"], 2);
        assert!(finding["file"].as_str().unwrap().ends_with("api.rs"));

        let strict = oas_forge(
            &project,
            &[
                "-i",
                "src",
                "-o",
                "a.yaml",
                "--deny-warnings",
                "--message-format",
                "json",
            ],
        );
        assert!(!strict.status.success());
        let finding: serde_json::Value =
            serde_json::from_str(stdout(&strict).lines().next().unwrap()).unwrap();
        assert_eq!(finding["severity"], "error");

        let broken = oas_forge(
            &Project::new(),
            &["-i", "missing", "--message-format", "json"],
        );
        assert!(!broken.status.success());
        let finding: serde_json::Value = serde_json::from_str(stdout(&broken).trim()).unwrap();
        assert_eq!(finding["severity"], "error");
    }

    #[test]
    fn warnings_are_printed_with_code_and_location() {
        let project = api_project();
        project.file(
            "src/api.rs",
            "/// @route GET /x\n/// @frobnicate\nfn x() {}",
        );
        project.file(
            "src/twice.rs",
            "/// @route GET /a\n/// @operation-id same\nfn a() {}\n/// @route GET /b\n/// @operation-id same\nfn b() {}",
        );
        let output = oas_forge(&project, &["-i", "src", "-o", "a.yaml"]);
        assert!(output.status.success());
        let text = stderr(&output);
        assert!(
            text.contains("warning[OF2006]: unknown directive `@frobnicate` ("),
            "{text}"
        );
        assert!(text.contains("api.rs:2)"), "{text}");
        // Lints of the merged document have no location.
        assert!(
            text.contains("warning[OF5002]: operationId `same` is used by 2 operations\n"),
            "{text}"
        );
    }

    #[test]
    fn check_outdated_as_json() {
        let project = api_project();
        let output = oas_forge(
            &project,
            &[
                "-i",
                "src",
                "-o",
                "a.yaml",
                "--check",
                "--message-format",
                "json",
            ],
        );
        assert!(!output.status.success());
        let finding: serde_json::Value = serde_json::from_str(stdout(&output).trim()).unwrap();
        assert_eq!(finding["message"], "output is not up to date");
        assert_eq!(
            oas_forge(
                &project,
                &["-i", "src", "-o", "a.yaml", "--message-format", "json"]
            )
            .status
            .code(),
            Some(0)
        );
        let ok = oas_forge(
            &project,
            &[
                "-i",
                "src",
                "-o",
                "a.yaml",
                "--check",
                "--message-format",
                "json",
            ],
        );
        assert!(ok.status.success());
        assert!(stdout(&ok).trim().is_empty());
    }

    #[test]
    fn variables_and_the_package_root() {
        let project = Project::new();
        project
            .file("Cargo.toml", "[package]\nname = \"shop\"\nversion = \"2.1.0\"\ndescription = \"A shop\"\nlicense = \"MIT\"\n")
            .file("src/lib.rs", "/// @openapi\n/// x-env: {{STAGE}}\n/// x-name: '{{CARGO_PKG_NAME}}'\nstruct S { a: u8 }");
        let output = oas_forge(
            &project,
            &["-i", "src", "-o", "api.yaml", "--var", "STAGE=prod"],
        );
        assert!(output.status.success(), "{}", stderr(&output));
        let api = project.read_yaml("api.yaml");
        assert_eq!(api["info"]["title"], "shop");
        assert_eq!(api["info"]["version"], "2.1.0");
        assert_eq!(api["info"]["license"]["identifier"], "MIT");
        assert_eq!(api["components"]["schemas"]["S"]["x-env"], "prod");
        // From Cargo.toml, not from the environment `cargo test` runs in.
        assert_eq!(api["components"]["schemas"]["S"]["x-name"], "shop");
        let bad = oas_forge(&project, &["--var", "novalue"]);
        assert!(!bad.status.success());
        assert!(stderr(&bad).contains("NAME=VALUE"));
    }
}
