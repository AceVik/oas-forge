//! Findings of a generator run.
//!
//! Every problem the generator detects is reported as a [`Diagnostic`] with a
//! stable [`Code`], a [`Severity`] and, where possible, the [`Location`] of the
//! annotation that caused it. Nothing is skipped silently: an input that cannot
//! be used produces at least a warning.

use std::fmt;
use std::path::{Path, PathBuf};

/// A position in an input file (1-based line, optional 1-based column).
///
/// # Examples
///
/// ```
/// use oas_forge::Location;
///
/// let location = Location::new("src/lib.rs", 12).with_column(5);
/// assert_eq!(location.to_string(), "src/lib.rs:12:5");
/// assert_eq!(location.offset(2).to_string(), "src/lib.rs:14");
/// ```
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Location {
    /// The file, as given (not canonicalized).
    file: PathBuf,
    /// The 1-based line.
    line: usize,
    /// The 1-based column; `None` when only the line is known.
    column: Option<usize>,
}

impl Location {
    /// A location at `line` (1-based) of `file`.
    ///
    /// # Examples
    ///
    /// ```
    /// let location = oas_forge::Location::new("src/lib.rs", 3);
    /// assert_eq!(location.line(), 3);
    /// assert_eq!(location.column(), None);
    /// ```
    pub fn new(file: impl Into<PathBuf>, line: usize) -> Self {
        Self {
            file: file.into(),
            line,
            column: None,
        }
    }

    /// The same location with a 1-based column.
    ///
    /// # Examples
    ///
    /// ```
    /// let location = oas_forge::Location::new("src/lib.rs", 3).with_column(8);
    /// assert_eq!(location.column(), Some(8));
    /// ```
    pub fn with_column(mut self, column: usize) -> Self {
        self.column = Some(column);
        self
    }

    /// The location `offset` lines further down, without a column; e.g. a
    /// line inside a block that starts at `self`.
    ///
    /// # Examples
    ///
    /// ```
    /// use oas_forge::Location;
    ///
    /// let block = Location::new("src/lib.rs", 10).with_column(1);
    /// assert_eq!(block.offset(3), Location::new("src/lib.rs", 13));
    /// ```
    pub fn offset(&self, offset: usize) -> Self {
        Self {
            file: self.file.clone(),
            line: self.line + offset,
            column: None,
        }
    }

    /// The file.
    ///
    /// # Examples
    ///
    /// ```
    /// use std::path::Path;
    ///
    /// let location = oas_forge::Location::new("src/lib.rs", 3);
    /// assert_eq!(location.file(), Path::new("src/lib.rs"));
    /// ```
    pub fn file(&self) -> &Path {
        &self.file
    }

    /// The 1-based line.
    ///
    /// # Examples
    ///
    /// ```
    /// assert_eq!(oas_forge::Location::new("src/lib.rs", 3).line(), 3);
    /// ```
    pub fn line(&self) -> usize {
        self.line
    }

    /// The 1-based column, if known.
    ///
    /// # Examples
    ///
    /// ```
    /// use oas_forge::Location;
    ///
    /// assert_eq!(Location::new("src/lib.rs", 3).column(), None);
    /// assert_eq!(Location::new("src/lib.rs", 3).with_column(7).column(), Some(7));
    /// ```
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
///
/// Ordered from the most to the least serious, so `Error < Warning`.
///
/// # Examples
///
/// ```
/// use oas_forge::{Code, Severity};
///
/// assert_eq!(Code::DanglingReference.default_severity(), Severity::Warning);
/// assert_eq!(Severity::Warning.to_string(), "warning");
/// assert!(Severity::Error < Severity::Note);
/// ```
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

/// Declares [`Code`] from `Variant = "OFxxxx", Severity;` entries: the enum,
/// [`Code::ALL`] and the tables behind [`Code::as_str`] and
/// [`Code::default_severity`], so each code is defined in one place.
macro_rules! codes {
    ($( $(#[$doc:meta])* $variant:ident = $code:literal, $severity:ident; )*) => {
        /// Stable identifier of a kind of finding.
        ///
        /// Codes never change their meaning: `OF0xxx` sources and configuration,
        /// `OF1xxx` extraction and attributes, `OF2xxx` route DSL, `OF3xxx`
        /// templates, generics and references, `OF4xxx` merging, `OF5xxx` lints,
        /// `OF6xxx` output.
        ///
        /// # Examples
        ///
        /// ```
        /// use oas_forge::{Code, Severity};
        ///
        /// let code = Code::UnresolvedReference;
        /// assert_eq!(code.to_string(), "OF3007");
        /// assert_eq!(code.default_severity(), Severity::Warning);
        /// assert!(Code::ALL.contains(&code));
        /// ```
        #[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
        #[non_exhaustive]
        pub enum Code {
            $( $(#[$doc])* $variant, )*
        }

        impl Code {
            /// Every known code: those of 0.2.0 in numeric order, later ones
            /// in the order they were added.
            pub const ALL: &'static [Code] = &[$(Code::$variant,)*];

            /// The code as written in messages, e.g. `OF2001`.
            ///
            /// # Examples
            ///
            /// ```
            /// assert_eq!(oas_forge::Code::InvalidRoute.as_str(), "OF2001");
            /// ```
            pub fn as_str(self) -> &'static str {
                match self { $( Self::$variant => $code, )* }
            }

            /// The severity findings of this code have unless configured otherwise.
            ///
            /// # Examples
            ///
            /// ```
            /// use oas_forge::{Code, Severity};
            ///
            /// assert_eq!(Code::InvalidRoute.default_severity(), Severity::Error);
            /// assert_eq!(Code::LossyLowering.default_severity(), Severity::Note);
            /// ```
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
    /// A symbolic link in an input directory is broken or forms a loop.
    UnreadableLink = "OF0005", Warning;
    /// A `.proto` file cannot be indexed, or defines a message another one defines.
    InvalidProto = "OF0006", Warning;

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
    /// A validation attribute whose value is not a literal (e.g. a constant) or out of range.
    NonLiteralConstraint = "OF1006", Warning;
    /// A `regex(path = ..)` / `pattern(..)` constant that is not defined in the scanned code.
    UnresolvedPattern = "OF1007", Warning;
    /// An `@openapi` directive of a type, field or variant has an invalid argument.
    InvalidSchemaDirective = "OF1008", Warning;
    /// A variant of a `serde_repr` enum whose discriminant is not an integer literal.
    UnknownDiscriminant = "OF1009", Warning;
    /// A type that cannot be documented (function pointer, trait object, `impl Trait`, macro).
    UndocumentableType = "OF1010", Warning;
    /// A `regex(path = ..)` / `pattern(..)` constant path that matches constants with different patterns.
    AmbiguousPattern = "OF1011", Warning;

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
    /// `proto(package.Message)` names no message of the `.proto` inputs.
    UnknownProtoMessage = "OF2009", Warning;

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
    /// A fragment inserts or extends itself, directly or through others.
    FragmentCycle = "OF3010", Warning;
    /// A fragment was used with more arguments than parameters.
    ExtraFragmentArgument = "OF3011", Warning;
    /// A type alias name refers to several aliases with different targets.
    AmbiguousAlias = "OF3012", Warning;
    /// A type alias refers to itself through other aliases.
    AliasCycle = "OF3013", Warning;
    /// A Rust type name does not identify one documented type.
    AmbiguousTypeName = "OF3014", Warning;

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
    /// A path placeholder and the path parameters of an operation disagree,
    /// or an AsyncAPI channel address and the channel's parameters.
    PathParameterMismatch = "OF5006", Warning;

    /// A construct has no equivalent in the target version and was approximated.
    LossyLowering = "OF6001", Note;
    /// A construct needs a newer OpenAPI version than the root declares and was removed.
    UnsupportedFeature = "OF6002", Warning;

    // Codes added after 0.2.0. New codes go at the end, whatever their
    // number: a variant's position is its discriminant and its place in the
    // ordering, which must not change between releases (cargo semver-checks).

    /// The `Cargo.toml` of a scanned crate cannot be read; the crate is named after its directory.
    UnreadableManifest = "OF0007", Warning;
    /// A generic reference is malformed or exceeds the parser nesting limit.
    InvalidGenericReference = "OF3015", Error;
    /// Expanding fragments exceeded the work or text size budget.
    ExpansionLimit = "OF3016", Error;
}

impl fmt::Display for Code {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// One finding of a generator run.
///
/// # Examples
///
/// ```
/// use oas_forge::{Code, Diagnostic, Location};
///
/// let diagnostic = Diagnostic::new(Code::DanglingReference, "`#/components/schemas/User` does not exist")
///     .at(Location::new("src/api.rs", 7));
/// assert_eq!(
///     diagnostic.to_string(),
///     "warning[OF5001]: `#/components/schemas/User` does not exist (src/api.rs:7)"
/// );
/// ```
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Diagnostic {
    /// What kind of finding this is.
    code: Code,
    /// The default of `code` unless raised or lowered
    /// ([`Diagnostic::with_severity`], [`Diagnostics::deny_warnings`]).
    severity: Severity,
    /// The human-readable message.
    message: String,
    /// The annotation that caused the finding, if known.
    location: Option<Location>,
}

impl Diagnostic {
    /// A finding with the default severity of `code`.
    ///
    /// # Examples
    ///
    /// ```
    /// use oas_forge::{Code, Diagnostic, Severity};
    ///
    /// let diagnostic = Diagnostic::new(Code::InvalidRoute, "`@route` needs a method and a path");
    /// assert_eq!(diagnostic.severity(), Severity::Error);
    /// assert_eq!(diagnostic.location(), None);
    /// ```
    pub fn new(code: Code, message: impl Into<String>) -> Self {
        Self {
            code,
            severity: code.default_severity(),
            message: message.into(),
            location: None,
        }
    }

    /// The same finding attached to `location`.
    ///
    /// # Examples
    ///
    /// ```
    /// use oas_forge::{Code, Diagnostic, Location};
    ///
    /// let diagnostic = Diagnostic::new(Code::UnknownDirective, "unknown directive `@foo`")
    ///     .at(Location::new("src/api.rs", 4));
    /// assert_eq!(diagnostic.location(), Some(&Location::new("src/api.rs", 4)));
    /// ```
    pub fn at(mut self, location: Location) -> Self {
        self.location = Some(location);
        self
    }

    /// The same finding attached to `location`, if any; `None` removes a
    /// location.
    ///
    /// # Examples
    ///
    /// ```
    /// use oas_forge::{Code, Diagnostic, Location};
    ///
    /// let known: Option<&Location> = None;
    /// let diagnostic = Diagnostic::new(Code::MergeConflict, "conflict").at_opt(known);
    /// assert_eq!(diagnostic.location(), None);
    /// ```
    pub fn at_opt(mut self, location: Option<&Location>) -> Self {
        self.location = location.cloned();
        self
    }

    /// The same finding with another severity.
    ///
    /// # Examples
    ///
    /// ```
    /// use oas_forge::{Code, Diagnostic, Severity};
    ///
    /// let diagnostic = Diagnostic::new(Code::UnusedComponent, "`Legacy` is never used")
    ///     .with_severity(Severity::Note);
    /// assert!(!diagnostic.is_error());
    /// assert_eq!(diagnostic.severity(), Severity::Note);
    /// ```
    pub fn with_severity(mut self, severity: Severity) -> Self {
        self.severity = severity;
        self
    }

    /// The stable code.
    ///
    /// # Examples
    ///
    /// ```
    /// use oas_forge::{Code, Diagnostic};
    ///
    /// let diagnostic = Diagnostic::new(Code::DuplicateOperationId, "`getUser` is used twice");
    /// assert_eq!(diagnostic.code(), Code::DuplicateOperationId);
    /// ```
    pub fn code(&self) -> Code {
        self.code
    }

    /// The severity.
    ///
    /// # Examples
    ///
    /// ```
    /// use oas_forge::{Code, Diagnostic, Severity};
    ///
    /// let diagnostic = Diagnostic::new(Code::GeneratedRoot, "root generated from the package");
    /// assert_eq!(diagnostic.severity(), Severity::Note);
    /// ```
    pub fn severity(&self) -> Severity {
        self.severity
    }

    /// The human-readable message.
    ///
    /// # Examples
    ///
    /// ```
    /// use oas_forge::{Code, Diagnostic};
    ///
    /// let diagnostic = Diagnostic::new(Code::MissingInclude, "include file api.yaml does not exist");
    /// assert_eq!(diagnostic.message(), "include file api.yaml does not exist");
    /// ```
    pub fn message(&self) -> &str {
        &self.message
    }

    /// Where the finding was caused, if known.
    ///
    /// # Examples
    ///
    /// ```
    /// use oas_forge::{Code, Diagnostic, Location};
    ///
    /// let diagnostic = Diagnostic::new(Code::FileTooLarge, "file is too large")
    ///     .at(Location::new("src/generated.rs", 1));
    /// assert_eq!(diagnostic.location().map(Location::line), Some(1));
    /// ```
    pub fn location(&self) -> Option<&Location> {
        self.location.as_ref()
    }

    /// Whether this finding fails generation.
    ///
    /// # Examples
    ///
    /// ```
    /// use oas_forge::{Code, Diagnostic};
    ///
    /// assert!(Diagnostic::new(Code::UndeclaredPathParam, "`{id}` is not declared").is_error());
    /// assert!(!Diagnostic::new(Code::MergeConflict, "`/info/title` differs").is_error());
    /// ```
    pub fn is_error(&self) -> bool {
        self.severity == Severity::Error
    }

    /// The finding as a JSON object (`severity`, `code`, `message` and, when
    /// known, `file`, `line`, `column`), e.g. for `--message-format json`.
    ///
    /// # Examples
    ///
    /// ```
    /// use oas_forge::{Code, Diagnostic, Location};
    /// use serde_json::json;
    ///
    /// let diagnostic = Diagnostic::new(Code::UnresolvedReference, "`$Usr` is not a known schema")
    ///     .at(Location::new("src/api.rs", 9).with_column(14));
    /// assert_eq!(
    ///     diagnostic.to_json(),
    ///     json!({
    ///         "severity": "warning",
    ///         "code": "OF3007",
    ///         "message": "`$Usr` is not a known schema",
    ///         "file": "src/api.rs",
    ///         "line": 9,
    ///         "column": 14
    ///     })
    /// );
    /// ```
    pub fn to_json(&self) -> serde_json::Value {
        let mut value = serde_json::Map::new();
        value.insert(
            "severity".into(),
            serde_json::json!(self.severity.to_string()),
        );
        value.insert("code".into(), serde_json::json!(self.code.as_str()));
        value.insert("message".into(), serde_json::json!(self.message));
        if let Some(location) = &self.location {
            value.insert(
                "file".into(),
                serde_json::json!(location.file().display().to_string()),
            );
            value.insert("line".into(), serde_json::json!(location.line()));
            if let Some(column) = location.column() {
                value.insert("column".into(), serde_json::json!(column));
            }
        }
        serde_json::Value::Object(value)
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
///
/// # Examples
///
/// ```
/// use oas_forge::{Code, Diagnostic, Diagnostics};
///
/// let mut diagnostics = Diagnostics::new();
/// diagnostics.push(Diagnostic::new(Code::MergeConflict, "`/info/title` differs"));
/// diagnostics.push(Diagnostic::new(Code::InvalidRoute, "`@route` has no path"));
///
/// let codes: Vec<&str> = diagnostics.iter().map(|d| d.code().as_str()).collect();
/// assert_eq!(codes, ["OF4001", "OF2001"]);
/// assert!(diagnostics.has_errors());
/// ```
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Diagnostics {
    /// The findings in the order they were reported.
    items: Vec<Diagnostic>,
}

impl Diagnostics {
    /// An empty collection.
    ///
    /// # Examples
    ///
    /// ```
    /// let diagnostics = oas_forge::Diagnostics::new();
    /// assert!(diagnostics.is_empty());
    /// assert!(!diagnostics.has_errors());
    /// ```
    pub fn new() -> Self {
        Self::default()
    }

    /// Adds a finding.
    ///
    /// # Examples
    ///
    /// ```
    /// use oas_forge::{Code, Diagnostic, Diagnostics};
    ///
    /// let mut diagnostics = Diagnostics::new();
    /// diagnostics.push(Diagnostic::new(Code::UnknownFragment, "no fragment `Paging`"));
    /// assert_eq!(diagnostics.len(), 1);
    /// ```
    pub fn push(&mut self, diagnostic: Diagnostic) {
        self.items.push(diagnostic);
    }

    /// The findings in the order they were reported.
    ///
    /// # Examples
    ///
    /// ```
    /// use oas_forge::{Code, Diagnostic, Diagnostics, Severity};
    ///
    /// let mut diagnostics = Diagnostics::new();
    /// diagnostics.push(Diagnostic::new(Code::GeneratedRoot, "root generated"));
    /// diagnostics.push(Diagnostic::new(Code::DanglingReference, "dangling `$ref`"));
    ///
    /// let warnings = diagnostics.iter().filter(|d| d.severity() == Severity::Warning);
    /// assert_eq!(warnings.count(), 1);
    /// ```
    pub fn iter(&self) -> std::slice::Iter<'_, Diagnostic> {
        self.items.iter()
    }

    /// The number of findings.
    ///
    /// # Examples
    ///
    /// ```
    /// use oas_forge::{Code, Diagnostic, Diagnostics};
    ///
    /// let mut diagnostics = Diagnostics::new();
    /// diagnostics.extend([
    ///     Diagnostic::new(Code::MergeConflict, "`/info/title` differs"),
    ///     Diagnostic::new(Code::MergeConflict, "`/info/version` differs"),
    /// ]);
    /// assert_eq!(diagnostics.len(), 2);
    /// ```
    pub fn len(&self) -> usize {
        self.items.len()
    }

    /// Whether there are no findings.
    ///
    /// # Examples
    ///
    /// ```
    /// use oas_forge::{Code, Diagnostic, Diagnostics};
    ///
    /// let mut diagnostics = Diagnostics::new();
    /// assert!(diagnostics.is_empty());
    /// diagnostics.push(Diagnostic::new(Code::GeneratedRoot, "root generated"));
    /// assert!(!diagnostics.is_empty());
    /// ```
    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }

    /// Whether any finding fails generation.
    ///
    /// # Examples
    ///
    /// ```
    /// use oas_forge::{Code, Diagnostic, Diagnostics};
    ///
    /// let mut diagnostics = Diagnostics::new();
    /// diagnostics.push(Diagnostic::new(Code::DanglingReference, "dangling `$ref`"));
    /// assert!(!diagnostics.has_errors());
    /// diagnostics.push(Diagnostic::new(Code::UnusedPathParam, "`id` is not in the path"));
    /// assert!(diagnostics.has_errors());
    /// ```
    pub fn has_errors(&self) -> bool {
        self.items.iter().any(Diagnostic::is_error)
    }

    /// Raises every warning to an error (`deny_warnings`); notes stay notes.
    ///
    /// # Examples
    ///
    /// ```
    /// use oas_forge::{Code, Diagnostic, Diagnostics, Severity};
    ///
    /// let mut diagnostics = Diagnostics::new();
    /// diagnostics.push(Diagnostic::new(Code::DanglingReference, "dangling `$ref`"));
    /// diagnostics.push(Diagnostic::new(Code::GeneratedRoot, "root generated"));
    /// diagnostics.deny_warnings();
    ///
    /// let severities: Vec<Severity> = diagnostics.iter().map(|d| d.severity()).collect();
    /// assert_eq!(severities, [Severity::Error, Severity::Note]);
    /// ```
    pub fn deny_warnings(&mut self) {
        for item in &mut self.items {
            if item.severity == Severity::Warning {
                item.severity = Severity::Error;
            }
        }
    }

    /// The findings as a vector.
    ///
    /// # Examples
    ///
    /// ```
    /// use oas_forge::{Code, Diagnostic, Diagnostics};
    ///
    /// let mut diagnostics = Diagnostics::new();
    /// diagnostics.push(Diagnostic::new(Code::GeneratedRoot, "root generated"));
    /// let findings: Vec<Diagnostic> = diagnostics.into_vec();
    /// assert_eq!(findings[0].code(), Code::GeneratedRoot);
    /// ```
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
