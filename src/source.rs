//! Discovery of input files.
//!
//! Input directories are walked recursively in sorted order, so the result does
//! not depend on the file system. Rust files and YAML/JSON files are read; all
//! other files are ignored. Files that are configured as outputs are never read
//! back as inputs.

use super::diagnostics::{Code, Diagnostic, Diagnostics, Location};
use super::error::{Error, Result};
use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

/// Default upper bound for a single input file (16 MiB).
pub const DEFAULT_MAX_FILE_SIZE: u64 = 16 * 1024 * 1024;

/// The kind of an input file, decided by its extension.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[non_exhaustive]
pub enum SourceKind {
    /// A Rust source file (`.rs`).
    Rust,
    /// A YAML document (`.yaml`, `.yml`).
    Yaml,
    /// A JSON document (`.json`).
    Json,
}

impl SourceKind {
    /// The kind of `path`, or `None` for files that are not read.
    pub fn of(path: &Path) -> Option<Self> {
        match path.extension()?.to_str()? {
            "rs" => Some(Self::Rust),
            "yaml" | "yml" => Some(Self::Yaml),
            "json" => Some(Self::Json),
            _ => None,
        }
    }
}

/// One input file and its content.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SourceFile {
    path: PathBuf,
    kind: SourceKind,
    text: String,
}

impl SourceFile {
    /// An in-memory source, e.g. for tests or generated code.
    pub fn new(path: impl Into<PathBuf>, kind: SourceKind, text: impl Into<String>) -> Self {
        Self {
            path: path.into(),
            kind,
            text: text.into(),
        }
    }

    /// The path the file was read from.
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// The kind of file.
    pub fn kind(&self) -> SourceKind {
        self.kind
    }

    /// The file content.
    pub fn text(&self) -> &str {
        &self.text
    }

    /// The location of `line` (1-based) in this file.
    pub fn location(&self, line: usize) -> Location {
        Location::new(&self.path, line)
    }
}

/// Options of [`SourceSet::discover`].
#[derive(Debug, Clone)]
#[non_exhaustive]
pub struct DiscoverOptions {
    /// Files that must not be read (the generator's own outputs).
    pub exclude: Vec<PathBuf>,
    /// Files larger than this are reported and skipped.
    pub max_file_size: u64,
}

impl Default for DiscoverOptions {
    fn default() -> Self {
        Self {
            exclude: Vec::new(),
            max_file_size: DEFAULT_MAX_FILE_SIZE,
        }
    }
}

impl DiscoverOptions {
    /// Sets the files that must not be read.
    pub fn with_exclude(mut self, exclude: Vec<PathBuf>) -> Self {
        self.exclude = exclude;
        self
    }

    /// Sets the size limit per file in bytes.
    pub fn with_max_file_size(mut self, bytes: u64) -> Self {
        self.max_file_size = bytes;
        self
    }
}

/// All input files of a run, in processing order.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SourceSet {
    files: Vec<SourceFile>,
    watched: Vec<PathBuf>,
}

impl SourceSet {
    /// A set of in-memory sources (processed in the given order).
    pub fn from_files(files: Vec<SourceFile>) -> Self {
        let watched = files.iter().map(|f| f.path.clone()).collect();
        Self { files, watched }
    }

    /// Reads every supported file below `inputs` (recursively, sorted) and every
    /// existing file of `includes` (in the given order).
    ///
    /// # Errors
    /// [`Error::Io`] when an input directory cannot be walked or a file cannot
    /// be read, [`Error::NoFilesFound`] when there is no file at all.
    pub fn discover(
        inputs: &[PathBuf],
        includes: &[PathBuf],
        options: &DiscoverOptions,
        diagnostics: &mut Diagnostics,
    ) -> Result<Self> {
        let excluded: BTreeSet<PathBuf> = options.exclude.iter().map(|p| normalize(p)).collect();
        let mut candidates = Vec::new();
        let mut any_file = false;
        let mut watched = Vec::new();

        for root in inputs {
            watched.push(root.clone());
            for entry in walkdir::WalkDir::new(root).sort_by_file_name() {
                let entry = entry.map_err(|e| {
                    let path = e.path().unwrap_or(root).to_path_buf();
                    Error::io(&path, std::io::Error::other(e))
                })?;
                if entry.file_type().is_file() {
                    any_file = true;
                    candidates.push(entry.into_path());
                }
            }
        }
        for include in includes {
            if include.is_file() {
                any_file = true;
                watched.push(include.clone());
                candidates.push(include.clone());
            } else {
                diagnostics.push(Diagnostic::new(
                    Code::MissingInclude,
                    format!("include file {} does not exist", include.display()),
                ));
            }
        }
        if !any_file {
            return Err(Error::NoFilesFound);
        }

        let mut files = Vec::new();
        for path in candidates {
            let Some(kind) = SourceKind::of(&path) else {
                continue;
            };
            if excluded.contains(&normalize(&path)) {
                diagnostics.push(
                    Diagnostic::new(
                        Code::OutputNotRead,
                        "file is a configured output and is not read as input",
                    )
                    .at(Location::new(&path, 1)),
                );
                continue;
            }
            let size = std::fs::metadata(&path)
                .map_err(|e| Error::io(&path, e))?
                .len();
            if size > options.max_file_size {
                diagnostics.push(
                    Diagnostic::new(
                        Code::FileTooLarge,
                        format!(
                            "file has {size} bytes, more than the limit of {}",
                            options.max_file_size
                        ),
                    )
                    .at(Location::new(&path, 1)),
                );
                continue;
            }
            let text = std::fs::read_to_string(&path).map_err(|e| Error::io(&path, e))?;
            files.push(SourceFile { path, kind, text });
        }
        Ok(Self { files, watched })
    }

    /// The files in processing order.
    pub fn files(&self) -> &[SourceFile] {
        &self.files
    }

    /// Directories and files whose changes affect the result
    /// (for `cargo:rerun-if-changed`).
    pub fn watched(&self) -> &[PathBuf] {
        &self.watched
    }
}

/// A comparable form of a path that does not need the file to exist.
fn normalize(path: &Path) -> PathBuf {
    if let Ok(canonical) = path.canonicalize() {
        return canonical;
    }
    match (path.parent(), path.file_name()) {
        (Some(parent), Some(name)) if !parent.as_os_str().is_empty() => parent
            .canonicalize()
            .map(|p| p.join(name))
            .unwrap_or_else(|_| path.to_path_buf()),
        _ => std::env::current_dir()
            .map(|d| d.join(path))
            .unwrap_or_else(|_| path.to_path_buf()),
    }
}
