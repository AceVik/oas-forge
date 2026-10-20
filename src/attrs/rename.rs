//! Case conversion rules of `rename_all` / `@openapi rename-all`.

use std::fmt;

/// A `rename_all` rule.
///
/// # Examples
///
/// ```
/// use oas_forge::attrs::RenameRule;
///
/// let rule = RenameRule::parse("camelCase").ok_or("unknown rule")?;
/// assert_eq!(rule, RenameRule::Camel);
/// assert_eq!(rule.apply_to_field("user_id"), "userId");
/// assert_eq!(rule.apply_to_variant("ReadOnly"), "readOnly");
/// assert_eq!(rule.to_string(), "camelCase");
/// # Ok::<(), Box<dyn std::error::Error>>(())
/// ```
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
    ///
    /// # Examples
    ///
    /// ```
    /// use oas_forge::attrs::RenameRule;
    ///
    /// assert_eq!(RenameRule::parse("SCREAMING_SNAKE_CASE"), Some(RenameRule::ScreamingSnake));
    /// // Only the exact spellings serde accepts.
    /// assert_eq!(RenameRule::parse("camelcase"), None);
    /// assert_eq!(RenameRule::parse("Train-Case"), None);
    /// ```
    pub fn parse(text: &str) -> Option<Self> {
        Self::ALL
            .iter()
            .find(|(name, _)| *name == text)
            .map(|(_, rule)| *rule)
    }

    /// The serde spelling of the rule.
    ///
    /// # Examples
    ///
    /// ```
    /// use oas_forge::attrs::RenameRule;
    ///
    /// assert_eq!(RenameRule::ScreamingKebab.as_str(), "SCREAMING-KEBAB-CASE");
    /// for (name, rule) in RenameRule::ALL {
    ///     assert_eq!(rule.as_str(), name);
    /// }
    /// ```
    pub fn as_str(self) -> &'static str {
        Self::ALL
            .iter()
            .find(|(_, rule)| *rule == self)
            .map(|(name, _)| *name)
            .unwrap_or_default()
    }

    /// Applies the rule to an enum variant name (`PascalCase` in Rust), exactly
    /// like serde does.
    ///
    /// # Examples
    ///
    /// ```
    /// use oas_forge::attrs::RenameRule;
    ///
    /// assert_eq!(RenameRule::Snake.apply_to_variant("VeryTasty"), "very_tasty");
    /// assert_eq!(RenameRule::Kebab.apply_to_variant("VeryTasty"), "very-tasty");
    /// // Every uppercase letter starts a word, like in serde.
    /// assert_eq!(RenameRule::Snake.apply_to_variant("HTTPServer"), "h_t_t_p_server");
    /// ```
    pub fn apply_to_variant(self, variant: &str) -> String {
        match self {
            Self::Pascal => variant.to_owned(),
            Self::Lower => variant.to_ascii_lowercase(),
            Self::Upper => variant.to_ascii_uppercase(),
            Self::Camel => lower_first(variant),
            Self::Snake => {
                let mut snake = String::with_capacity(variant.len() + 4);
                for (i, ch) in variant.char_indices() {
                    if i > 0 && ch.is_uppercase() {
                        snake.push('_');
                    }
                    snake.push(ch.to_ascii_lowercase());
                }
                snake
            }
            Self::ScreamingSnake => Self::Snake.apply_to_variant(variant).to_ascii_uppercase(),
            Self::Kebab => Self::Snake.apply_to_variant(variant).replace('_', "-"),
            Self::ScreamingKebab => Self::ScreamingSnake
                .apply_to_variant(variant)
                .replace('_', "-"),
        }
    }

    /// Applies the rule to a struct field name (`snake_case` in Rust), exactly
    /// like serde does.
    ///
    /// # Examples
    ///
    /// ```
    /// use oas_forge::attrs::RenameRule;
    ///
    /// assert_eq!(RenameRule::Pascal.apply_to_field("very_tasty"), "VeryTasty");
    /// assert_eq!(RenameRule::ScreamingKebab.apply_to_field("very_tasty"), "VERY-TASTY");
    /// // Names that are not `snake_case` keep their case, like in serde.
    /// assert_eq!(RenameRule::Snake.apply_to_field("lastName"), "lastName");
    /// ```
    pub fn apply_to_field(self, field: &str) -> String {
        match self {
            Self::Lower | Self::Snake => field.to_owned(),
            Self::Upper | Self::ScreamingSnake => field.to_ascii_uppercase(),
            Self::Pascal => {
                let mut pascal = String::with_capacity(field.len());
                let mut capitalize = true;
                for ch in field.chars() {
                    if ch == '_' {
                        capitalize = true;
                    } else if capitalize {
                        pascal.push(ch.to_ascii_uppercase());
                        capitalize = false;
                    } else {
                        pascal.push(ch);
                    }
                }
                pascal
            }
            Self::Camel => lower_first(&Self::Pascal.apply_to_field(field)),
            Self::Kebab => field.replace('_', "-"),
            Self::ScreamingKebab => field.to_ascii_uppercase().replace('_', "-"),
        }
    }
}

/// The text with its first character in ASCII lowercase.
fn lower_first(text: &str) -> String {
    let mut chars = text.chars();
    match chars.next() {
        Some(first) => first.to_ascii_lowercase().to_string() + chars.as_str(),
        None => String::new(),
    }
}

impl fmt::Display for RenameRule {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}
