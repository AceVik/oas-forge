use oas_forge::error::{Error, Result};
use std::error::Error as _;
use std::path::PathBuf;

fn yaml_error() -> serde_yaml_ng::Error {
    serde_yaml_ng::from_str::<serde_yaml_ng::Value>("a: : b").unwrap_err()
}

#[test]
fn conversions_from_library_errors() {
    fn io() -> Result<()> {
        Err(std::io::Error::other("disk full"))?
    }
    fn yaml() -> Result<()> {
        Err(yaml_error())?
    }
    fn json() -> Result<()> {
        Err(serde_json::from_str::<serde_json::Value>("{").unwrap_err())?
    }
    fn toml() -> Result<()> {
        Err(toml::from_str::<toml::Value>("= 1").unwrap_err())?
    }

    assert!(matches!(io(), Err(Error::Io(_))));
    assert!(matches!(yaml(), Err(Error::Yaml(_))));
    assert!(matches!(json(), Err(Error::Json(_))));
    assert!(matches!(toml(), Err(Error::Toml(_))));
}

#[test]
fn messages_name_the_failure() {
    let io = Error::from(std::io::Error::other("disk full"));
    assert_eq!(io.to_string(), "IO error: disk full");
    assert!(io.source().is_some());

    assert!(
        Error::from(yaml_error())
            .to_string()
            .starts_with("YAML parsing error: ")
    );

    assert!(
        Error::NoRootFound
            .to_string()
            .contains("must contain 'openapi' and 'info'")
    );
    assert!(
        Error::MultipleRootsFound
            .to_string()
            .contains("Multiple Root")
    );
    assert_eq!(
        Error::NoFilesFound.to_string(),
        "Empty input: No files found in the specified directories."
    );
}

#[test]
fn parse_error_names_the_file() {
    let source = syn::parse_str::<syn::File>("fn {").unwrap_err();
    let error = Error::Parse {
        file: PathBuf::from("src/lib.rs"),
        source,
    };
    assert!(
        error
            .to_string()
            .starts_with("Syntactic parsing error in file \"src/lib.rs\": ")
    );
}

#[test]
fn source_mapped_error_shows_location_and_context() {
    let error = Error::SourceMapped {
        file: PathBuf::from("src/api.rs"),
        line: 12,
        source: yaml_error(),
        context: "    12 | a: : b".to_string(),
    };
    let message = error.to_string();
    assert!(
        message.starts_with("YAML error in src/api.rs:12: "),
        "{message}"
    );
    assert!(
        message.ends_with("\nContext:\n    12 | a: : b"),
        "{message}"
    );
}
