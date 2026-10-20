//! The only place that talks to the YAML library (see ADR 0009).
//!
//! Documents are represented as `serde_json::Value` everywhere else; YAML is an
//! input and output format only.

use serde_json::{Map, Number, Value};

/// A YAML syntax error.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct YamlError {
    /// The parser message, including the position inside the snippet.
    pub message: String,
}

/// Parses one YAML (or JSON) document.
///
/// Mapping keys that are not strings (`200:`) become their textual form, so
/// every document is representable as JSON.
pub(crate) fn parse(text: &str) -> Result<Value, YamlError> {
    let value: serde_yaml_ng::Value = serde_yaml_ng::from_str(text).map_err(|e| YamlError {
        message: e.to_string(),
    })?;
    Ok(to_json(value))
}

/// Serializes a value as a YAML document (no leading `---`).
pub(crate) fn to_string(value: &Value) -> String {
    let text = serde_yaml_ng::to_string(value).unwrap_or_default();
    match text.strip_prefix("---\n") {
        Some(rest) => rest.to_string(),
        None => text,
    }
}

fn to_json(value: serde_yaml_ng::Value) -> Value {
    use serde_yaml_ng::Value as Y;
    match value {
        Y::Null => Value::Null,
        Y::Bool(b) => Value::Bool(b),
        Y::Number(n) => number(&n),
        Y::String(s) => Value::String(s),
        Y::Sequence(items) => Value::Array(items.into_iter().map(to_json).collect()),
        Y::Mapping(mapping) => {
            let mut map = Map::with_capacity(mapping.len());
            for (key, value) in mapping {
                map.insert(key_text(key), to_json(value));
            }
            Value::Object(map)
        }
        Y::Tagged(tagged) => to_json(tagged.value),
    }
}

fn number(n: &serde_yaml_ng::Number) -> Value {
    if let Some(i) = n.as_i64() {
        Value::Number(i.into())
    } else if let Some(u) = n.as_u64() {
        Value::Number(u.into())
    } else {
        let f = n.as_f64().unwrap_or(f64::NAN);
        Number::from_f64(f).map_or_else(|| Value::String(n.to_string()), Value::Number)
    }
}

fn key_text(key: serde_yaml_ng::Value) -> String {
    use serde_yaml_ng::Value as Y;
    match key {
        Y::String(s) => s,
        Y::Number(n) => n.to_string(),
        Y::Bool(b) => b.to_string(),
        Y::Null => "null".to_string(),
        Y::Tagged(tagged) => key_text(tagged.value),
        other => to_string(&to_json(other)).trim_end().to_string(),
    }
}
