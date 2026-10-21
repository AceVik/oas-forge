//! `@openapi` directives in the doc comments of structs, enums, fields,
//! variants and type aliases.

use super::rename::RenameRule;
use crate::docs::{DocLine, description};

/// What the doc comment of a schema item (or of one of its fields/variants) says.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct ItemDocs {
    /// The description: the `@description` text, else the prose of the doc
    /// comment (without `@internal` sections).
    pub description: String,
    /// `@summary text`: the title.
    pub summary: Option<String>,
    /// Whether a line starts with `@openapi` (explicit export).
    pub exported: bool,
    /// `@openapi rename "Name"`
    pub rename: Option<String>,
    /// `@openapi rename-all rule`
    pub rename_all: Option<RenameRule>,
    /// `@openapi<T, U>`
    pub blueprint: Option<Vec<String>>,
    /// The YAML override of each `@openapi` line: the text after it and the
    /// lines after it, up to the next directive line.
    pub overrides: Vec<Vec<DocLine>>,
    /// `@openapi skip`: the item, field or variant is left out.
    pub skip: bool,
    /// `@example value` lines (YAML values).
    pub examples: Vec<DocLine>,
    /// Directives with an invalid argument (the message and its line).
    pub problems: Vec<DocLine>,
    /// `@openapi-fragment` blocks placed in the doc comment: the header and
    /// the body lines. They are not part of the item's description or
    /// override.
    pub fragments: Vec<(DocLine, Vec<DocLine>)>,
}

/// How the text after `@openapi` on a header line is used.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ItemKind {
    /// Structs and enums: `@openapi<T>` declares a blueprint.
    Container,
    /// Fields and variants.
    Member,
}

/// The part of a doc comment that the plain lines (no directive) belong to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Section {
    /// Prose: the description, unless `@description` replaces it.
    Prose,
    /// The YAML of an `@openapi` line; `indent` is the indentation of that
    /// line. A line starting with `@` that is indented more is YAML text (in
    /// a block scalar).
    Yaml {
        /// The indentation of the `@openapi` line.
        indent: usize,
    },
    /// `@description` and the plain lines after it.
    Description,
    /// `@internal` and the plain lines after it: never documented.
    Internal,
}

/// The text after `directive` (`@example`) at the start of `line`, if the
/// line starts with that directive as a whole word.
fn argument<'a>(line: &'a str, directive: &str) -> Option<&'a str> {
    let rest = line.strip_prefix(directive)?;
    (rest.is_empty() || rest.starts_with(char::is_whitespace)).then(|| rest.trim())
}

/// The number of leading spaces of `text`.
fn indentation(text: &str) -> usize {
    text.len() - text.trim_start_matches(' ').len()
}

/// Whether `trimmed` starts a section of its own: a line starting with `@`,
/// except `@insert` and `@extend`, which belong to the YAML around them.
fn is_directive(trimmed: &str) -> bool {
    trimmed.starts_with('@')
        && argument(trimmed, "@insert").is_none()
        && argument(trimmed, "@extend").is_none()
}

/// Whether `trimmed` ends an `@openapi-fragment` body: the directives of
/// the item itself (a fragment may hold route directives).
fn ends_fragment(trimmed: &str) -> bool {
    trimmed.starts_with("@openapi")
        || ["@example", "@description", "@summary", "@internal"]
            .iter()
            .any(|directive| argument(trimmed, directive).is_some())
}

impl ItemDocs {
    /// Reads the doc `lines` of an item of `kind`. Problems are collected
    /// in `problems`, not reported.
    ///
    /// Plain lines belong to the section of the directive before them:
    /// prose (before any), the YAML of `@openapi`, `@description` or
    /// `@internal`. Every other directive (`@example`, `@summary`, unknown
    /// ones) ends the section; prose follows it.
    pub(crate) fn parse(lines: &[DocLine], kind: ItemKind) -> Self {
        let mut docs = Self::default();
        let mut prose = Vec::new();
        let mut explicit: Option<Vec<&str>> = None;
        let mut section = Section::Prose;
        // The open `@openapi-fragment` block, and the line of the last line.
        let mut fragment: Option<(DocLine, Vec<DocLine>)> = None;
        let mut last_line: Option<usize> = None;

        for line in lines {
            let trimmed = line.trimmed();
            // A fragment block ends at the next directive of the item or
            // where the doc comment is interrupted (a blank line between two
            // `///` groups), like the blocks of file comments.
            let gap = last_line.is_some_and(|last| line.line > last + 1);
            last_line = Some(line.line);
            if let Some((header, mut body)) = fragment.take() {
                if gap || ends_fragment(trimmed) {
                    docs.fragments.push((header, body));
                } else {
                    body.push(line.clone());
                    fragment = Some((header, body));
                    continue;
                }
            }
            let yaml_text =
                matches!(section, Section::Yaml { indent } if indentation(&line.text) > indent);
            if !is_directive(trimmed) || yaml_text {
                match section {
                    Section::Prose => prose.push(line.text.as_str()),
                    Section::Yaml { .. } => docs
                        .overrides
                        .last_mut()
                        .into_iter()
                        .for_each(|lines| lines.push(line.clone())),
                    Section::Description => explicit.get_or_insert_default().push(&line.text),
                    Section::Internal => {}
                }
                continue;
            }
            section = Section::Prose;
            if trimmed.starts_with("@openapi-fragment") {
                fragment = Some((DocLine::new(trimmed, line.line), Vec::new()));
            } else if let Some(example) = argument(trimmed, "@example") {
                docs.examples.push(DocLine::new(example, line.line));
            } else if let Some(text) = argument(trimmed, "@description") {
                section = Section::Description;
                let lines = explicit.get_or_insert_default();
                if !text.is_empty() {
                    lines.push(text);
                }
            } else if argument(trimmed, "@internal").is_some() {
                section = Section::Internal;
            } else if let Some(text) = argument(trimmed, "@summary") {
                if text.is_empty() {
                    docs.problems
                        .push(DocLine::new("`@summary` needs a text", line.line));
                } else {
                    docs.summary = Some(text.to_string());
                }
            } else if let Some(rest) = trimmed.strip_prefix("@openapi") {
                section = Section::Yaml {
                    indent: indentation(&line.text),
                };
                docs.header(rest.trim_start(), line.line, kind);
            } else {
                // An unknown directive is prose (`@see`, `@note`).
                prose.push(line.text.as_str());
            }
        }
        docs.fragments.extend(fragment);
        docs.description = description(explicit.as_deref().unwrap_or(&prose));
        docs
    }

    /// Reads the text after `@openapi` on the header line `line`.
    fn header(&mut self, rest: &str, line: usize, kind: ItemKind) {
        self.exported = true;
        let mut yaml = Vec::new();
        // The directive word ends at whitespace or a quote, so that
        // `rename_all` or `skipped` are not mistaken for `rename`/`skip`.
        let (word, argument) = rest
            .split_once(|c: char| c.is_whitespace() || c == '"')
            .map_or((rest, ""), |(word, _)| {
                (word, rest.get(word.len()..).unwrap_or_default().trim())
            });
        if word == "skip" {
            self.skip = true;
            if !argument.is_empty() && !argument.starts_with('#') {
                self.problems.push(DocLine::new(
                    format!("`@openapi skip` takes no argument; `{argument}` is ignored"),
                    line,
                ));
            }
        } else if word == "rename-all" {
            self.rename_all = RenameRule::parse(unquote(argument));
            if self.rename_all.is_none() {
                let rules: Vec<&str> = RenameRule::ALL.iter().map(|(name, _)| *name).collect();
                self.problems.push(DocLine::new(
                    format!(
                        "`@openapi rename-all` needs one of {}; `{argument}` is ignored",
                        rules.join(", ")
                    ),
                    line,
                ));
            }
        } else if word == "rename" {
            let name = unquote(argument);
            if name.is_empty() {
                self.problems
                    .push(DocLine::new("`@openapi rename` needs a name", line));
            } else {
                self.rename = Some(name.to_string());
            }
        } else if rest.starts_with('-') {
            // `@openapi-type Name` and friends mark the item; their
            // argument is not schema YAML.
        } else if kind == ItemKind::Container
            && let Some(generics) = rest.strip_prefix('<')
        {
            if let Some((params, after)) = generics.rsplit_once('>') {
                self.blueprint = Some(
                    params
                        .split(',')
                        .map(|p| p.trim().to_string())
                        .filter(|p| !p.is_empty())
                        .collect(),
                );
                let after = after.trim();
                if !after.is_empty() {
                    yaml.push(DocLine::new(after, line));
                }
            }
        } else if !rest.is_empty() {
            yaml.push(DocLine::new(rest, line));
        }
        self.overrides.push(yaml);
    }

    /// The override YAML of all `@openapi` lines, if any.
    pub(crate) fn override_text(&self) -> Option<String> {
        let lines: Vec<&str> = self
            .overrides
            .iter()
            .flatten()
            .map(|l| l.text.as_str())
            .collect();
        if lines.iter().all(|l| l.trim().is_empty()) {
            return None;
        }
        Some(lines.join("\n"))
    }

    /// The line of the first override line.
    pub(crate) fn override_line(&self) -> Option<usize> {
        self.overrides.iter().flatten().next().map(|l| l.line)
    }

    /// The override YAML of each `@openapi` line that has any, with the
    /// line where it starts.
    pub(crate) fn override_sections(&self) -> Vec<(String, usize)> {
        self.overrides
            .iter()
            .filter(|lines| lines.iter().any(|l| !l.text.trim().is_empty()))
            .filter_map(|lines| {
                let first = lines.first()?.line;
                let text: Vec<&str> = lines.iter().map(|l| l.text.as_str()).collect();
                Some((text.join("\n"), first))
            })
            .collect()
    }
}

/// `text` trimmed and without surrounding double quotes.
fn unquote(text: &str) -> &str {
    text.trim().trim_matches('"')
}
