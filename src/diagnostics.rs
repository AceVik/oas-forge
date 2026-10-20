//! Findings of a generator run.
//!
//! Every problem the generator detects is reported as a [`Diagnostic`] with a
//! stable [`Code`], a [`Severity`] and, where possible, the [`Location`] of the
//! annotation that caused it. Nothing is skipped silently: an input that cannot
//! be used produces at least a warning.

use std::fmt;
use std::path::{Path, PathBuf};

/// A position in an input file (1-based line, optional 1-based column).
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Location {
    file: PathBuf,
    line: usize,
    column: Option<usize>,
}

impl Location {
    /// A location at `line` (1-based) of `file`.
    pub fn new(file: impl Into<PathBuf>, line: usize) -> Self {
        Self {
            file: file.into(),
            line,
            column: None,
        }
    }

    /// The same location with a 1-based column.
    pub fn with_column(mut self, column: usize) -> Self {
        self.column = Some(column);
        self
    }

    /// The location `offset` lines further down.
    pub fn offset(&self, offset: usize) -> Self {
        Self {
            file: self.file.clone(),
            line: self.line + offset,
            column: None,
        }
    }

    /// The file.
    pub fn file(&self) -> &Path {
        &self.file
    }

    /// The 1-based line.
    pub fn line(&self) -> usize {
        self.line
    }

    /// The 1-based column, if known.
    pub fn column(&self) -> Option<usize> {
        self.column
    }
}

impl fmt::Display for Location {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}:{}", self.file.display(), self.line)?;
        if let Some(column) = self.column {
            write!(f, ":{column}")?;
        }
        Ok(())
    }
}

/// How serious a finding is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[non_exhaustive]
pub enum Severity {
    /// The output would be wrong; generation fails.
    Error,
    /// Something was ignored or is likely a mistake.
    Warning,
    /// Informational.
    Note,
}

impl fmt::Display for Severity {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Error => "error",
            Self::Warning => "warning",
            Self::Note => "note",
        })
    }
}

macro_rules! codes {
    ($( $(#[$doc:meta])* $variant:ident = $code:literal, $severity:ident; )*) => {
        /// Stable identifier of a kind of finding.
        ///
        /// Codes never change their meaning: `OF0xxx` sources and configuration,
        /// `OF1xxx` extraction and attributes, `OF2xxx` route DSL, `OF3xxx`
        /// templates, generics and references, `OF4xxx` merging, `OF5xxx` lints,
        /// `OF6xxx` output.
        #[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
        #[non_exhaustive]
        pub enum Code {
            $( $(#[$doc])* $variant, )*
        }

        impl Code {
            /// Every known code, in numeric order.
            pub const ALL: &'static [Code] = &[$(Code::$variant,)*];

            /// The code as written in messages, e.g. `OF2001`.
            pub fn as_str(self) -> &'static str {
                match self { $( Self::$variant => $code, )* }
            }

            /// The severity findings of this code have unless configured otherwise.
            pub fn default_severity(self) -> Severity {
                match self { $( Self::$variant => Severity::$severity, )* }
            }
        }
    };
}

codes! {
    /// An `include` file does not exist.
    MissingInclude = "OF0001", Warning;
    /// A file was skipped because it exceeds the size limit.
    FileTooLarge = "OF0002", Error;
    /// A file that is also an output was not read as input.
    OutputNotRead = "OF0003", Note;
    /// The root document was generated from package metadata.
    GeneratedRoot = "OF0004", Note;

    /// An `@openapi` override is not valid YAML.
    InvalidOverride = "OF1001", Warning;
    /// An `@openapi<T>` header on an item that has no name.
    UnnamedBlueprint = "OF1002", Warning;
    /// A file-level block was closed by a separator line.
    BlockSeparator = "OF1003", Note;
    /// Two items produce the same schema name.
    DuplicateSchema = "OF1004", Warning;
    /// A variant whose shape serde cannot represent in the enum's tagging mode.
    UnsupportedVariant = "OF1005", Warning;
    /// A validation attribute whose value is not a literal (e.g. a constant).
    NonLiteralConstraint = "OF1006", Warning;
    /// A `regex(path = ..)` / `pattern(..)` constant that is not defined in the scanned code.
    UnresolvedPattern = "OF1007", Warning;

    /// A route is missing its method or path.
    InvalidRoute = "OF2001", Error;
    /// A path placeholder has no parameter definition.
    UndeclaredPathParam = "OF2002", Error;
    /// A path parameter is declared but not used in the path.
    UnusedPathParam = "OF2003", Error;
    /// A DSL directive could not be parsed.
    InvalidDirective = "OF2004", Warning;
    /// Embedded OpenAPI YAML in a route is not valid.
    InvalidRouteYaml = "OF2005", Warning;
    /// An unknown DSL directive.
    UnknownDirective = "OF2006", Warning;
    /// A handler's signature does not fit its route (signature inference).
    SignatureMismatch = "OF2007", Warning;
    /// `@query-params $Struct` (or an inferred extractor) names no documented struct.
    UnresolvedExpansion = "OF2008", Warning;

    /// `@insert` or `@extend` names an unknown fragment.
    UnknownFragment = "OF3001", Warning;
    /// A fragment body is not valid YAML.
    InvalidFragment = "OF3002", Warning;
    /// A fragment was used with fewer arguments than parameters.
    MissingFragmentArgument = "OF3003", Warning;
    /// A generic reference names an unknown blueprint.
    UnknownBlueprint = "OF3004", Warning;
    /// A blueprint was used with the wrong number of arguments.
    BlueprintArity = "OF3005", Warning;
    /// Generic instantiation exceeded the nesting limit.
    InstantiationDepth = "OF3006", Error;
    /// A `$Name` reference does not name a known schema.
    UnresolvedReference = "OF3007", Warning;
    /// Imported templates use an unknown wire format version.
    UnknownTemplateFormat = "OF3008", Warning;
    /// A `{{env:NAME}}` placeholder names an unset environment variable.
    UnknownVariable = "OF3009", Warning;

    /// Two documents set the same value differently.
    MergeConflict = "OF4001", Warning;

    /// A `$ref` points to a component that does not exist.
    DanglingReference = "OF5001", Warning;
    /// Two operations share an operationId.
    DuplicateOperationId = "OF5002", Warning;
    /// A component is defined but never referenced (strict lints).
    UnusedComponent = "OF5003", Warning;
    /// An operation or schema has no description (strict lints).
    MissingDescription = "OF5004", Warning;
    /// A security requirement names a scheme that is not defined.
    UndefinedSecurityScheme = "OF5005", Warning;
    /// A path placeholder and the path parameters of an operation disagree.
    PathParameterMismatch = "OF5006", Warning;

    /// A construct has no equivalent in the target version and was approximated.
    LossyLowering = "OF6001", Note;
    /// A construct needs a newer OpenAPI version than the root declares and was removed.
    UnsupportedFeature = "OF6002", Warning;
}

impl fmt::Display for Code {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// One finding of a generator run.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Diagnostic {
    code: Code,
    severity: Severity,
    message: String,
    location: Option<Location>,
}

impl Diagnostic {
    /// A finding with the default severity of `code`.
    pub fn new(code: Code, message: impl Into<String>) -> Self {
        Self {
            code,
            severity: code.default_severity(),
            message: message.into(),
            location: None,
        }
    }

    /// The same finding attached to `location`.
    pub fn at(mut self, location: Location) -> Self {
        self.location = Some(location);
        self
    }

    /// The same finding attached to `location`, if any.
    pub fn at_opt(mut self, location: Option<&Location>) -> Self {
        self.location = location.cloned();
        self
    }

    /// The same finding with another severity.
    pub fn with_severity(mut self, severity: Severity) -> Self {
        self.severity = severity;
        self
    }

    /// The stable code.
    pub fn code(&self) -> Code {
        self.code
    }

    /// The severity.
    pub fn severity(&self) -> Severity {
        self.severity
    }

    /// The human-readable message.
    pub fn message(&self) -> &str {
        &self.message
    }

    /// Where the finding was caused, if known.
    pub fn location(&self) -> Option<&Location> {
        self.location.as_ref()
    }

    /// Whether this finding fails generation.
    pub fn is_error(&self) -> bool {
        self.severity == Severity::Error
    }

    /// The finding as a JSON object (`severity`, `code`, `message` and, when
    /// known, `file`, `line`, `column`), e.g. for `--message-format json`.
    pub fn to_json(&self) -> serde_json::Value {
        let mut value = serde_json::json!({
            "severity": self.severity.to_string(),
            "code": self.code.as_str(),
            "message": self.message,
        });
        if let Some(location) = &self.location {
            value["file"] = serde_json::json!(location.file().display().to_string());
            value["line"] = serde_json::json!(location.line());
            if let Some(column) = location.column() {
                value["column"] = serde_json::json!(column);
            }
        }
        value
    }
}

impl fmt::Display for Diagnostic {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}[{}]: {}", self.severity, self.code, self.message)?;
        if let Some(location) = &self.location {
            write!(f, " ({location})")?;
        }
        Ok(())
    }
}

/// An ordered collection of findings.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Diagnostics {
    items: Vec<Diagnostic>,
}

impl Diagnostics {
    /// An empty collection.
    pub fn new() -> Self {
        Self::default()
    }

    /// Adds a finding.
    pub fn push(&mut self, diagnostic: Diagnostic) {
        self.items.push(diagnostic);
    }

    /// The findings in the order they were reported.
    pub fn iter(&self) -> std::slice::Iter<'_, Diagnostic> {
        self.items.iter()
    }

    /// The number of findings.
    pub fn len(&self) -> usize {
        self.items.len()
    }

    /// Whether there are no findings.
    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }

    /// Whether any finding fails generation.
    pub fn has_errors(&self) -> bool {
        self.items.iter().any(Diagnostic::is_error)
    }

    /// Raises every warning to an error (`deny_warnings`).
    pub fn deny_warnings(&mut self) {
        for item in &mut self.items {
            if item.severity == Severity::Warning {
                item.severity = Severity::Error;
            }
        }
    }

    /// The findings as a vector.
    pub fn into_vec(self) -> Vec<Diagnostic> {
        self.items
    }
}

impl IntoIterator for Diagnostics {
    type Item = Diagnostic;
    type IntoIter = std::vec::IntoIter<Diagnostic>;

    fn into_iter(self) -> Self::IntoIter {
        self.items.into_iter()
    }
}

impl<'a> IntoIterator for &'a Diagnostics {
    type Item = &'a Diagnostic;
    type IntoIter = std::slice::Iter<'a, Diagnostic>;

    fn into_iter(self) -> Self::IntoIter {
        self.items.iter()
    }
}

/// Adds all findings of an iterator.
impl Extend<Diagnostic> for Diagnostics {
    fn extend<I: IntoIterator<Item = Diagnostic>>(&mut self, iter: I) {
        self.items.extend(iter);
    }
}
