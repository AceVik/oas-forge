//! Parameters: `@param <in> name: Type [flags] ["description"]`, the short
//! forms `@path-param`, `@query-param`, `@header-param`, `@cookie-param`,
//! `@querystring-param`, inline path parameters and struct expansion
//! (`@query-params $Pagination`).

use super::tokens::{tokenize, unquote};
use crate::attrs::validate::{Constraints, LengthUnit, Rule};
use crate::schema::{TypeExpr, TypeMapper};
use crate::yaml;
use serde_json::{Number, Value, json};

/// The marker a struct expansion leaves in `parameters` until the struct's
/// schema is known (see `generator`).
pub(crate) const EXPAND_KEY: &str = "x-oas-forge-expand";

/// Parameter locations.
pub(super) const LOCATIONS: &[&str] = &["path", "query", "header", "cookie", "querystring"];

/// A parsed parameter directive.
pub(super) struct Parameter {
    pub(super) name: String,
    pub(super) value: Value,
    /// Tokens that are neither a known flag nor a quoted description; they
    /// are reported and ignored.
    pub(super) ignored: Vec<String>,
}

/// Parses `name: Type [flags] ["description"]` for location `location`;
/// `None` when the text is not `name: ...`.
pub(super) fn parse(location: &str, rest: &str, types: &TypeMapper) -> Option<Parameter> {
    let (name, spec) = rest.split_once(':')?;
    let name = name.trim();
    if name.is_empty() || name.contains(char::is_whitespace) {
        return None;
    }
    let tokens = tokenize(spec);
    let first = tokens.first().map(String::as_str).unwrap_or_default();
    let (type_text, flags) = if first.is_empty() || is_flag(first) {
        ("String", &tokens[..])
    } else {
        (first, &tokens[1..])
    };
    let (mut schema, mut required) = match TypeExpr::parse(type_text) {
        Some(t) => {
            let mapped = types.map(&t);
            (mapped.schema, mapped.required)
        }
        None => (json!({ "type": "string" }), true),
    };

    let mut parameter = serde_json::Map::new();
    let mut description: Vec<&str> = Vec::new();
    let mut constraints = Vec::new();
    let mut ignored = Vec::new();
    let mut has_default = false;
    let mut explicitly_required = false;
    for token in flags {
        if !description.is_empty() || token.starts_with('"') {
            description.push(token);
            continue;
        }
        let (flag, value) = match token.split_once('=') {
            Some((flag, value)) => (flag, Some(value)),
            None => (token.as_str(), None),
        };
        let parsed = || value.map(scalar);
        match (flag, value) {
            ("required", None) => explicitly_required = true,
            ("deprecated", None) => {
                parameter.insert("deprecated".into(), json!(true));
            }
            ("explode", None) => {
                parameter.insert("explode".into(), json!(true));
            }
            ("explode", Some(_)) => {
                parameter.insert("explode".into(), parsed().unwrap_or_default());
            }
            ("allow-reserved", None) => {
                parameter.insert("allowReserved".into(), json!(true));
            }
            ("allow-empty", None) => {
                parameter.insert("allowEmptyValue".into(), json!(true));
            }
            ("style", Some(style)) => {
                parameter.insert("style".into(), json!(text(style)));
            }
            ("example", Some(_)) => {
                parameter.insert("example".into(), parsed().unwrap_or_default());
            }
            ("default", Some(_)) => {
                schema["default"] = parsed().unwrap_or_default();
                has_default = true;
            }
            ("enum", Some(_)) => match parsed() {
                Some(Value::Array(values)) => schema["enum"] = Value::Array(values),
                _ => ignored.push(token.clone()),
            },
            ("format", Some(format)) => schema["format"] = json!(text(format)),
            ("pattern", Some(pattern)) => {
                constraints.push(Rule::Pattern(text(pattern).to_string()));
            }
            ("min" | "max", Some(_)) => match parsed() {
                Some(Value::Number(number)) => {
                    constraints.push(bound(&schema, flag == "min", number))
                }
                _ => ignored.push(token.clone()),
            },
            _ => ignored.push(token.clone()),
        }
    }
    Constraints::new(constraints).apply(&mut schema);

    // A parameter with a default may be left out, unless marked `required`.
    required = (required && !has_default) || explicitly_required || location == "path";
    let mut value = json!({ "name": name, "in": location, "required": required });
    if !description.is_empty() {
        let text = description.join(" ");
        value["description"] = json!(unquote(&text).unwrap_or(&text));
    }
    if location == "querystring" {
        value["content"] = json!({ "application/x-www-form-urlencoded": { "schema": schema } });
    } else {
        value["schema"] = schema;
    }
    if let Value::Object(map) = &mut value {
        map.extend(parameter);
    }
    Some(Parameter {
        name: name.to_string(),
        value,
        ignored,
    })
}

/// A path parameter declared inline: `{id: u64 "User id"}`.
pub(super) fn inline_path(
    name: &str,
    type_text: Option<&str>,
    description: Option<&str>,
    types: &TypeMapper,
) -> Value {
    let schema = TypeExpr::parse(type_text.unwrap_or("String"))
        .map_or_else(|| json!({ "type": "string" }), |t| types.map(&t).schema);
    let mut param = json!({ "name": name, "in": "path", "required": true });
    if let Some(d) = description {
        param["description"] = json!(d);
    }
    param["schema"] = schema;
    param
}

/// `@query-params $Pagination`: one parameter per property of the struct,
/// expanded once the struct's schema is known.
pub(super) fn expansion(location: &str, type_text: &str) -> Value {
    let name = type_text.trim().trim_start_matches('$');
    json!({ EXPAND_KEY: { "in": location, "schema": format!("${name}") } })
}

fn is_flag(token: &str) -> bool {
    token.starts_with('"')
        || matches!(
            token,
            "required" | "deprecated" | "explode" | "allow-reserved" | "allow-empty"
        )
        || token.split_once('=').is_some_and(|(flag, _)| {
            matches!(
                flag,
                "example"
                    | "default"
                    | "enum"
                    | "min"
                    | "max"
                    | "pattern"
                    | "format"
                    | "style"
                    | "explode"
            )
        })
}

/// A flag value as YAML (`10` is a number, `[a, b]` a list), falling back to
/// the text.
fn scalar(value: &str) -> Value {
    match yaml::parse(value) {
        Ok(Value::Null) | Err(_) => json!(text(value)),
        Ok(value) => value,
    }
}

/// `min=`/`max=` as a range for numbers and a length otherwise.
fn bound(schema: &Value, lower: bool, number: Number) -> Rule {
    let numeric = matches!(
        schema.get("type").and_then(Value::as_str),
        Some("integer" | "number")
    );
    if numeric {
        let (min, max) = if lower {
            (Some(number), None)
        } else {
            (None, Some(number))
        };
        Rule::Range {
            min,
            max,
            exclusive_min: None,
            exclusive_max: None,
        }
    } else {
        let n = number.as_u64();
        let (min, max) = if lower { (n, None) } else { (None, n) };
        Rule::Length {
            min,
            max,
            unit: LengthUnit::Chars,
        }
    }
}

/// A flag value without its surrounding quotes.
fn text(value: &str) -> &str {
    unquote(value).unwrap_or(value)
}
