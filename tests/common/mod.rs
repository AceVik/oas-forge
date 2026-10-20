//! Helpers shared by all test suites (`unit`, `integration`, `cli`, `property`).
//!
//! Each suite pulls this file in via `#[path = "../common/mod.rs"] mod common;`,
//! so not every helper is used by every suite.
#![allow(dead_code)]

pub mod spec;

use oas_forge::scanner::Snippet;
use oas_forge::visitor::{ExtractedItem, OpenApiVisitor};
use serde_json::Value;
use std::fs;
use std::path::{Path, PathBuf};
use syn::visit::Visit;
use tempfile::TempDir;

/// Routes `log` output into the test harness. Without a logger, `log` skips
/// evaluating message arguments, so call this in tests that hit logging paths.
pub fn init_logging() {
    let _ = env_logger::builder()
        .is_test(true)
        .filter_level(log::LevelFilter::Trace)
        .try_init();
}

/// Parses YAML (or JSON) text into a `serde_json::Value` for structural assertions.
pub fn yaml(text: &str) -> Value {
    serde_yaml_ng::from_str(text).unwrap_or_else(|e| panic!("invalid YAML ({e}):\n{text}"))
}

/// Builds a snippet as if it was extracted from line 1 of `test.rs`.
pub fn snippet(content: &str) -> Snippet {
    Snippet {
        content: content.to_string(),
        file_path: PathBuf::from("test.rs"),
        line_number: 1,
        operation_id: None,
    }
}

/// Runs the visitor over a complete Rust source file and returns the extracted items.
pub fn extract(code: &str) -> Vec<ExtractedItem> {
    let file: syn::File = syn::parse_str(code).expect("test source must be valid Rust");
    let mut visitor = OpenApiVisitor::default();
    visitor.visit_file(&file);
    visitor.items
}

/// Returns the raw content of the schema item called `name`.
pub fn schema_content<'a>(items: &'a [ExtractedItem], name: &str) -> &'a str {
    items
        .iter()
        .find_map(|item| match item {
            ExtractedItem::Schema {
                name: Some(n),
                content,
                ..
            } if n == name => Some(content.as_str()),
            _ => None,
        })
        .unwrap_or_else(|| panic!("no schema named `{name}` in {items:#?}"))
}

/// Returns `components.schemas.<name>` of the schema item called `name`.
pub fn schema(items: &[ExtractedItem], name: &str) -> Value {
    yaml(schema_content(items, name))["components"]["schemas"][name].clone()
}

/// Names of all named schema items, in extraction order.
pub fn schema_names(items: &[ExtractedItem]) -> Vec<String> {
    items
        .iter()
        .filter_map(|item| match item {
            ExtractedItem::Schema { name: Some(n), .. } => Some(n.clone()),
            _ => None,
        })
        .collect()
}

/// Converts string slices into the owned lines `parse_route_dsl` expects.
pub fn lines(src: &[&str]) -> Vec<String> {
    src.iter().map(|l| l.to_string()).collect()
}

/// Compiles route DSL lines (operation id `op`) and parses the generated YAML.
pub fn route(src: &[&str]) -> Value {
    let generated = oas_forge::dsl::parse_route_dsl(&lines(src), "op")
        .unwrap_or_else(|| panic!("route DSL was rejected: {src:#?}"));
    yaml(&generated)
}

/// Compiles route DSL lines and returns the single operation at `paths.<path>.<method>`.
pub fn operation(src: &[&str], path: &str, method: &str) -> Value {
    route(src)["paths"][path][method].clone()
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
