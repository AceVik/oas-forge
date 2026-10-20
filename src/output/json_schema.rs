//! The component schemas as a JSON Schema 2020-12 bundle.

use super::refs::rewrite_refs;
use serde_json::{Map, Value, json};

/// The JSON Schema dialect of the bundle.
pub(super) const DIALECT: &str = "https://json-schema.org/draft/2020-12/schema";

/// `{"$schema": ..., "$defs": {...}}` with every component schema; references
/// point into `$defs`.
pub(super) fn bundle(canonical: &Value) -> Value {
    let mut definitions = canonical
        .pointer("/components/schemas")
        .cloned()
        .unwrap_or_else(|| Value::Object(Map::new()));
    rewrite_refs(&mut definitions, "#/components/schemas/", "#/$defs/");
    json!({ "$schema": DIALECT, "$defs": definitions })
}
