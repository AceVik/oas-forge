use std::path::PathBuf;
use thiserror::Error;

#[derive(Error, Debug)]
pub enum Error {
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    #[error("YAML parsing error: {0}")]
    Yaml(#[from] serde_yaml_ng::Error),

    #[error("JSON parsing error: {0}")]
    Json(#[from] serde_json::Error),

    #[error("TOML parsing error: {0}")]
    Toml(#[from] toml::de::Error),

    #[error("Syntactic parsing error in file {file:?}: {source}")]
    Parse { file: PathBuf, source: syn::Error },

    #[error(
        "Validation failed: No Root OpenAPI definition found. One definition must contain 'openapi' and 'info' fields."
    )]
    NoRootFound,

    #[error(
        "Validation failed: Multiple Root OpenAPI definitions found. Only one definition can be the Root."
    )]
    MultipleRootsFound,

    #[error("Empty input: No files found in the specified directories.")]
    NoFilesFound,

    #[error("YAML error in {file}:{line}: {source}\nContext:\n{context}")]
    SourceMapped {
        file: PathBuf,
        line: usize,
        source: serde_yaml_ng::Error,
        context: String,
    },

    #[error("Invalid configuration in {}: {message}", file.display())]
    Config { file: PathBuf, message: String },
}

impl Error {
    pub(crate) fn config(file: &std::path::Path, message: impl Into<String>) -> Self {
        Self::Config {
            file: file.to_path_buf(),
            message: message.into(),
        }
    }
}

pub type Result<T> = std::result::Result<T, Error>;
