use oas_forge::diagnostics::{Code, Diagnostic, Location};
use oas_forge::error::Error;
use std::error::Error as _;
use std::path::PathBuf;

#[test]
fn messages_name_the_problem_and_its_location() {
    let io = Error::Io {
        path: PathBuf::from("out/spec.yaml"),
        source: std::io::Error::other("disk full"),
    };
    assert_eq!(io.to_string(), "IO error at out/spec.yaml: disk full");
    assert!(io.source().is_some());

    let config = Error::Config {
        file: PathBuf::from("openapi.toml"),
        message: "bad".into(),
    };
    assert_eq!(
        config.to_string(),
        "Invalid configuration in openapi.toml: bad"
    );
    assert!(config.source().is_none());

    let parse = Error::Parse {
        location: Location::new("src/lib.rs", 3).with_column(7),
        message: "expected `;`".into(),
    };
    assert_eq!(
        parse.to_string(),
        "Rust syntax error in src/lib.rs:3:7: expected `;`"
    );

    let yaml = Error::Yaml {
        location: Location::new("src/api.rs", 12),
        message: "mapping values are not allowed".into(),
        context: "    12 | a: : b".into(),
    };
    assert_eq!(
        yaml.to_string(),
        "YAML error in src/api.rs:12: mapping values are not allowed\nContext:\n    12 | a: : b"
    );
}

#[test]
fn validation_errors() {
    assert!(
        Error::NoRootFound
            .to_string()
            .contains("must contain 'openapi' and 'info'")
    );
    assert_eq!(
        Error::NoFilesFound.to_string(),
        "Empty input: No files found in the specified directories."
    );
    let no_output = Error::NoOutput.to_string();
    assert!(
        no_output.contains("At least one output path"),
        "{no_output}"
    );
    for kind in ["output_json_schema", "output_swagger2", "output_asyncapi"] {
        assert!(no_output.contains(kind), "{no_output}");
    }

    let roots = Error::MultipleRootsFound {
        locations: vec![Location::new("a.rs", 1), Location::new("b.rs", 2)],
    };
    assert_eq!(
        roots.to_string(),
        "Validation failed: Multiple Root OpenAPI definitions found. Only one definition can be the Root.\n  root defined at a.rs:1\n  root defined at b.rs:2"
    );
}

#[test]
fn diagnostics_error_lists_only_errors() {
    let error = Error::Diagnostics(vec![
        Diagnostic::new(Code::InvalidRoute, "no path").at(Location::new("a.rs", 4)),
        Diagnostic::new(Code::UnknownFragment, "just a warning"),
    ]);
    assert_eq!(
        error.to_string(),
        "Generation failed with 1 error(s):\n  error[OF2001]: no path (a.rs:4)"
    );
}
