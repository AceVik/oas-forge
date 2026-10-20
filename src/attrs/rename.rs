//! Case conversion rules of `rename_all` / `@openapi rename-all`.

use std::fmt;

/// A `rename_all` rule.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum RenameRule {
    /// `lowercase`
    Lower,
    /// `UPPERCASE`
    Upper,
    /// `PascalCase`
    Pascal,
    /// `camelCase`
    Camel,
    /// `snake_case`
    Snake,
    /// `SCREAMING_SNAKE_CASE`
    ScreamingSnake,
    /// `kebab-case`
    Kebab,
    /// `SCREAMING-KEBAB-CASE`
    ScreamingKebab,
}

impl RenameRule {
    /// Every rule with its serde spelling.
    pub const ALL: [(&'static str, RenameRule); 8] = [
        ("lowercase", Self::Lower),
        ("UPPERCASE", Self::Upper),
        ("PascalCase", Self::Pascal),
        ("camelCase", Self::Camel),
        ("snake_case", Self::Snake),
        ("SCREAMING_SNAKE_CASE", Self::ScreamingSnake),
        ("kebab-case", Self::Kebab),
        ("SCREAMING-KEBAB-CASE", Self::ScreamingKebab),
    ];

    /// Parses the serde spelling of a rule (`"camelCase"`).
    pub fn parse(text: &str) -> Option<Self> {
        Self::ALL
            .iter()
            .find(|(name, _)| *name == text)
            .map(|(_, rule)| *rule)
    }

    /// The serde spelling of the rule.
    pub fn as_str(self) -> &'static str {
        Self::ALL
            .iter()
            .find(|(_, rule)| *rule == self)
            .map(|(name, _)| *name)
            .unwrap_or_default()
    }

    /// Applies the rule to a name.
    ///
    /// The conversion accepts both field-style (`snake_case`) and
    /// variant-style (`PascalCase`) input.
    pub fn apply(self, name: &str) -> String {
        match self {
            Self::Lower => name.to_lowercase(),
            Self::Upper => name.to_uppercase(),
            Self::Pascal => {
                if name.contains('_') {
                    name.split('_').map(capitalize).collect()
                } else {
                    capitalize(name)
                }
            }
            Self::Camel => {
                if name.contains('_') {
                    let mut parts = name.split('_');
                    let first = parts.next().unwrap_or_default().to_lowercase();
                    first + &parts.map(capitalize).collect::<String>()
                } else {
                    let mut chars = name.chars();
                    match chars.next() {
                        Some(f) => f.to_lowercase().chain(chars).collect(),
                        None => String::new(),
                    }
                }
            }
            Self::Snake => {
                let mut out = String::with_capacity(name.len() + 4);
                for (i, c) in name.chars().enumerate() {
                    if c.is_uppercase() && i > 0 {
                        out.push('_');
                    }
                    out.extend(c.to_lowercase());
                }
                out
            }
            Self::ScreamingSnake => Self::Snake.apply(name).to_uppercase(),
            Self::Kebab => Self::Snake.apply(name).replace('_', "-"),
            Self::ScreamingKebab => Self::Kebab.apply(name).to_uppercase(),
        }
    }
}

impl fmt::Display for RenameRule {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

fn capitalize(part: &str) -> String {
    let mut chars = part.chars();
    match chars.next() {
        Some(f) => f.to_uppercase().chain(chars).collect(),
        None => String::new(),
    }
}
