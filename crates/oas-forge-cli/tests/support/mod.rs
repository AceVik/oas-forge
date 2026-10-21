//! Helpers shared by the CLI test targets.
#![allow(dead_code)]

use serde_json::Value;
use std::fs;
use std::path::{Path, PathBuf};
use tempfile::TempDir;

/// A minimal root document so full-spec generation passes validation.
pub const ROOT_DOC: &str = r#"
/// @openapi
/// openapi: 3.1.0
/// info:
///   title: Test API
///   version: 1.0.0
fn root() {}
"#;

/// A throw-away project directory.
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

    /// Reads and parses a generated YAML/JSON file.
    pub fn read_yaml(&self, rel: &str) -> Value {
        let text = fs::read_to_string(self.path(rel)).expect("read output");
        let mut options = serde_saphyr::Options::default();
        options.strict_booleans = true;
        serde_saphyr::from_str_with_options(&text, options).expect("valid YAML")
    }
}
