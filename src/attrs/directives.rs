//! `@openapi` directives in the doc comments of structs, enums, fields,
//! variants and type aliases.

use super::rename::RenameRule;
use crate::docs::{DocLine, description};

/// What the doc comment of a schema item (or of one of its fields/variants) says.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct ItemDocs {
    /// Documentation before the first `@openapi` line.
    pub description: String,
    /// Whether a line starts with `@openapi` (explicit export).
    pub exported: bool,
    /// `@openapi rename "Name"`
    pub rename: Option<String>,
    /// `@openapi rename-all rule`
    pub rename_all: Option<RenameRule>,
    /// `@openapi<T, U>`
    pub blueprint: Option<Vec<String>>,
    /// YAML override lines (text after a bare `@openapi`, and every following line).
    pub overrides: Vec<DocLine>,
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

impl ItemDocs {
    pub(crate) fn parse(lines: &[DocLine], kind: ItemKind) -> Self {
        let mut docs = Self::default();
        let mut description_lines = Vec::new();
        let mut in_block = false;
        // The open `@openapi-fragment` block, and the line of the last line.
        let mut fragment: Option<(DocLine, Vec<DocLine>)> = None;
        let mut last_line: Option<usize> = None;

        for line in lines {
            let trimmed = line.trimmed();
            // A fragment block ends at the next `@openapi…`/`@example` line or
            // where the doc comment is interrupted (a blank line between two
            // `///` groups), like the blocks of file comments.
            let gap = last_line.is_some_and(|last| line.line > last + 1);
            last_line = Some(line.line);
            if let Some((header, mut body)) = fragment.take() {
                if gap || trimmed.starts_with("@openapi") || trimmed.starts_with("@example") {
                    docs.fragments.push((header, body));
                } else {
                    body.push(line.clone());
                    fragment = Some((header, body));
                    continue;
                }
            }
            if trimmed.starts_with("@openapi-fragment") {
                fragment = Some((DocLine::new(trimmed, line.line), Vec::new()));
                continue;
            }
            if let Some(example) = trimmed.strip_prefix("@example")
                && (example.is_empty() || example.starts_with(char::is_whitespace))
            {
                docs.examples.push(DocLine::new(example.trim(), line.line));
                continue;
            }
            let Some(rest) = trimmed.strip_prefix("@openapi") else {
                if in_block {
                    docs.overrides.push(line.clone());
                } else {
                    description_lines.push(line.text.as_str());
                }
                continue;
            };
            in_block = true;
            docs.exported = true;
            let rest = rest.trim_start();

            // The directive word ends at whitespace or a quote, so that
            // `rename_all` or `skipped` are not mistaken for `rename`/`skip`.
            let (word, argument) = rest
                .split_once(|c: char| c.is_whitespace() || c == '"')
                .map_or((rest, ""), |(word, _)| {
                    (word, rest.get(word.len()..).unwrap_or_default().trim())
                });
            if word == "skip" {
                docs.skip = true;
                if !argument.is_empty() && !argument.starts_with('#') {
                    docs.problems.push(DocLine::new(
                        format!("`@openapi skip` takes no argument; `{argument}` is ignored"),
                        line.line,
                    ));
                }
            } else if word == "rename-all" {
                docs.rename_all = RenameRule::parse(unquote(argument));
                if docs.rename_all.is_none() {
                    let rules: Vec<&str> = RenameRule::ALL.iter().map(|(name, _)| *name).collect();
                    docs.problems.push(DocLine::new(
                        format!(
                            "`@openapi rename-all` needs one of {}; `{argument}` is ignored",
                            rules.join(", ")
                        ),
                        line.line,
                    ));
                }
            } else if word == "rename" {
                let name = unquote(argument);
                if name.is_empty() {
                    docs.problems
                        .push(DocLine::new("`@openapi rename` needs a name", line.line));
                } else {
                    docs.rename = Some(name.to_string());
                }
            } else if rest.starts_with('-') {
                // `@openapi-type Name` and friends mark the item; their
                // argument is not schema YAML.
            } else if kind == ItemKind::Container
                && let Some(generics) = rest.strip_prefix('<')
            {
                if let Some((params, after)) = generics.rsplit_once('>') {
                    docs.blueprint = Some(
                        params
                            .split(',')
                            .map(|p| p.trim().to_string())
                            .filter(|p| !p.is_empty())
                            .collect(),
                    );
                    let after = after.trim();
                    if !after.is_empty() {
                        docs.overrides.push(DocLine::new(after, line.line));
                    }
                }
            } else if !rest.is_empty() {
                docs.overrides.push(DocLine::new(rest, line.line));
            }
        }
        docs.fragments.extend(fragment);
        docs.description = description(&description_lines);
        docs
    }

    /// The override YAML, if any.
    pub(crate) fn override_text(&self) -> Option<String> {
        if self.overrides.iter().all(|l| l.text.trim().is_empty()) {
            return None;
        }
        Some(
            self.overrides
                .iter()
                .map(|l| l.text.as_str())
                .collect::<Vec<_>>()
                .join("\n"),
        )
    }

    /// The line of the first override line.
    pub(crate) fn override_line(&self) -> Option<usize> {
        self.overrides.first().map(|l| l.line)
    }
}

fn unquote(text: &str) -> &str {
    text.trim().trim_matches('"')
}
