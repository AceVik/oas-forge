//! Errors that abort a generator run.
//!
//! Recoverable problems are [`Diagnostic`]s instead; a run fails with
//! [`Error::Diagnostics`] when at least one diagnostic has error severity.

use super::diagnostics::{Diagnostic, Location};
use std::fmt;
use std::path::{Path, PathBuf};

/// Why a generator run failed.
#[derive(Debug)]
#[non_exhaustive]
pub enum Error {
    /// A file or directory could not be read or written.
    Io {
        /// The path that failed.
        path: PathBuf,
        /// The underlying error.
        source: std::io::Error,
    },
    /// A configuration file is unreadable or invalid.
    Config {
        /// The configuration file.
        file: PathBuf,
        /// What is wrong.
        message: String,
    },
    /// A Rust source file is not valid Rust.
    Parse {
        /// Where parsing failed.
        location: Location,
        /// The parser message.
        message: String,
    },
    /// A YAML or JSON snippet is invalid.
    Yaml {
        /// Where the snippet starts (or the exact error position, if known).
        location: Location,
        /// The parser message.
        message: String,
        /// The first lines of the snippet, numbered.
        context: String,
    },
    /// The full specification needs a root document with `openapi` and `info`.
    NoRootFound,
    /// More than one document declares `openapi` and `info`.
    MultipleRootsFound {
        /// Where the root documents were defined.
        locations: Vec<Location>,
    },
    /// The input directories contain no files at all.
    NoFilesFound,
    /// No output was configured.
    NoOutput,
    /// Generation failed because of error-level diagnostics.
    Diagnostics(Vec<Diagnostic>),
}

impl Error {
    pub(crate) fn io(path: &Path, source: std::io::Error) -> Self {
        Self::Io {
            path: path.to_path_buf(),
            source,
        }
    }

    pub(crate) fn config(file: &Path, message: impl Into<String>) -> Self {
        Self::Config {
            file: file.to_path_buf(),
            message: message.into(),
        }
    }
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io { path, source } => write!(f, "IO error at {}: {source}", path.display()),
            Self::Config { file, message } => {
                write!(f, "Invalid configuration in {}: {message}", file.display())
            }
            Self::Parse { location, message } => {
                write!(f, "Rust syntax error in {location}: {message}")
            }
            Self::Yaml {
                location,
                message,
                context,
            } => write!(f, "YAML error in {location}: {message}\nContext:\n{context}"),
            Self::NoRootFound => f.write_str(
                "Validation failed: No Root OpenAPI definition found. One definition must contain 'openapi' and 'info' fields.",
            ),
            Self::MultipleRootsFound { locations } => {
                f.write_str(
                    "Validation failed: Multiple Root OpenAPI definitions found. Only one definition can be the Root.",
                )?;
                for location in locations {
                    write!(f, "\n  root defined at {location}")?;
                }
                Ok(())
            }
            Self::NoFilesFound => {
                f.write_str("Empty input: No files found in the specified directories.")
            }
            Self::NoOutput => f.write_str(
                "At least one output path (output, output_schemas, output_paths, or output_fragments) is required",
            ),
            Self::Diagnostics(diagnostics) => {
                let errors = diagnostics.iter().filter(|d| d.is_error()).count();
                write!(f, "Generation failed with {errors} error(s):")?;
                for diagnostic in diagnostics.iter().filter(|d| d.is_error()) {
                    write!(f, "\n  {diagnostic}")?;
                }
                Ok(())
            }
        }
    }
}

impl std::error::Error for Error {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Io { source, .. } => Some(source),
            _ => None,
        }
    }
}

/// Result of fallible `oas-forge` operations.
pub type Result<T> = std::result::Result<T, Error>;
