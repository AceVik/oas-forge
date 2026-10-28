//! Protocol Buffers definitions: `.proto` input files, indexed by their
//! package and top-level message names, for `proto(package.Message)`
//! payloads of AsyncAPI messages.
//!
//! The index is a small scan, not a protobuf compiler: it skips comments
//! and strings, tracks braces and reads `syntax`, `package` and the
//! `message` declarations outside any braces. A file it cannot read is
//! reported ([`Code::InvalidProto`]) and left out.

use crate::diagnostics::{Code, Diagnostic, Diagnostics, Location};
use crate::source::SourceFile;
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

/// The media type of protobuf payloads (`contentType` of a message).
pub const PROTOBUF_MEDIA_TYPE: &str = "application/x-protobuf";

/// One indexed `.proto` file.
///
/// # Examples
///
/// ```
/// use oas_forge::diagnostics::Diagnostics;
/// use oas_forge::proto::ProtoIndex;
/// use oas_forge::source::{SourceFile, SourceKind};
///
/// let text = "syntax = \"proto3\";\npackage chat.v1;\nmessage Hello { string name = 1; }\n";
/// let mut index = ProtoIndex::new();
/// index.add(&SourceFile::new("chat.proto", SourceKind::Proto, text), &mut Diagnostics::new());
///
/// let file = index.message("chat.v1.Hello").ok_or("not indexed")?;
/// assert_eq!(file.package(), Some("chat.v1"));
/// assert_eq!(file.messages(), ["Hello"]);
/// assert_eq!(file.text(), text);
/// # Ok::<(), Box<dyn std::error::Error>>(())
/// ```
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProtoFile {
    /// Where the file was read from.
    path: PathBuf,
    /// The `syntax` (`proto2`, `proto3`), if declared.
    syntax: Option<String>,
    /// The `package`, if declared.
    package: Option<String>,
    /// The top-level message names, in order.
    messages: Vec<String>,
    /// The whole file.
    text: String,
}

impl ProtoFile {
    /// The path of the file.
    ///
    /// # Examples
    ///
    /// ```
    /// use oas_forge::diagnostics::Diagnostics;
    /// use oas_forge::proto::ProtoIndex;
    /// use oas_forge::source::{SourceFile, SourceKind};
    /// use std::path::Path;
    ///
    /// let mut index = ProtoIndex::new();
    /// let file = SourceFile::new("proto/a.proto", SourceKind::Proto, "message A {}");
    /// index.add(&file, &mut Diagnostics::new());
    /// assert_eq!(index.message("A").map(|f| f.path()), Some(Path::new("proto/a.proto")));
    /// ```
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// The declared package (`package chat.v1;`), if any.
    ///
    /// # Examples
    ///
    /// ```
    /// use oas_forge::diagnostics::Diagnostics;
    /// use oas_forge::proto::ProtoIndex;
    /// use oas_forge::source::{SourceFile, SourceKind};
    ///
    /// let mut index = ProtoIndex::new();
    /// let file = SourceFile::new("a.proto", SourceKind::Proto, "message A {}");
    /// index.add(&file, &mut Diagnostics::new());
    /// assert_eq!(index.message("A").and_then(|f| f.package()), None);
    /// ```
    pub fn package(&self) -> Option<&str> {
        self.package.as_deref()
    }

    /// The names of the top-level messages, in declaration order (nested
    /// messages are not indexed).
    ///
    /// # Examples
    ///
    /// ```
    /// use oas_forge::diagnostics::Diagnostics;
    /// use oas_forge::proto::ProtoIndex;
    /// use oas_forge::source::{SourceFile, SourceKind};
    ///
    /// let text = "package p;\nmessage Outer { message Inner {} }\nmessage Other {}\n";
    /// let mut index = ProtoIndex::new();
    /// index.add(&SourceFile::new("a.proto", SourceKind::Proto, text), &mut Diagnostics::new());
    /// assert_eq!(index.message("p.Other").map(|f| f.messages()), Some(&["Outer".to_string(), "Other".to_string()][..]));
    /// assert!(index.message("p.Inner").is_none());
    /// ```
    pub fn messages(&self) -> &[String] {
        &self.messages
    }

    /// The whole text of the file.
    ///
    /// # Examples
    ///
    /// ```
    /// use oas_forge::diagnostics::Diagnostics;
    /// use oas_forge::proto::ProtoIndex;
    /// use oas_forge::source::{SourceFile, SourceKind};
    ///
    /// let mut index = ProtoIndex::new();
    /// let file = SourceFile::new("a.proto", SourceKind::Proto, "message A {}\n");
    /// index.add(&file, &mut Diagnostics::new());
    /// assert_eq!(index.message("A").map(|f| f.text()), Some("message A {}\n"));
    /// ```
    pub fn text(&self) -> &str {
        &self.text
    }

    /// The `schemaFormat` of an AsyncAPI Multi Format Schema Object with
    /// this file: `application/vnd.google.protobuf;version=2` for `syntax =
    /// "proto2"`, `version=3` otherwise.
    ///
    /// # Examples
    ///
    /// ```
    /// use oas_forge::diagnostics::Diagnostics;
    /// use oas_forge::proto::ProtoIndex;
    /// use oas_forge::source::{SourceFile, SourceKind};
    ///
    /// let mut index = ProtoIndex::new();
    /// let mut diagnostics = Diagnostics::new();
    /// index.add(&SourceFile::new("a.proto", SourceKind::Proto, "syntax = \"proto2\"; message A {}"), &mut diagnostics);
    /// index.add(&SourceFile::new("b.proto", SourceKind::Proto, "syntax = \"proto3\"; message B {}"), &mut diagnostics);
    /// assert_eq!(
    ///     index.message("A").map(|f| f.schema_format()),
    ///     Some("application/vnd.google.protobuf;version=2")
    /// );
    /// assert_eq!(
    ///     index.message("B").map(|f| f.schema_format()),
    ///     Some("application/vnd.google.protobuf;version=3")
    /// );
    /// ```
    pub fn schema_format(&self) -> &'static str {
        if self.syntax.as_deref() == Some("proto2") {
            "application/vnd.google.protobuf;version=2"
        } else {
            "application/vnd.google.protobuf;version=3"
        }
    }
}

/// The `.proto` files of a run, by the package-qualified names of their
/// top-level messages (`chat.v1.Hello`).
///
/// # Examples
///
/// ```
/// use oas_forge::Code;
/// use oas_forge::diagnostics::Diagnostics;
/// use oas_forge::proto::ProtoIndex;
/// use oas_forge::source::{SourceFile, SourceKind};
///
/// let mut index = ProtoIndex::new();
/// let mut diagnostics = Diagnostics::new();
/// index.add(
///     &SourceFile::new("chat.proto", SourceKind::Proto, "package chat;\nmessage Hello {}\n"),
///     &mut diagnostics,
/// );
/// // An unbalanced brace: the file is reported and left out.
/// index.add(
///     &SourceFile::new("broken.proto", SourceKind::Proto, "message Broken {\n"),
///     &mut diagnostics,
/// );
/// assert!(index.message("chat.Hello").is_some());
/// assert!(index.message("Broken").is_none());
/// let codes: Vec<Code> = diagnostics.iter().map(|d| d.code()).collect();
/// assert_eq!(codes, [Code::InvalidProto]);
/// ```
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ProtoIndex {
    /// The indexed files, in the order they were added.
    files: Vec<ProtoFile>,
    /// The file (position in `files`) of each qualified message name.
    messages: BTreeMap<String, usize>,
}

/// The empty index, for blocks compiled without `.proto` files.
pub(crate) static EMPTY: ProtoIndex = ProtoIndex::new();

impl ProtoIndex {
    /// An empty index.
    ///
    /// # Examples
    ///
    /// ```
    /// let index = oas_forge::proto::ProtoIndex::new();
    /// assert!(index.message("chat.Hello").is_none());
    /// ```
    pub const fn new() -> Self {
        Self {
            files: Vec::new(),
            messages: BTreeMap::new(),
        }
    }

    /// Indexes a `.proto` file. A file that cannot be read (an unclosed
    /// comment, string or brace, a second `package`, …) is reported and
    /// left out; a message name declared before (in this or another file)
    /// is reported and keeps the first declaration.
    ///
    /// # Examples
    ///
    /// ```
    /// use oas_forge::Code;
    /// use oas_forge::diagnostics::Diagnostics;
    /// use oas_forge::proto::ProtoIndex;
    /// use oas_forge::source::{SourceFile, SourceKind};
    ///
    /// let mut index = ProtoIndex::new();
    /// let mut diagnostics = Diagnostics::new();
    /// for path in ["a.proto", "b.proto"] {
    ///     index.add(&SourceFile::new(path, SourceKind::Proto, "package p;\nmessage M {}\n"), &mut diagnostics);
    /// }
    /// assert_eq!(index.message("p.M").map(|f| f.path().to_string_lossy().into_owned()), Some("a.proto".into()));
    /// let finding = diagnostics.iter().next().ok_or("no finding")?;
    /// assert_eq!(finding.code(), Code::InvalidProto);
    /// assert_eq!(finding.location().map(|l| l.line()), Some(2));
    /// # Ok::<(), Box<dyn std::error::Error>>(())
    /// ```
    pub fn add(&mut self, file: &SourceFile, diagnostics: &mut Diagnostics) {
        let scanned = match scan(file.text()) {
            Ok(scanned) => scanned,
            Err((line, message)) => {
                diagnostics.push(
                    Diagnostic::new(
                        Code::InvalidProto,
                        format!("{message}; the file is not indexed"),
                    )
                    .at(Location::new(file.path(), line)),
                );
                return;
            }
        };
        let position = self.files.len();
        let mut names = Vec::with_capacity(scanned.messages.len());
        // `names` keeps the order; the set finds duplicates in log time.
        let mut seen = BTreeSet::new();
        for (name, line) in &scanned.messages {
            if seen.contains(name.as_str()) {
                diagnostics.push(
                    Diagnostic::new(
                        Code::InvalidProto,
                        format!("message `{name}` is declared twice; the first is used"),
                    )
                    .at(Location::new(file.path(), *line)),
                );
                continue;
            }
            let qualified = match &scanned.package {
                Some(package) => format!("{package}.{name}"),
                None => name.clone(),
            };
            if let Some(first) = self
                .messages
                .get(&qualified)
                .and_then(|i| self.files.get(*i))
            {
                diagnostics.push(
                    Diagnostic::new(
                        Code::InvalidProto,
                        format!(
                            "message `{qualified}` is also defined in {}; that definition is used",
                            first.path.display()
                        ),
                    )
                    .at(Location::new(file.path(), *line)),
                );
                continue;
            }
            self.messages.insert(qualified, position);
            seen.insert(name.as_str());
            names.push(name.clone());
        }
        self.files.push(ProtoFile {
            path: file.path().to_path_buf(),
            syntax: scanned.syntax,
            package: scanned.package,
            messages: names,
            text: file.text().to_string(),
        });
    }

    /// The file that defines the top-level message `name`, qualified with
    /// its package (`chat.v1.Hello`).
    ///
    /// # Examples
    ///
    /// ```
    /// use oas_forge::diagnostics::Diagnostics;
    /// use oas_forge::proto::ProtoIndex;
    /// use oas_forge::source::{SourceFile, SourceKind};
    ///
    /// let mut index = ProtoIndex::new();
    /// let file = SourceFile::new("chat.proto", SourceKind::Proto, "package chat.v1; message Hello {}");
    /// index.add(&file, &mut Diagnostics::new());
    /// assert!(index.message("chat.v1.Hello").is_some());
    /// assert!(index.message("Hello").is_none());
    /// ```
    pub fn message(&self, name: &str) -> Option<&ProtoFile> {
        self.messages.get(name).and_then(|i| self.files.get(*i))
    }
}

/// What the scan of a `.proto` file finds.
#[derive(Debug, Default)]
struct Scanned {
    /// The `syntax` value.
    syntax: Option<String>,
    /// The `package` name.
    package: Option<String>,
    /// The top-level message names and their lines.
    messages: Vec<(String, usize)>,
}

/// A token of a `.proto` file.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Token {
    /// An identifier, a dotted name or a number.
    Word(String),
    /// The content of a string literal.
    Text(String),
    /// Any other character, such as `{`, `;` or `=`.
    Punct(char),
}

/// Scans a `.proto` file; the error is the 1-based line and what is wrong.
fn scan(text: &str) -> Result<Scanned, (usize, String)> {
    let tokens = tokenize(text)?;
    let mut scanned = Scanned::default();
    let mut depth = 0usize;
    let mut rest = tokens.as_slice();
    while let Some(((token, line), tail)) = rest.split_first() {
        rest = tail;
        match token {
            Token::Punct('{') => depth += 1,
            Token::Punct('}') => {
                depth = depth
                    .checked_sub(1)
                    .ok_or_else(|| (*line, "`}` without `{`".to_string()))?;
            }
            Token::Word(word) if depth == 0 => match word.as_str() {
                "syntax" | "edition" => {
                    if let [(Token::Punct('='), _), (Token::Text(value), _), ..] = rest {
                        if word == "syntax" {
                            scanned.syntax = Some(value.clone());
                        }
                    } else {
                        return Err((*line, format!("`{word}` needs `= \"…\";`")));
                    }
                }
                "package" => {
                    let name = match rest {
                        [(Token::Word(name), _), (Token::Punct(';'), _), ..]
                            if is_qualified_name(name) =>
                        {
                            name.clone()
                        }
                        _ => return Err((*line, "`package` needs `name.name;`".to_string())),
                    };
                    if scanned.package.is_some() {
                        return Err((*line, "a second `package`".to_string()));
                    }
                    scanned.package = Some(name);
                }
                "message" => match rest {
                    [(Token::Word(name), _), (Token::Punct('{'), _), ..] if is_name(name) => {
                        scanned.messages.push((name.clone(), *line));
                    }
                    _ => return Err((*line, "`message` needs `Name {`".to_string())),
                },
                _ => {}
            },
            _ => {}
        }
    }
    if depth > 0 {
        let last = tokens.last().map_or(1, |(_, line)| *line);
        return Err((last, "a `{` is not closed".to_string()));
    }
    Ok(scanned)
}

/// Splits a `.proto` file into tokens with their 1-based lines, without
/// comments; the error is the line and what is wrong.
fn tokenize(text: &str) -> Result<Vec<(Token, usize)>, (usize, String)> {
    let mut tokens = Vec::new();
    let mut line = 1;
    let mut chars = text.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            '\n' => line += 1,
            c if c.is_whitespace() => {}
            '/' if chars.peek() == Some(&'/') => while chars.next_if(|c| *c != '\n').is_some() {},
            '/' if chars.peek() == Some(&'*') => {
                let start = line;
                chars.next();
                let mut closed = false;
                while let Some(c) = chars.next() {
                    match c {
                        '\n' => line += 1,
                        '*' if chars.next_if_eq(&'/').is_some() => {
                            closed = true;
                            break;
                        }
                        _ => {}
                    }
                }
                if !closed {
                    return Err((start, "a `/*` comment is not closed".to_string()));
                }
            }
            '"' | '\'' => {
                let mut value = String::new();
                let mut closed = false;
                while let Some(next) = chars.next() {
                    match next {
                        '\n' => break,
                        '\\' => {
                            if let Some(escaped) = chars.next_if(|c| *c != '\n') {
                                value.push('\\');
                                value.push(escaped);
                            }
                        }
                        quote if quote == c => {
                            closed = true;
                            break;
                        }
                        other => value.push(other),
                    }
                }
                if !closed {
                    return Err((line, "a string is not closed on its line".to_string()));
                }
                tokens.push((Token::Text(value), line));
            }
            c if is_word_char(c) => {
                let mut word = c.to_string();
                while let Some(next) = chars.next_if(|c| is_word_char(*c)) {
                    word.push(next);
                }
                tokens.push((Token::Word(word), line));
            }
            other => tokens.push((Token::Punct(other), line)),
        }
    }
    Ok(tokens)
}

/// Whether `c` belongs to an identifier, a dotted name or a number.
fn is_word_char(c: char) -> bool {
    c.is_ascii_alphanumeric() || c == '_' || c == '.'
}

/// Whether `name` is a protobuf identifier: a letter or `_`, then letters,
/// digits or `_`.
fn is_name(name: &str) -> bool {
    let mut chars = name.chars();
    chars
        .next()
        .is_some_and(|c| c.is_ascii_alphabetic() || c == '_')
        && chars.all(|c| c.is_ascii_alphanumeric() || c == '_')
}

/// Whether `name` is a dotted name of identifiers (`chat.v1`).
fn is_qualified_name(name: &str) -> bool {
    name.split('.').all(is_name)
}
