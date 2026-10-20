//! Smart references: whole string scalars `$Name` that name a known schema
//! become `#/components/schemas/<name>` (ADR 0001).

use super::lexical::ESCAPED_DOLLAR;
use super::registry::Registry;
use crate::diagnostics::{Code, Diagnostic, Diagnostics, Location};
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

/// Resolves smart references in `value`; unknown names are reported once.
pub fn resolve(
    value: &mut Value,
    known: &KnownSchemas,
    registry: &Registry,
    location: &Location,
    diagnostics: &mut Diagnostics,
) {
    let mut reported = HashSet::new();
    resolve_at(value, known, registry, location, diagnostics, &mut reported);
}

fn resolve_at(
    value: &mut Value,
    known: &KnownSchemas,
    registry: &Registry,
    location: &Location,
    diagnostics: &mut Diagnostics,
    reported: &mut HashSet<String>,
) {
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
                resolve_at(item, known, registry, location, diagnostics, reported);
            }
        }
        Value::Object(map) => {
            for child in map.values_mut() {
                resolve_at(child, known, registry, location, diagnostics, reported);
            }
        }
        _ => {}
    }
}

/// Turns protected `\$` escapes back into `$` (the very last text step).
pub fn unescape(value: &mut Value) {
    match value {
        Value::String(s) if s.contains(ESCAPED_DOLLAR) => {
            *s = s.replace(ESCAPED_DOLLAR, "$");
        }
        Value::Array(items) => items.iter_mut().for_each(unescape),
        Value::Object(map) => {
            if map.keys().any(|k| k.contains(ESCAPED_DOLLAR)) {
                let entries: Vec<_> = std::mem::take(map).into_iter().collect();
                for (key, mut child) in entries {
                    unescape(&mut child);
                    map.insert(key.replace(ESCAPED_DOLLAR, "$"), child);
                }
            } else {
                map.values_mut().for_each(unescape);
            }
        }
        _ => {}
    }
}
