//! Struct expansion of parameters (`@query-params $Pagination`, inferred
//! `Query<T>` and `Path<T>`), run on the merged document once every
//! component schema is known.

use super::EXPAND_KEY;
use crate::diagnostics::{Code, Diagnostic, Diagnostics};
use crate::document::for_each_operation;
use crate::schema::strip_null;
use serde_json::{Map, Value, json};

/// Replaces every expansion marker with one parameter per property of the
/// referenced schema, then checks that path placeholders are declared.
pub(crate) fn expand_parameters(document: &mut Value, diagnostics: &mut Diagnostics) {
    let schemas = document
        .pointer("/components/schemas")
        .cloned()
        .unwrap_or_else(|| json!({}));
    for_each_operation(document, |at, operation| {
        let Some(Value::Array(parameters)) = operation.get_mut("parameters") else {
            return;
        };
        if !parameters.iter().any(|p| p.get(EXPAND_KEY).is_some()) {
            return;
        }
        // Explicit parameters win over expanded ones, wherever they are listed.
        let explicit: Vec<(Value, Value)> = parameters
            .iter()
            .filter(|p| p.get(EXPAND_KEY).is_none())
            .map(|p| (p["name"].clone(), p["in"].clone()))
            .collect();
        let mut expanded = Vec::with_capacity(parameters.len());
        let mut path_markers = false;
        for parameter in std::mem::take(parameters) {
            let Some(marker) = parameter.get(EXPAND_KEY) else {
                expanded.push(parameter);
                continue;
            };
            let location = marker["in"].as_str().unwrap_or("query").to_string();
            path_markers |= location == "path";
            let target = marker["schema"].as_str().unwrap_or_default();
            let schema = target
                .strip_prefix("#/components/schemas/")
                .and_then(|name| schemas.get(name));
            let new = match schema {
                Some(schema) => from_schema(schema, target, &location, marker.get("name")),
                None => {
                    diagnostics.push(Diagnostic::new(
                        Code::UnresolvedExpansion,
                        format!(
                            "`{}` of {} {} {} is not a documented struct; its {location} parameters are not documented",
                            target.trim_start_matches('$'),
                            at.method,
                            at.section.trim_end_matches('s'),
                            at.name
                        ),
                    ));
                    Vec::new()
                }
            };
            for parameter in new {
                let identity = (parameter["name"].clone(), parameter["in"].clone());
                let duplicate = explicit.contains(&identity)
                    || expanded
                        .iter()
                        .any(|p| p["name"] == identity.0 && p["in"] == identity.1);
                if !duplicate {
                    expanded.push(parameter);
                }
            }
        }
        if path_markers {
            for placeholder in placeholders(at.name) {
                let declared = expanded
                    .iter()
                    .any(|p| p["in"] == "path" && p["name"] == placeholder.as_str());
                if !declared {
                    diagnostics.push(Diagnostic::new(
                        Code::UndeclaredPathParam,
                        format!(
                            "path parameter `{placeholder}` of `{}` has no definition",
                            at.name
                        ),
                    ));
                }
            }
        }
        *parameters = expanded;
    });
    remove_empty_parameters(document);
}

/// One parameter per property of an object schema; a non-object schema is a
/// single parameter called `fallback`.
fn from_schema(
    schema: &Value,
    target: &str,
    location: &str,
    fallback: Option<&Value>,
) -> Vec<Value> {
    let Some(properties) = schema.get("properties").and_then(Value::as_object) else {
        return fallback
            .map(|name| vec![json!({ "name": name, "in": location, "required": true, "schema": { "$ref": target } })])
            .unwrap_or_default();
    };
    let required: Vec<&Value> = schema
        .get("required")
        .and_then(Value::as_array)
        .map(|r| r.iter().collect())
        .unwrap_or_default();
    properties
        .iter()
        .map(|(name, property)| {
            let mut property = property.clone();
            let description = match &mut property {
                Value::Object(map) => map.remove("description"),
                _ => None,
            };
            let mut parameter = Map::new();
            parameter.insert("name".into(), json!(name));
            parameter.insert("in".into(), json!(location));
            parameter.insert(
                "required".into(),
                json!(location == "path" || required.contains(&&json!(name))),
            );
            if let Some(description) = description {
                parameter.insert("description".into(), description);
            }
            parameter.insert("schema".into(), strip_null(property));
            Value::Object(parameter)
        })
        .collect()
}

fn placeholders(path: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut rest = path;
    while let Some(open) = rest.find('{') {
        let after = &rest[open + 1..];
        let Some(close) = after.find('}') else { break };
        out.push(after[..close].to_string());
        rest = &after[close + 1..];
    }
    out
}

fn remove_empty_parameters(document: &mut Value) {
    for_each_operation(document, |_, operation| {
        if operation
            .get("parameters")
            .and_then(Value::as_array)
            .is_some_and(Vec::is_empty)
        {
            operation.remove("parameters");
        }
    });
}
