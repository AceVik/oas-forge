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
#[derive(Debug, Clone, Default)]
pub struct KnownSchemas {
    names: BTreeSet<String>,
}

impl KnownSchemas {
    /// Adds a schema name.
    pub fn insert(&mut self, name: impl Into<String>) {
        self.names.insert(name.into());
    }

    /// Whether `name` is known.
    pub fn contains(&self, name: &str) -> bool {
        self.names.contains(name)
    }

    /// Every `components.schemas` name defined in `document`.
    pub fn collect_from(&mut self, document: &Value) {
        if let Some(schemas) = document
            .pointer("/components/schemas")
            .and_then(Value::as_object)
        {
            self.names.extend(schemas.keys().cloned());
        }
    }
}

fn reference_name(text: &str) -> Option<&str> {
    let name = text.strip_prefix('$')?;
    let mut chars = name.chars();
    let first = chars.next()?;
    ((first.is_alphabetic() || first == '_') && chars.all(|c| c.is_alphanumeric() || c == '_'))
        .then_some(name)
}

/// What smart references may resolve to.
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

fn restore(text: &str, regex: bool) -> String {
    text.replace(ESCAPED_DOLLAR, if regex { "\\$" } else { "$" })
}
