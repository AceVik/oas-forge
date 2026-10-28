//! Struct expansion of parameters (`@query-params $Pagination`, inferred
//! `Query<T>` and `Path<T>`), run on the merged document once every
//! component schema is known.

use super::EXPAND_KEY;
use crate::diagnostics::{Code, Diagnostic, Diagnostics, Location};
use crate::document::for_each_operation;
use crate::schema::strip_null;
use serde_json::{Map, Value, json};
use std::collections::BTreeSet;

/// How deep flattened structs (`#[serde(flatten)]`) are followed.
const MAX_FLATTEN_DEPTH: usize = 8;

/// The total size of the parameters that struct expansion adds to one
/// document: each costs [`PARAMETER_COST`] and its JSON text. Every
/// operation that expands a struct copies its fields.
pub(crate) const MAX_EXPANDED_SIZE: usize = 16 * 1024 * 1024;

/// What an expanded parameter costs of [`MAX_EXPANDED_SIZE`] besides its
/// JSON text: as a value it takes far more memory than as text.
const PARAMETER_COST: usize = 256;

/// Replaces every expansion marker with one parameter per property of the
/// referenced schema. Placeholders an expansion leaves undeclared are
/// reported by the lints.
pub(crate) fn expand_parameters(document: &mut Value, diagnostics: &mut Diagnostics) {
    let schemas = document
        .pointer("/components/schemas")
        .cloned()
        .unwrap_or_else(|| json!({}));
    let mut left = MAX_EXPANDED_SIZE;
    for_each_operation(document, |at, operation| {
        let Some(Value::Array(parameters)) = operation.get_mut("parameters") else {
            return;
        };
        if !parameters.iter().any(|p| p.get(EXPAND_KEY).is_some()) {
            return;
        }
        // Explicit parameters win over expanded ones, wherever they are
        // listed; then the first expanded one of an identity.
        let mut seen: BTreeSet<String> = parameters
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
            // The line of the expansion, for findings.
            let place = |diagnostic: Diagnostic| match (
                marker.get("file").and_then(Value::as_str),
                marker.get("line").and_then(Value::as_u64),
            ) {
                (Some(file), Some(line)) => diagnostic.at(Location::new(
                    file,
                    usize::try_from(line).unwrap_or_default(),
                )),
                _ => diagnostic,
            };
            let struct_name = target.trim_start_matches('$');
            let new = match schema {
                Some(schema) => {
                    // `Option<Query<T>>`: every field may be left out.
                    let optional = marker.get("optional") == Some(&Value::Bool(true));
                    let fallback = marker.get("name");
                    let new = from_schema(schema, &schemas, optional, target, &location, fallback);
                    let size = new
                        .iter()
                        .map(|parameter| PARAMETER_COST + parameter.to_string().len())
                        .fold(0, usize::saturating_add);
                    if let Some(rest) = left.checked_sub(size) {
                        left = rest;
                        new
                    } else {
                        left = 0;
                        diagnostics.push(place(Diagnostic::new(
                            Code::ExpansionLimit,
                            format!(
                                "the {location} parameters of `{struct_name}` for {} {} {} exceed what is left of the {MAX_EXPANDED_SIZE} bytes of struct expansion in a document; they are left out",
                                at.method,
                                at.section.trim_end_matches('s'),
                                at.name
                            ),
                        )));
                        Vec::new()
                    }
                }
                None => {
                    diagnostics.push(place(Diagnostic::new(
                        Code::UnresolvedExpansion,
                        format!(
                            "`{struct_name}` of {} {} {} is not a documented struct; its {location} parameters are not documented",
                            at.method,
                            at.section.trim_end_matches('s'),
                            at.name
                        ),
                    )));
                    Vec::new()
                }
            };
            for parameter in new {
                if seen.insert(identity(&parameter)) {
                    expanded.push(parameter);
                }
            }
        }
        *parameters = expanded;
    });
    remove_empty_parameters(document);
}

/// The name and location of a parameter, as JSON texts.
pub(super) fn identity(parameter: &Value) -> String {
    let part = |key: &str| {
        parameter
            .get(key)
            .map_or_else(|| "null".to_string(), Value::to_string)
    };
    format!("{} {}", part("name"), part("in"))
}

/// The properties of an object schema with the names it requires, including
/// those of flattened structs (`allOf`; an optional one is
/// `anyOf: [{$ref}, {}]`).
#[derive(Default)]
struct Fields {
    /// The properties by name; the first schema that has a name wins.
    properties: Map<String, Value>,
    /// The names of the required properties (none of optional schemas).
    required: BTreeSet<String>,
    /// The referenced schemas followed so far, and whether as optional:
    /// following one again adds nothing, and a schema whose `allOf` refers
    /// to itself many times made the walk exponential.
    followed: BTreeSet<(String, bool)>,
}

impl Fields {
    /// Adds the properties of `schema`, following references into
    /// `schemas` (`components.schemas`) and flattened `allOf` parts up to
    /// [`MAX_FLATTEN_DEPTH`]. With `optional` set, the properties it adds
    /// are not required.
    fn collect(&mut self, schema: &Value, schemas: &Value, optional: bool, depth: usize) {
        if depth > MAX_FLATTEN_DEPTH {
            return;
        }
        if let Some((name, target)) = schema
            .get("$ref")
            .and_then(Value::as_str)
            .and_then(|r| r.strip_prefix("#/components/schemas/"))
            .and_then(|name| Some((name, schemas.get(name)?)))
        {
            if self.followed.insert((name.to_string(), optional)) {
                self.collect(target, schemas, optional, depth + 1);
            }
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
                self.required.insert(name.to_string());
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
/// included, all `optional` if asked); a non-object schema is a single
/// parameter called `fallback`.
fn from_schema(
    schema: &Value,
    schemas: &Value,
    optional: bool,
    target: &str,
    location: &str,
    fallback: Option<&Value>,
) -> Vec<Value> {
    let mut fields = Fields::default();
    fields.collect(schema, schemas, optional, 0);
    if fields.properties.is_empty() {
        return fallback
            .map(|name| vec![json!({ "name": name, "in": location, "required": true, "schema": { "$ref": target } })])
            .unwrap_or_default();
    }
    let Fields {
        properties,
        required,
        ..
    } = fields;
    properties
        .into_iter()
        .map(|(name, mut property)| {
            let required = location == "path" || required.contains(&name);
            let mut parameter = Map::new();
            parameter.insert("name".into(), json!(name));
            parameter.insert("in".into(), json!(location));
            parameter.insert("required".into(), json!(required));
            lift_annotations(&mut property, &mut parameter);
            parameter.insert("schema".into(), strip_null(property));
            Value::Object(parameter)
        })
        .collect()
}

/// Moves the keywords of a field's schema that the Parameter Object has
/// itself into `parameter`: `description`, `deprecated` and `example`. The
/// schema's `examples` (a list) become the parameter's `example` when there
/// is one, its `examples` (Example Objects `example1`, `example2`, …) when
/// there are several; next to an `example` they stay in the schema, as
/// `example` and `examples` exclude each other.
fn lift_annotations(property: &mut Value, parameter: &mut Map<String, Value>) {
    let Value::Object(schema) = property else {
        return;
    };
    for key in ["description", "deprecated", "example"] {
        if let Some(value) = schema.remove(key) {
            parameter.insert(key.into(), value);
        }
    }
    if parameter.contains_key("example") {
        return;
    }
    match schema.remove("examples") {
        Some(Value::Array(mut examples)) if examples.len() == 1 => {
            parameter.insert("example".into(), examples.remove(0));
        }
        Some(Value::Array(examples)) => {
            let named: Map<String, Value> = examples
                .into_iter()
                .enumerate()
                .map(|(i, value)| (format!("example{}", i + 1), json!({ "value": value })))
                .collect();
            parameter.insert("examples".into(), Value::Object(named));
        }
        Some(other) => {
            schema.insert("examples".into(), other);
        }
        None => {}
    }
}

/// Removes empty `parameters` lists (an expansion may leave nothing).
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
