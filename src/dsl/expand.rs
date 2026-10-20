//! Struct expansion of parameters (`@query-params $Pagination`, inferred
//! `Query<T>` and `Path<T>`), run on the merged document once every
//! component schema is known.

use super::EXPAND_KEY;
use crate::diagnostics::{Code, Diagnostic, Diagnostics, Location};
use crate::document::for_each_operation;
use crate::schema::strip_null;
use serde_json::{Map, Value, json};

/// How deep flattened structs (`#[serde(flatten)]`) are followed.
const MAX_FLATTEN_DEPTH: usize = 8;

/// Replaces every expansion marker with one parameter per property of the
/// referenced schema. Placeholders an expansion leaves undeclared are
/// reported by the lints.
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
            .map(identity)
            .collect();
        let mut expanded = Vec::with_capacity(parameters.len());
        for parameter in std::mem::take(parameters) {
            let Some(marker) = parameter.get(EXPAND_KEY) else {
                expanded.push(parameter);
                continue;
            };
            let location = marker
                .get("in")
                .and_then(Value::as_str)
                .unwrap_or("query")
                .to_string();
            let target = marker
                .get("schema")
                .and_then(Value::as_str)
                .unwrap_or_default();
            let schema = target
                .strip_prefix("#/components/schemas/")
                .and_then(|name| schemas.get(name));
            let new = match schema {
                Some(schema) => {
                    from_schema(schema, &schemas, target, &location, marker.get("name"))
                }
                None => {
                    let mut diagnostic = Diagnostic::new(
                        Code::UnresolvedExpansion,
                        format!(
                            "`{}` of {} {} {} is not a documented struct; its {location} parameters are not documented",
                            target.trim_start_matches('$'),
                            at.method,
                            at.section.trim_end_matches('s'),
                            at.name
                        ),
                    );
                    if let (Some(file), Some(line)) = (
                        marker.get("file").and_then(Value::as_str),
                        marker.get("line").and_then(Value::as_u64),
                    ) {
                        let line = usize::try_from(line).unwrap_or_default();
                        diagnostic = diagnostic.at(Location::new(file, line));
                    }
                    diagnostics.push(diagnostic);
                    Vec::new()
                }
            };
            for parameter in new {
                let id = identity(&parameter);
                let duplicate =
                    explicit.contains(&id) || expanded.iter().any(|p| identity(p) == id);
                if !duplicate {
                    expanded.push(parameter);
                }
            }
        }
        *parameters = expanded;
    });
    remove_empty_parameters(document);
}

/// The name and location of a parameter.
fn identity(parameter: &Value) -> (Value, Value) {
    (
        parameter.get("name").cloned().unwrap_or_default(),
        parameter.get("in").cloned().unwrap_or_default(),
    )
}

/// The properties of an object schema with the names it requires, including
/// those of flattened structs (`allOf`; an optional one is
/// `anyOf: [{$ref}, {}]`).
#[derive(Default)]
struct Fields {
    properties: Map<String, Value>,
    required: Vec<String>,
}

impl Fields {
    fn collect(&mut self, schema: &Value, schemas: &Value, optional: bool, depth: usize) {
        if depth > MAX_FLATTEN_DEPTH {
            return;
        }
        if let Some(target) = schema
            .get("$ref")
            .and_then(Value::as_str)
            .and_then(|r| r.strip_prefix("#/components/schemas/"))
            .and_then(|name| schemas.get(name))
        {
            self.collect(target, schemas, optional, depth + 1);
            return;
        }
        if let Some(properties) = schema.get("properties").and_then(Value::as_object) {
            for (name, property) in properties {
                if !self.properties.contains_key(name) {
                    self.properties.insert(name.clone(), property.clone());
                }
            }
        }
        if !optional {
            let required = schema.get("required").and_then(Value::as_array);
            for name in required.into_iter().flatten().filter_map(Value::as_str) {
                if !self.required.iter().any(|r| r == name) {
                    self.required.push(name.to_string());
                }
            }
        }
        for part in schema
            .get("allOf")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
        {
            match part.get("anyOf").and_then(Value::as_array) {
                // `#[serde(flatten)] field: Option<T>`: every field optional.
                Some(alternatives) if alternatives.iter().any(is_empty_or_null) => {
                    for alternative in alternatives.iter().filter(|a| !is_empty_or_null(a)) {
                        self.collect(alternative, schemas, true, depth + 1);
                    }
                }
                _ => self.collect(part, schemas, optional, depth + 1),
            }
        }
    }
}

/// `{}` or `{type: null}`: the alternative of an optional value.
fn is_empty_or_null(schema: &Value) -> bool {
    schema.as_object().is_some_and(Map::is_empty)
        || schema.get("type").and_then(Value::as_str) == Some("null")
}

/// One parameter per property of an object schema (flattened structs
/// included); a non-object schema is a single parameter called `fallback`.
fn from_schema(
    schema: &Value,
    schemas: &Value,
    target: &str,
    location: &str,
    fallback: Option<&Value>,
) -> Vec<Value> {
    let mut fields = Fields::default();
    fields.collect(schema, schemas, false, 0);
    if fields.properties.is_empty() {
        return fallback
            .map(|name| vec![json!({ "name": name, "in": location, "required": true, "schema": { "$ref": target } })])
            .unwrap_or_default();
    }
    let Fields {
        properties,
        required,
    } = fields;
    properties
        .into_iter()
        .map(|(name, mut property)| {
            let description = match &mut property {
                Value::Object(map) => map.remove("description"),
                _ => None,
            };
            let required = location == "path" || required.contains(&name);
            let mut parameter = Map::new();
            parameter.insert("name".into(), json!(name));
            parameter.insert("in".into(), json!(location));
            parameter.insert("required".into(), json!(required));
            if let Some(description) = description {
                parameter.insert("description".into(), description);
            }
            parameter.insert("schema".into(), strip_null(property));
            Value::Object(parameter)
        })
        .collect()
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
