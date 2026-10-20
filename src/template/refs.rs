//! Smart references: whole string scalars `$Name` that name a known schema
//! become `#/components/schemas/<name>` (ADR 0001). A `{"$ref": "$Name"}`
//! that names a built-in type (`$String`, `$Uuid`, e.g. from a blueprint
//! instantiated with `$Page<String>`) becomes that type's schema.

use super::lexical::ESCAPED_DOLLAR;
use super::registry::Registry;
use crate::diagnostics::{Code, Diagnostic, Diagnostics, Location};
use crate::schema::{TypeExpr, TypeMapper, shadow};
use serde_json::Value;
use std::collections::{BTreeSet, HashSet};

/// The schema names smart references may resolve to.
///
/// # Examples
///
/// ```
/// use oas_forge::template::refs::KnownSchemas;
/// use serde_json::json;
///
/// let mut known = KnownSchemas::default();
/// known.collect_from(&json!({ "components": { "schemas": { "User": {} } } }));
/// known.insert("Page_User");
/// assert!(known.contains("User") && known.contains("Page_User"));
/// ```
#[derive(Debug, Clone, Default)]
pub struct KnownSchemas {
    /// The names, sorted.
    names: BTreeSet<String>,
}

impl KnownSchemas {
    /// Adds a schema name.
    ///
    /// # Examples
    ///
    /// ```
    /// use oas_forge::template::refs::KnownSchemas;
    ///
    /// let mut known = KnownSchemas::default();
    /// known.insert("User");
    /// assert!(known.contains("User"));
    /// ```
    pub fn insert(&mut self, name: impl Into<String>) {
        self.names.insert(name.into());
    }

    /// Whether `name` is known.
    ///
    /// # Examples
    ///
    /// ```
    /// use oas_forge::template::refs::KnownSchemas;
    ///
    /// let mut known = KnownSchemas::default();
    /// known.insert("User");
    /// assert!(known.contains("User"));
    /// assert!(!known.contains("user"));
    /// ```
    pub fn contains(&self, name: &str) -> bool {
        self.names.contains(name)
    }

    /// Every `components.schemas` name defined in `document`.
    ///
    /// # Examples
    ///
    /// ```
    /// use oas_forge::template::refs::KnownSchemas;
    /// use serde_json::json;
    ///
    /// let mut known = KnownSchemas::default();
    /// known.collect_from(&json!({
    ///     "paths": {},
    ///     "components": { "schemas": { "User": {}, "Team": {} } }
    /// }));
    /// assert!(known.contains("Team"));
    /// assert!(!known.contains("paths"));
    /// ```
    pub fn collect_from(&mut self, document: &Value) {
        if let Some(schemas) = document
            .pointer("/components/schemas")
            .and_then(Value::as_object)
        {
            self.names.extend(schemas.keys().cloned());
        }
    }
}

/// The name of a smart reference: `text` is `$` followed by an identifier
/// (a letter or `_`, then letters, digits or `_`). `None` for other text,
/// e.g. a generic reference (`$Page<User>`) or prose.
fn reference_name(text: &str) -> Option<&str> {
    let name = text.strip_prefix('$')?;
    let mut chars = name.chars();
    let first = chars.next()?;
    ((first.is_alphabetic() || first == '_') && chars.all(|c| c.is_alphanumeric() || c == '_'))
        .then_some(name)
}

/// What smart references may resolve to.
///
/// # Examples
///
/// ```
/// use oas_forge::diagnostics::{Diagnostics, Location};
/// use oas_forge::schema::TypeMapper;
/// use oas_forge::template::Registry;
/// use oas_forge::template::refs::{self, KnownSchemas, Targets};
/// use serde_json::json;
///
/// let mut known = KnownSchemas::default();
/// known.insert("User");
/// let mut registry = Registry::new();
/// // `#[serde(rename = "User")] struct UserDto`
/// registry.add_alias("UserDto", "User");
/// let targets = Targets { known: &known, registry: &registry, types: TypeMapper::builtin() };
///
/// let mut value = json!({ "$ref": "$UserDto" });
/// let mut diagnostics = Diagnostics::new();
/// refs::resolve(&mut value, targets, &Location::new("src/lib.rs", 1), &mut diagnostics);
/// assert_eq!(value, json!({ "$ref": "#/components/schemas/User" }));
/// ```
#[derive(Debug, Clone, Copy)]
pub struct Targets<'a> {
    /// The documented schema names.
    pub known: &'a KnownSchemas,
    /// Aliases from Rust names to schema names.
    pub registry: &'a Registry,
    /// The type table for built-in types.
    pub types: &'a TypeMapper,
}

/// Resolves smart references in `value`; unknown names are reported once.
///
/// Every whole string `$Name` that names a known schema, directly or by the
/// Rust name of a renamed type, becomes a pointer to that schema. A `{"$ref": "$Name"}` of a
/// built-in type is replaced by its schema; one of an unknown name is
/// reported and still becomes a pointer, so the lints see it.
///
/// # Examples
///
/// ```
/// use oas_forge::diagnostics::{Code, Diagnostics, Location};
/// use oas_forge::schema::TypeMapper;
/// use oas_forge::template::Registry;
/// use oas_forge::template::refs::{self, KnownSchemas, Targets};
/// use serde_json::json;
///
/// let mut known = KnownSchemas::default();
/// known.insert("User");
/// let registry = Registry::new();
/// let targets = Targets { known: &known, registry: &registry, types: TypeMapper::builtin() };
///
/// let mut value = json!({
///     "properties": {
///         "owner": { "$ref": "$User" },
///         "id": { "$ref": "$Uuid" },
///         "group": { "$ref": "$Group" }
///     }
/// });
/// let mut diagnostics = Diagnostics::new();
/// refs::resolve(&mut value, targets, &Location::new("src/lib.rs", 4), &mut diagnostics);
///
/// assert_eq!(
///     value,
///     json!({
///         "properties": {
///             "owner": { "$ref": "#/components/schemas/User" },
///             "id": { "type": "string", "format": "uuid" },
///             "group": { "$ref": "#/components/schemas/Group" }
///         }
///     })
/// );
/// let codes: Vec<Code> = diagnostics.iter().map(|d| d.code()).collect();
/// assert_eq!(codes, [Code::UnresolvedReference]);
/// ```
pub fn resolve(
    value: &mut Value,
    targets: Targets<'_>,
    location: &Location,
    diagnostics: &mut Diagnostics,
) {
    let mut reported = HashSet::new();
    resolve_at(value, targets, location, diagnostics, &mut reported);
}

/// The schema of a built-in type named by `{"$ref": "$Name"}`.
fn builtin(name: &str, targets: Targets<'_>) -> Option<serde_json::Map<String, Value>> {
    if targets.known.contains(name) || targets.registry.alias(name).is_some() {
        return None;
    }
    let schema = targets.types.map(&TypeExpr::parse(name)?).into_schema();
    match schema {
        Value::Object(map) if !map.contains_key("$ref") => Some(map),
        _ => None,
    }
}

/// [`resolve`] at `value` and below. `reported` holds the unknown names
/// reported so far. In a mapping, the built-in alternatives are chosen
/// first ([`shadow::choose`]), then a `$ref` of a built-in type is
/// replaced, then the children are resolved.
fn resolve_at(
    value: &mut Value,
    targets: Targets<'_>,
    location: &Location,
    diagnostics: &mut Diagnostics,
    reported: &mut HashSet<String>,
) {
    let Targets {
        known, registry, ..
    } = targets;
    match value {
        Value::String(s) => {
            let Some(name) = reference_name(s) else {
                return;
            };
            let target = if known.contains(name) {
                Some(name.to_string())
            } else {
                registry
                    .alias(name)
                    .filter(|alias| known.contains(alias))
                    .map(str::to_string)
            };
            match target {
                Some(target) => *s = format!("#/components/schemas/{target}"),
                None => {
                    if reported.insert(name.to_string()) {
                        diagnostics.push(
                            Diagnostic::new(
                                Code::UnresolvedReference,
                                format!("`${name}` does not name a known schema"),
                            )
                            .at(location.clone()),
                        );
                    }
                }
            }
        }
        Value::Array(items) => {
            for item in items {
                resolve_at(item, targets, location, diagnostics, reported);
            }
        }
        Value::Object(map) => {
            // A bare built-in name (`Duration`) that is also a documented
            // schema refers to that schema.
            shadow::choose(map, |name| {
                known.contains(name)
                    || registry
                        .alias(name)
                        .is_some_and(|alias| known.contains(alias))
            });
            let schema = map
                .get("$ref")
                .and_then(Value::as_str)
                .and_then(reference_name)
                .and_then(|name| builtin(name, targets));
            if let Some(schema) = schema {
                map.remove("$ref");
                for (key, value) in schema {
                    map.entry(key).or_insert(value);
                }
            }
            for child in map.values_mut() {
                resolve_at(child, targets, location, diagnostics, reported);
            }
            // An unknown name (reported above) still becomes a pointer, so
            // the reference stays well-formed and the lints see it.
            if let Some(Value::String(target)) = map.get_mut("$ref")
                && let Some(name) = reference_name(target)
            {
                *target = format!("#/components/schemas/{name}");
            }
        }
        _ => {}
    }
}

/// Turns protected `\$` escapes back into `$` (the very last text step).
/// Regular expressions keep `\$`, their escape of a literal dollar sign:
/// `pattern` values and the keys of `patternProperties`.
///
/// # Examples
///
/// ```
/// use oas_forge::template::lexical::ESCAPED_DOLLAR;
/// use oas_forge::template::refs;
/// use serde_json::json;
///
/// // `\$5` and `^\$[0-9]+$` after the text stage:
/// let mut value = json!({
///     "description": format!("Costs {ESCAPED_DOLLAR}5"),
///     "pattern": format!("^{ESCAPED_DOLLAR}[0-9]+$")
/// });
/// refs::unescape(&mut value);
/// assert_eq!(value, json!({ "description": "Costs $5", "pattern": "^\\$[0-9]+$" }));
/// ```
pub fn unescape(value: &mut Value) {
    unescape_at(value, false);
}

/// `regex`: a string `value` is a regular expression (the value of
/// `pattern`), or the keys of an object `value` are (`patternProperties`).
fn unescape_at(value: &mut Value, regex: bool) {
    match value {
        Value::String(s) if s.contains(ESCAPED_DOLLAR) => *s = restore(s, regex),
        Value::Array(items) => items.iter_mut().for_each(|item| unescape_at(item, false)),
        Value::Object(map) => {
            if map.keys().any(|k| k.contains(ESCAPED_DOLLAR)) {
                let entries: Vec<_> = std::mem::take(map).into_iter().collect();
                for (key, mut child) in entries {
                    unescape_child(&key, &mut child);
                    map.insert(restore(&key, regex), child);
                }
            } else {
                for (key, child) in map.iter_mut() {
                    unescape_child(key, child);
                }
            }
        }
        _ => {}
    }
}

/// Unescapes the value of `key`; the keys of `patternProperties` and the
/// value of `pattern` are regular expressions.
fn unescape_child(key: &str, child: &mut Value) {
    match child {
        Value::Object(_) => unescape_at(child, key == "patternProperties"),
        _ => unescape_at(child, key == "pattern"),
    }
}

/// `text` with each [`ESCAPED_DOLLAR`] turned back into `$`, or into `\$`
/// in a regular expression.
fn restore(text: &str, regex: bool) -> String {
    text.replace(ESCAPED_DOLLAR, if regex { "\\$" } else { "$" })
}
