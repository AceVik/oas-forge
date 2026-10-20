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

        for line in lines {
            let trimmed = line.trimmed();
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

            if rest == "skip" {
                docs.skip = true;
            } else if let Some(rule) = rest.strip_prefix("rename-all") {
                docs.rename_all = RenameRule::parse(unquote(rule));
            } else if let Some(name) = rest.strip_prefix("rename") {
                docs.rename = Some(unquote(name).to_string());
            } else if rest.starts_with('-') {
                // `@openapi-type Name` and friends mark the item; their
                // argument is not schema YAML.
            } else if kind == ItemKind::Container && rest.starts_with('<') {
                if let Some(end) = rest.rfind('>') {
                    docs.blueprint = Some(
                        rest[1..end]
                            .split(',')
                            .map(|p| p.trim().to_string())
                            .filter(|p| !p.is_empty())
                            .collect(),
                    );
                    let after = rest[end + 1..].trim();
                    if !after.is_empty() {
                        docs.overrides.push(DocLine::new(after, line.line));
                    }
                }
            } else if !rest.is_empty() {
                docs.overrides.push(DocLine::new(rest, line.line));
            }
        }
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
