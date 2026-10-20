//! Helpers shared by all test suites (`unit`, `integration`, `corpus`, `property`).
//!
//! Each suite pulls this file in via `#[path = "../common/mod.rs"] mod common;`,
//! so not every helper is used by every suite.
#![allow(dead_code)]

pub mod spec;

use oas_forge::diagnostics::{Code, Diagnostics, Location};
use oas_forge::dsl::{self, RouteSource};
use oas_forge::extract::{self, Item};
use oas_forge::generator::generate_from_sources;
use oas_forge::source::{SourceFile, SourceKind};
use oas_forge::template::Variables;
use serde_json::Value;
use std::fs;
use std::path::{Path, PathBuf};
use tempfile::TempDir;

/// Parses YAML (or JSON) text into a `serde_json::Value` for structural
/// assertions, with the same YAML 1.2 boolean rules as the generator.
pub fn yaml(text: &str) -> Value {
    let mut options = serde_saphyr::Options::default();
    options.strict_booleans = true;
    serde_saphyr::from_str_with_options(text, options)
        .unwrap_or_else(|e| panic!("invalid YAML ({e}):\n{text}"))
}

/// The location used for in-memory test sources.
pub fn here() -> Location {
    Location::new("test.rs", 1)
}

/// Extracts the annotations of a Rust source text (panics on invalid Rust).
pub fn extract(code: &str) -> Vec<Item> {
    extract_with_diagnostics(code).0
}

/// Extracts the annotations of a Rust source text and returns the findings.
pub fn extract_with_diagnostics(code: &str) -> (Vec<Item>, Diagnostics) {
    let mut diagnostics = Diagnostics::new();
    let items = extract::extract_str(Path::new("test.rs"), code, &mut diagnostics)
        .unwrap_or_else(|e| panic!("test source must be valid Rust: {e}"));
    (items, diagnostics)
}

/// The schema of the schema item (or named YAML schema) called `name`.
pub fn schema(items: &[Item], name: &str) -> Value {
    items
        .iter()
        .find_map(|item| match item {
            Item::Schema(s) if s.name == name => Some(s.schema.clone()),
            Item::NamedSchema(s) if s.name == name => Some(yaml(&s.text)),
            _ => None,
        })
        .unwrap_or_else(|| panic!("no schema named `{name}` in {items:#?}"))
}

/// Names of all schema items (reflected and named YAML), in extraction order.
pub fn schema_names(items: &[Item]) -> Vec<String> {
    items
        .iter()
        .filter_map(|item| match item {
            Item::Schema(s) => Some(s.name.clone()),
            Item::NamedSchema(s) => Some(s.name.clone()),
            _ => None,
        })
        .collect()
}

/// The texts of all free YAML document items.
pub fn documents(items: &[Item]) -> Vec<String> {
    items
        .iter()
        .filter_map(|item| match item {
            Item::Document(d) => Some(d.text.clone()),
            _ => None,
        })
        .collect()
}

/// Converts string slices into owned lines.
pub fn lines(src: &[&str]) -> Vec<String> {
    src.iter().map(|l| l.to_string()).collect()
}

/// Compiles route DSL lines (operation id `op`); returns the document or
/// `None` together with the findings.
pub fn try_route(src: &[&str]) -> (Option<Value>, Diagnostics) {
    let mut diagnostics = Diagnostics::new();
    let lines = lines(src);
    let location = here();
    let compiled = dsl::compile(
        RouteSource {
            lines: &lines,
            line_numbers: &[],
            operation_id: Some("op"),
            default_tags: &[],
            location: &location,
        },
        &mut diagnostics,
    );
    (compiled.map(|c| c.document), diagnostics)
}

/// Compiles route DSL lines (operation id `op`) and returns the document.
pub fn route(src: &[&str]) -> Value {
    let (document, diagnostics) = try_route(src);
    document.unwrap_or_else(|| panic!("route DSL was rejected: {src:#?}\n{diagnostics:#?}"))
}

/// Compiles route DSL lines and returns the operation at `paths.<path>.<method>`.
pub fn operation(src: &[&str], path: &str, method: &str) -> Value {
    route(src)["paths"][path][method].clone()
}

/// Runs the whole pipeline on in-memory sources (`name`, `content`).
pub fn pipeline(files: &[(&str, &str)]) -> (Value, Diagnostics) {
    try_pipeline(files).unwrap_or_else(|e| panic!("{e}"))
}

/// Runs the whole pipeline on in-memory sources and returns fatal errors.
pub fn try_pipeline(files: &[(&str, &str)]) -> oas_forge::Result<(Value, Diagnostics)> {
    let sources = files
        .iter()
        .map(|(name, code)| {
            let kind = SourceKind::of(Path::new(name)).expect("supported extension");
            SourceFile::new(*name, kind, *code)
        })
        .collect();
    generate_from_sources(sources, &Variables::new())
}

/// Codes of the findings, in order.
pub fn codes(diagnostics: &Diagnostics) -> Vec<Code> {
    diagnostics.iter().map(|d| d.code()).collect()
}

/// A throw-away project directory for tests that touch the file system.
pub struct Project {
    dir: TempDir,
}

impl Project {
    pub fn new() -> Self {
        Self {
            dir: TempDir::new().expect("create temp dir"),
        }
    }

    pub fn root(&self) -> &Path {
        self.dir.path()
    }

    pub fn path(&self, rel: &str) -> PathBuf {
        self.dir.path().join(rel)
    }

    /// Writes `content` to `rel`, creating parent directories as needed.
    pub fn file(&self, rel: &str, content: &str) -> &Self {
        let path = self.path(rel);
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).expect("create parent dirs");
        }
        fs::write(&path, content).expect("write test file");
        self
    }

    pub fn dir(&self, rel: &str) -> &Self {
        fs::create_dir_all(self.path(rel)).expect("create dir");
        self
    }

    pub fn read(&self, rel: &str) -> String {
        fs::read_to_string(self.path(rel)).unwrap_or_else(|e| panic!("cannot read {rel}: {e}"))
    }

    /// Reads and parses a generated YAML/JSON file.
    pub fn read_yaml(&self, rel: &str) -> Value {
        yaml(&self.read(rel))
    }
}

/// A minimal root document so full-spec generation passes validation.
pub const ROOT_DOC: &str = r#"
/// @openapi
/// openapi: 3.1.0
/// info:
///   title: Test API
///   version: 1.0.0
fn root() {}
"#;
