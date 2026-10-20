//! Rust types and their JSON Schema.
//!
//! Schemas are produced in the JSON Schema 2020-12 dialect of OpenAPI 3.1
//! (ADR 0002); [`dialect`] lowers them for other targets.

pub mod dialect;
pub mod types;

pub use types::{MappedType, TypeExpr, TypeMapper};

use serde_json::{Value, json};

/// `schema` extended to also accept `null` (JSON Schema 2020-12 form).
///
/// - `{"type": "string"}` becomes `{"type": ["string", "null"]}` (an `enum`
///   gets a `null` entry),
/// - references and composed schemas become `{"anyOf": [schema, {"type": "null"}]}`,
/// - schemas that already accept `null`, and the empty schema, are unchanged.
pub fn nullable(schema: Value) -> Value {
    let Value::Object(mut map) = schema else {
        return schema;
    };
    if map.is_empty() || accepts_null(&map) {
        return Value::Object(map);
    }
    let simple = !map.contains_key("$ref")
        && !["anyOf", "oneOf", "allOf", "not", "const"]
            .iter()
            .any(|k| map.contains_key(*k));
    match map.get_mut("type") {
        Some(Value::String(t)) if simple => {
            let t = std::mem::take(t);
            map.insert("type".into(), json!([t, "null"]));
        }
        Some(Value::Array(types)) if simple => types.push(json!("null")),
        _ => return json!({ "anyOf": [Value::Object(map), { "type": "null" }] }),
    }
    if let Some(Value::Array(values)) = map.get_mut("enum") {
        values.push(Value::Null);
    }
    Value::Object(map)
}

fn accepts_null(map: &serde_json::Map<String, Value>) -> bool {
    match map.get("type") {
        Some(Value::String(t)) => t == "null",
        Some(Value::Array(types)) => types.iter().any(|t| t == "null"),
        _ => map
            .get("anyOf")
            .and_then(Value::as_array)
            .is_some_and(|alternatives| {
                alternatives.iter().any(|a| *a == json!({ "type": "null" }))
            }),
    }
}
