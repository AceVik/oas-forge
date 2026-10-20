//! Rust types and their JSON Schema.
//!
//! Schemas are produced in the JSON Schema 2020-12 dialect of OpenAPI 3.1
//! (ADR 0002); [`dialect`] lowers them for other targets.

pub mod dialect;
pub(crate) mod shadow;
pub mod types;

pub use types::{MappedType, TypeExpr, TypeMapper};

use serde_json::{Value, json};

/// `schema` extended to also accept `null` (JSON Schema 2020-12 form).
///
/// - `{"type": "string"}` becomes `{"type": ["string", "null"]}` (an `enum`
///   gets a `null` entry),
/// - references and composed schemas become `{"anyOf": [schema, {"type": "null"}]}`,
/// - schemas that already accept `null`, and the empty schema, are unchanged.
///
/// # Examples
///
/// ```
/// use oas_forge::schema::nullable;
/// use serde_json::json;
///
/// assert_eq!(nullable(json!({ "type": "string" })), json!({ "type": ["string", "null"] }));
/// assert_eq!(
///     nullable(json!({ "type": "string", "enum": ["a", "b"] })),
///     json!({ "type": ["string", "null"], "enum": ["a", "b", null] })
/// );
/// assert_eq!(
///     nullable(json!({ "$ref": "#/components/schemas/User" })),
///     json!({ "anyOf": [{ "$ref": "#/components/schemas/User" }, { "type": "null" }] })
/// );
/// ```
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

/// Whether the schema already accepts `null`: a `null` type (alone or in a
/// type list) or a `{"type": "null"}` alternative in `anyOf`.
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

/// The inverse of [`nullable`]: `schema` without accepting `null`
/// (parameters cannot be `null`).
///
/// # Examples
///
/// ```
/// use oas_forge::schema::strip_null;
/// use serde_json::json;
///
/// assert_eq!(strip_null(json!({ "type": ["integer", "null"] })), json!({ "type": "integer" }));
/// assert_eq!(
///     strip_null(json!({
///         "anyOf": [{ "$ref": "#/components/schemas/User" }, { "type": "null" }],
///         "description": "The owner"
///     })),
///     json!({ "$ref": "#/components/schemas/User", "description": "The owner" })
/// );
/// ```
pub fn strip_null(schema: Value) -> Value {
    let Value::Object(mut map) = schema else {
        return schema;
    };
    if let Some(Value::Array(types)) = map.get_mut("type") {
        types.retain(|t| t != "null");
        if let [single] = types.as_slice() {
            let single = single.clone();
            map.insert("type".into(), single);
        }
        if let Some(Value::Array(values)) = map.get_mut("enum") {
            values.retain(|v| !v.is_null());
        }
        return Value::Object(map);
    }
    if let Some(Value::Array(alternatives)) = map.get("anyOf") {
        let others: Vec<&Value> = alternatives
            .iter()
            .filter(|a| **a != json!({ "type": "null" }))
            .collect();
        if others.len() < alternatives.len() {
            if let [single] = others.as_slice() {
                let single = (*single).clone();
                map.remove("anyOf");
                return match single {
                    Value::Object(inner) => {
                        let mut out = inner;
                        for (key, value) in map {
                            out.entry(key).or_insert(value);
                        }
                        Value::Object(out)
                    }
                    other => other,
                };
            }
            let others: Vec<Value> = others.into_iter().cloned().collect();
            map.insert("anyOf".into(), Value::Array(others));
        }
    }
    Value::Object(map)
}
