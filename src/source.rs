//! Discovery of input files.
//!
//! Input directories are walked recursively in sorted order, so the result does
//! not depend on the file system. Rust files, YAML/JSON files and Protocol
//! Buffers definitions (`.proto`) are read; all other files are ignored. Files that are configured as outputs are never read
//! back as inputs.

use super::diagnostics::{Code, Diagnostic, Diagnostics, Location};
use super::error::{Error, Result};
use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

/// Default upper bound for a single input file (16 MiB).
pub const DEFAULT_MAX_FILE_SIZE: u64 = 16 * 1024 * 1024;

/// The kind of an input file, decided by its extension.
///
/// # Examples
///
/// ```
/// use oas_forge::source::SourceKind;
/// use std::path::Path;
///
/// assert_eq!(SourceKind::of(Path::new("src/api.rs")), Some(SourceKind::Rust));
/// assert_eq!(SourceKind::of(Path::new("openapi/root.yml")), Some(SourceKind::Yaml));
/// assert_eq!(SourceKind::of(Path::new("proto/chat.proto")), Some(SourceKind::Proto));
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[non_exhaustive]
pub enum SourceKind {
    /// A Rust source file (`.rs`).
    Rust,
    /// A YAML document (`.yaml`, `.yml`).
    Yaml,
    /// A JSON document (`.json`).
    Json,
    /// A Protocol Buffers definition (`.proto`), for `proto(package.Message)`
    /// payloads of AsyncAPI messages.
    Proto,
}

impl SourceKind {
    /// The kind of `path`, or `None` for files that are not read.
    ///
    /// # Examples
    ///
    /// ```
    /// use oas_forge::source::SourceKind;
    /// use std::path::Path;
    ///
    /// assert_eq!(SourceKind::of(Path::new("components.json")), Some(SourceKind::Json));
    /// assert_eq!(SourceKind::of(Path::new("README.md")), None);
    /// assert_eq!(SourceKind::of(Path::new("Makefile")), None);
    /// ```
    pub fn of(path: &Path) -> Option<Self> {
        match path.extension()?.to_str()? {
            "rs" => Some(Self::Rust),
            "yaml" | "yml" => Some(Self::Yaml),
            "json" => Some(Self::Json),
            "proto" => Some(Self::Proto),
            _ => None,
        }
    }
}

/// One input file and its content.
///
/// # Examples
///
/// ```
/// use oas_forge::source::{SourceFile, SourceKind};
///
/// let file = SourceFile::new("api/root.yaml", SourceKind::Yaml, "openapi: 3.1.0\n");
/// assert_eq!(file.kind(), SourceKind::Yaml);
/// assert_eq!(file.location(1).to_string(), "api/root.yaml:1");
/// ```
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SourceFile {
    /// The path as found or given; findings show it as is.
    path: PathBuf,
    /// How the content is read.
    kind: SourceKind,
    /// The whole content (UTF-8).
    text: String,
}

impl SourceFile {
    /// An in-memory source, e.g. for tests or generated code. `kind` is not
    /// checked against the extension of `path`.
    ///
    /// # Examples
    ///
    /// ```
    /// use oas_forge::source::{SourceFile, SourceKind};
    ///
    /// let file = SourceFile::new("generated.rs", SourceKind::Rust, "pub struct Tag;");
    /// assert_eq!(file.text(), "pub struct Tag;");
    /// ```
    pub fn new(path: impl Into<PathBuf>, kind: SourceKind, text: impl Into<String>) -> Self {
        Self {
            path: path.into(),
            kind,
            text: text.into(),
        }
    }

    /// The path the file was read from.
    ///
    /// # Examples
    ///
    /// ```
    /// use oas_forge::source::{SourceFile, SourceKind};
    /// use std::path::Path;
    ///
    /// let file = SourceFile::new("src/lib.rs", SourceKind::Rust, "");
    /// assert_eq!(file.path(), Path::new("src/lib.rs"));
    /// ```
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// The kind of file.
    ///
    /// # Examples
    ///
    /// ```
    /// use oas_forge::source::{SourceFile, SourceKind};
    ///
    /// let file = SourceFile::new("schemas.json", SourceKind::Json, "{}");
    /// assert_eq!(file.kind(), SourceKind::Json);
    /// ```
    pub fn kind(&self) -> SourceKind {
        self.kind
    }

    /// The file content.
    ///
    /// # Examples
    ///
    /// ```
    /// use oas_forge::source::{SourceFile, SourceKind};
    ///
    /// let file = SourceFile::new("root.yaml", SourceKind::Yaml, "openapi: 3.1.0\n");
    /// assert_eq!(file.text().lines().count(), 1);
    /// ```
    pub fn text(&self) -> &str {
        &self.text
    }

    /// The location of `line` (1-based) in this file.
    ///
    /// # Examples
    ///
    /// ```
    /// use oas_forge::Location;
    /// use oas_forge::source::{SourceFile, SourceKind};
    ///
    /// let file = SourceFile::new("src/lib.rs", SourceKind::Rust, "");
    /// assert_eq!(file.location(7), Location::new("src/lib.rs", 7));
    /// ```
    pub fn location(&self, line: usize) -> Location {
        Location::new(&self.path, line)
    }
}

/// Options of [`SourceSet::discover`].
///
/// # Examples
///
/// ```
/// use oas_forge::source::DiscoverOptions;
/// use std::path::PathBuf;
///
/// let options = DiscoverOptions::default()
///     .with_exclude(vec![PathBuf::from("openapi.yaml")])
///     .with_max_file_size(1024 * 1024);
/// assert_eq!(options.max_file_size, 1024 * 1024);
/// ```
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
    /// Sets the files that must not be read (a skipped file is reported as
    /// [`Code::OutputNotRead`]).
    ///
    /// # Examples
    ///
    /// ```
    /// use oas_forge::Code;
    /// use oas_forge::diagnostics::Diagnostics;
    /// use oas_forge::source::{DiscoverOptions, SourceSet};
    ///
    /// let dir = tempfile::tempdir()?;
    /// std::fs::write(dir.path().join("lib.rs"), "pub struct Tag;")?;
    /// std::fs::write(dir.path().join("openapi.yaml"), "openapi: 3.1.0")?;
    ///
    /// let options = DiscoverOptions::default().with_exclude(vec![dir.path().join("openapi.yaml")]);
    /// let mut diagnostics = Diagnostics::new();
    /// let set = SourceSet::discover(&[dir.path().to_path_buf()], &[], &options, &mut diagnostics)?;
    /// assert_eq!(set.files().len(), 1);
    /// assert_eq!(diagnostics.iter().next().map(|d| d.code()), Some(Code::OutputNotRead));
    /// # Ok::<(), Box<dyn std::error::Error>>(())
    /// ```
    pub fn with_exclude(mut self, exclude: Vec<PathBuf>) -> Self {
        self.exclude = exclude;
        self
    }

    /// Sets the size limit per file in bytes; a larger file is skipped with
    /// an error ([`Code::FileTooLarge`]).
    ///
    /// # Examples
    ///
    /// ```
    /// use oas_forge::Code;
    /// use oas_forge::diagnostics::Diagnostics;
    /// use oas_forge::source::{DiscoverOptions, SourceSet};
    ///
    /// let dir = tempfile::tempdir()?;
    /// std::fs::write(dir.path().join("lib.rs"), "pub struct Tag { pub name: String }")?;
    ///
    /// let options = DiscoverOptions::default().with_max_file_size(8);
    /// let mut diagnostics = Diagnostics::new();
    /// let set = SourceSet::discover(&[dir.path().to_path_buf()], &[], &options, &mut diagnostics)?;
    /// assert!(set.files().is_empty());
    /// assert_eq!(diagnostics.iter().next().map(|d| d.code()), Some(Code::FileTooLarge));
    /// # Ok::<(), Box<dyn std::error::Error>>(())
    /// ```
    pub fn with_max_file_size(mut self, bytes: u64) -> Self {
        self.max_file_size = bytes;
        self
    }
}

/// All input files of a run, in processing order.
///
/// # Examples
///
/// ```
/// use oas_forge::source::{SourceFile, SourceKind, SourceSet};
///
/// let set = SourceSet::from_files(vec![
///     SourceFile::new("b.rs", SourceKind::Rust, ""),
///     SourceFile::new("a.yaml", SourceKind::Yaml, ""),
/// ]);
/// let paths: Vec<_> = set.files().iter().map(|f| f.path().to_path_buf()).collect();
/// assert_eq!(paths, ["b.rs", "a.yaml"].map(std::path::PathBuf::from));
/// ```
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SourceSet {
    /// The files in processing order; each file at most once.
    files: Vec<SourceFile>,
    /// The input directories and the existing include files (for in-memory
    /// sets, the file paths), for `cargo:rerun-if-changed`.
    watched: Vec<PathBuf>,
}

impl SourceSet {
    /// A set of in-memory sources (processed in the given order).
    ///
    /// # Examples
    ///
    /// ```
    /// use oas_forge::source::{SourceFile, SourceKind, SourceSet};
    /// use std::path::Path;
    ///
    /// let set = SourceSet::from_files(vec![SourceFile::new("lib.rs", SourceKind::Rust, "")]);
    /// assert_eq!(set.files().len(), 1);
    /// assert_eq!(set.watched(), [Path::new("lib.rs")]);
    /// ```
    pub fn from_files(files: Vec<SourceFile>) -> Self {
        let watched = files.iter().map(|f| f.path.clone()).collect();
        Self { files, watched }
    }

    /// Reads every supported file below `inputs` (recursively, sorted) and every
    /// existing file of `includes` (in the given order). Symbolic links are
    /// followed; broken links and loops are reported and skipped. A file that
    /// is reached more than once is read once.
    ///
    /// # Errors
    /// [`Error::Io`] when an input directory cannot be walked or a file cannot
    /// be read, [`Error::NoFilesFound`] when there is no file at all.
    ///
    /// # Examples
    ///
    /// ```
    /// use oas_forge::diagnostics::Diagnostics;
    /// use oas_forge::source::{DiscoverOptions, SourceKind, SourceSet};
    ///
    /// let dir = tempfile::tempdir()?;
    /// std::fs::create_dir(dir.path().join("api"))?;
    /// std::fs::write(dir.path().join("lib.rs"), "pub struct Tag;")?;
    /// std::fs::write(dir.path().join("api/root.yaml"), "openapi: 3.1.0")?;
    /// std::fs::write(dir.path().join("README.md"), "# Shop")?;
    ///
    /// let mut diagnostics = Diagnostics::new();
    /// let set = SourceSet::discover(
    ///     &[dir.path().to_path_buf()],
    ///     &[],
    ///     &DiscoverOptions::default(),
    ///     &mut diagnostics,
    /// )?;
    /// let kinds: Vec<SourceKind> = set.files().iter().map(|f| f.kind()).collect();
    /// // Sorted by path: `api/root.yaml` before `lib.rs`; `README.md` is not read.
    /// assert_eq!(kinds, [SourceKind::Yaml, SourceKind::Rust]);
    /// assert!(diagnostics.is_empty());
    /// # Ok::<(), Box<dyn std::error::Error>>(())
    /// ```
    pub fn discover(
        inputs: &[PathBuf],
        includes: &[PathBuf],
        options: &DiscoverOptions,
        diagnostics: &mut Diagnostics,
    ) -> Result<Self> {
        let excluded: BTreeSet<PathBuf> = options.exclude.iter().map(|p| normalize(p)).collect();
        // Links may lead into any input, not out of them.
        let roots: Vec<PathBuf> = inputs
            .iter()
            .filter_map(|root| std::fs::canonicalize(root).ok())
            .collect();
        let mut candidates = Vec::new();
        let mut any_file = false;
        let mut watched = Vec::new();

        for root in inputs {
            watched.push(root.clone());
            let mut walk = walkdir::WalkDir::new(root)
                .follow_links(true)
                .sort_by_file_name()
                .into_iter();
            while let Some(entry) = walk.next() {
                let entry = match entry {
                    Ok(entry) => entry,
                    Err(error) => {
                        if let Some(skipped) = unfollowable_link(&error) {
                            diagnostics.push(skipped);
                            continue;
                        }
                        return Err(walk_error(root, error));
                    }
                };
                // An input that is a link leads to its own canonical root.
                if entry.path_is_symlink()
                    && let Some(skipped) = link_outside(entry.path(), &roots)
                {
                    diagnostics.push(skipped);
                    if entry.file_type().is_dir() {
                        walk.skip_current_dir();
                    }
                    continue;
                }
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
        // Overlapping inputs, includes inside inputs and links reach the
        // same file more than once; it is read at its first position.
        let mut seen = BTreeSet::new();
        for path in candidates {
            let Some(kind) = SourceKind::of(&path) else {
                continue;
            };
            let normalized = normalize(&path);
            if !seen.insert(normalized.clone()) {
                continue;
            }
            if excluded.contains(&normalized) {
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
    ///
    /// # Examples
    ///
    /// ```
    /// use oas_forge::source::{SourceFile, SourceKind, SourceSet};
    ///
    /// let set = SourceSet::from_files(vec![SourceFile::new("lib.rs", SourceKind::Rust, "pub struct Tag;")]);
    /// assert_eq!(set.files()[0].text(), "pub struct Tag;");
    /// ```
    pub fn files(&self) -> &[SourceFile] {
        &self.files
    }

    /// Directories and files whose changes affect the result
    /// (for `cargo:rerun-if-changed`).
    ///
    /// # Examples
    ///
    /// ```
    /// use oas_forge::diagnostics::Diagnostics;
    /// use oas_forge::source::{DiscoverOptions, SourceSet};
    ///
    /// let dir = tempfile::tempdir()?;
    /// std::fs::write(dir.path().join("lib.rs"), "pub struct Tag;")?;
    /// let missing = dir.path().join("missing.yaml");
    ///
    /// let mut diagnostics = Diagnostics::new();
    /// let set = SourceSet::discover(
    ///     &[dir.path().to_path_buf()],
    ///     &[missing],
    ///     &DiscoverOptions::default(),
    ///     &mut diagnostics,
    /// )?;
    /// // The directory itself, not each file; missing includes are not watched.
    /// assert_eq!(set.watched(), [dir.path()]);
    /// # Ok::<(), Box<dyn std::error::Error>>(())
    /// ```
    pub fn watched(&self) -> &[PathBuf] {
        &self.watched
    }
}

/// The finding for a symbolic link below an input directory that cannot be
/// followed (broken, or a loop); `None` for other walk errors.
fn unfollowable_link(error: &walkdir::Error) -> Option<Diagnostic> {
    let path = error.path()?;
    let reason = if let Some(ancestor) = error.loop_ancestor() {
        format!("it forms a loop with {}", ancestor.display())
    } else if error.depth() > 0
        && std::fs::symlink_metadata(path).is_ok_and(|m| m.file_type().is_symlink())
    {
        "its target does not exist or cannot be read".to_string()
    } else {
        return None;
    };
    Some(
        Diagnostic::new(
            Code::UnreadableLink,
            format!("symbolic link is skipped: {reason}"),
        )
        .at(Location::new(path, 1)),
    )
}

/// A finding for the symbolic link at `path` when its target lies outside
/// every input directory in `roots` (canonical paths): such a link would
/// pull files the user did not configure, such as credentials elsewhere on
/// the machine, into the document.
fn link_outside(path: &Path, roots: &[PathBuf]) -> Option<Diagnostic> {
    let target = std::fs::canonicalize(path).ok();
    if target
        .as_ref()
        .is_some_and(|target| roots.iter().any(|root| target.starts_with(root)))
    {
        return None;
    }
    let target = target.map_or_else(|| "?".to_string(), |t| t.display().to_string());
    Some(
        Diagnostic::new(
            Code::LinkOutsideInputs,
            format!(
                "symbolic link is skipped: its target {target} is outside the input directories"
            ),
        )
        .at(Location::new(path, 1)),
    )
}

/// An [`Error::Io`] for a failed directory walk, without repeating the
/// walker's own prefix.
fn walk_error(root: &Path, error: walkdir::Error) -> Error {
    let path = error.path().unwrap_or(root).to_path_buf();
    let message = error.to_string();
    let source = error
        .into_io_error()
        .unwrap_or_else(|| std::io::Error::other(message));
    Error::io(&path, source)
}

/// A comparable form of a path: canonical when it exists. A path that does
/// not exist cannot be a discovered file, so it only has to stay distinct.
fn normalize(path: &Path) -> PathBuf {
    path.canonicalize().unwrap_or_else(|_| path.to_path_buf())
}
