//! Struct expansion of parameters (`@query-params $Pagination`, inferred
//! `Query<T>` and `Path<T>`), run on the merged document once every
//! component schema is known.

use super::EXPAND_KEY;
use crate::diagnostics::{Code, Diagnostic, Diagnostics, Location};
use crate::document::for_each_operation;
use crate::schema::strip_null;
use serde_json::{Map, Value, json};
use std::collections::{BTreeMap, BTreeSet};

/// How deep flattened structs (`#[serde(flatten)]`) are followed.
const MAX_FLATTEN_DEPTH: usize = 8;

/// The total size of the parameters that struct expansion adds to one
/// document: each costs [`PARAMETER_COST`] and its JSON text. Every
/// operation that expands a struct copies its fields.
pub(crate) const MAX_EXPANDED_SIZE: usize = 16 * 1024 * 1024;

/// What an expanded parameter costs of [`MAX_EXPANDED_SIZE`] besides its
/// JSON text: as a value it takes far more memory than as text.
const PARAMETER_COST: usize = 256;

/// How many schemas, properties and required names the struct expansions
/// of one document may visit in all: a struct that many other structs
/// flatten is walked again for each of them.
pub(crate) const MAX_EXPANSION_STEPS: usize = 1 << 22;

/// Replaces every expansion marker with one parameter per property of the
/// referenced schema. Placeholders an expansion leaves undeclared are
/// reported by the lints.
pub(crate) fn expand_parameters(document: &mut Value, diagnostics: &mut Diagnostics) {
    let schemas = document
        .pointer("/components/schemas")
        .cloned()
        .unwrap_or_else(|| json!({}));
    let mut left = MAX_EXPANDED_SIZE;
    let mut steps = MAX_EXPANSION_STEPS;
    // The number of fields of each struct expanded so far (by reference
    // and whether optional): once the budget is used up, a struct that
    // cannot fit is reported without walking it again. 2000 operations
    // expanding 20 000 fields each walked them all, for ten seconds.
    let mut counts: BTreeMap<(String, bool), usize> = BTreeMap::new();
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
                    let expansion = Expansion {
                        target,
                        location: &location,
                        fallback,
                    };
                    let key = (target.to_string(), optional);
                    let fits = |fields: usize| {
                        parameter_count(fields, fallback.is_some()).saturating_mul(PARAMETER_COST)
                            <= left
                    };
                    let expanded = match counts.get(&key) {
                        Some(fields) if !fits(*fields) => None,
                        _ => Fields::walk(schema, &schemas, optional, &mut steps).and_then(
                            |fields| {
                                counts.insert(key, fields.properties.len());
                                if fits(fields.properties.len()) {
                                    from_fields(fields, &expansion, left)
                                } else {
                                    None
                                }
                            },
                        ),
                    };
                    match expanded {
                        Some((new, size)) => {
                            left = left.saturating_sub(size);
                            new
                        }
                        None => {
                            left = 0;
                            diagnostics.push(place(Diagnostic::new(
                                Code::ExpansionLimit,
                                format!(
                                    "the {location} parameters of `{struct_name}` for {} {} {} exceed what is left of the {MAX_EXPANDED_SIZE} bytes (or {MAX_EXPANSION_STEPS} visited schemas and fields) of struct expansion in a document; they are left out",
                                    at.method,
                                    at.section.trim_end_matches('s'),
                                    at.name
                                ),
                            )));
                            Vec::new()
                        }
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
/// `anyOf: [{$ref}, {}]`). They are borrowed from the schemas: a struct is
/// copied once its parameters are known to fit the budget.
struct Fields<'a> {
    /// The properties in order; the first schema that has a name wins.
    properties: Vec<(&'a str, &'a Value)>,
    /// The names in `properties`.
    names: BTreeSet<&'a str>,
    /// The names of the required properties (none of optional schemas).
    required: BTreeSet<&'a str>,
    /// The referenced schemas followed so far, and whether as optional:
    /// following one again adds nothing, and a schema whose `allOf` refers
    /// to itself many times made the walk exponential.
    followed: BTreeSet<(&'a str, bool)>,
    /// What is left of [`MAX_EXPANSION_STEPS`] for the document.
    steps: usize,
    /// Whether the steps ran out before the walk was complete.
    exhausted: bool,
}

impl<'a> Fields<'a> {
    /// The fields of `schema` (all optional with `optional` set), taking
    /// the schemas, properties and required names visited from `steps`;
    /// `None` when they run out.
    fn walk(
        schema: &'a Value,
        schemas: &'a Value,
        optional: bool,
        steps: &mut usize,
    ) -> Option<Self> {
        let mut fields = Self {
            properties: Vec::new(),
            names: BTreeSet::new(),
            required: BTreeSet::new(),
            followed: BTreeSet::new(),
            steps: *steps,
            exhausted: false,
        };
        fields.collect(schema, schemas, optional, 0);
        *steps = fields.steps;
        (!fields.exhausted).then_some(fields)
    }

    /// Takes `count` steps; `false` (and the walk is exhausted) when fewer
    /// are left.
    fn spend(&mut self, count: usize) -> bool {
        match self.steps.checked_sub(count) {
            Some(rest) => {
                self.steps = rest;
                true
            }
            None => {
                self.steps = 0;
                self.exhausted = true;
                false
            }
        }
    }

    /// Adds the properties of `schema`, following references into
    /// `schemas` (`components.schemas`) and flattened `allOf` parts up to
    /// [`MAX_FLATTEN_DEPTH`]. With `optional` set, the properties it adds
    /// are not required.
    fn collect(&mut self, schema: &'a Value, schemas: &'a Value, optional: bool, depth: usize) {
        if depth > MAX_FLATTEN_DEPTH || !self.spend(1) {
            return;
        }
        if let Some((name, target)) = schema
            .get("$ref")
            .and_then(Value::as_str)
            .and_then(|r| r.strip_prefix("#/components/schemas/"))
            .and_then(|name| Some((name, schemas.get(name)?)))
        {
            if self.followed.insert((name, optional)) {
                self.collect(target, schemas, optional, depth + 1);
            }
            return;
        }
        if let Some(properties) = schema.get("properties").and_then(Value::as_object) {
            if !self.spend(properties.len()) {
                return;
            }
            for (name, property) in properties {
                if self.names.insert(name.as_str()) {
                    self.properties.push((name.as_str(), property));
                }
            }
        }
        if !optional {
            let required = schema.get("required").and_then(Value::as_array);
            if !self.spend(required.map_or(0, Vec::len)) {
                return;
            }
            for name in required.into_iter().flatten().filter_map(Value::as_str) {
                self.required.insert(name);
            }
        }
        for part in schema
            .get("allOf")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
        {
            if self.exhausted {
                return;
            }
            match part.get("anyOf").and_then(Value::as_array) {
                // `#[serde(flatten)] field: Option<T>`: every field optional.
                Some(alternatives)
                    if self.spend(alternatives.len())
                        && alternatives.iter().any(is_empty_or_null) =>
                {
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

/// What an expansion marker asks for.
struct Expansion<'m> {
    /// The reference of the struct's schema (`#/components/schemas/Name`).
    target: &'m str,
    /// Where the parameters go (`query`, `path`, …).
    location: &'m str,
    /// The name of the single parameter for a non-object schema.
    fallback: Option<&'m Value>,
}

/// The number of parameters a struct with `fields` fields expands to: one
/// per field, or the fallback parameter (if there is one) without fields.
fn parameter_count(fields: usize, fallback: bool) -> usize {
    if fields == 0 {
        usize::from(fallback)
    } else {
        fields
    }
}

/// One parameter per field (flattened structs included, all optional if
/// the `expansion` asks), or a single parameter called its fallback for a
/// non-object schema, with what they cost of the budget; `None` when they
/// do not fit in what is `left` of it. The caller counts the parameters
/// before they are built (each costs at least [`PARAMETER_COST`]), so a
/// struct that cannot fit is not copied: once the budget was used up, 200
/// operations expanding 20 000 fields still copied and serialized them
/// all, for a minute.
fn from_fields(
    fields: Fields<'_>,
    expansion: &Expansion<'_>,
    left: usize,
) -> Option<(Vec<Value>, usize)> {
    let location = expansion.location;
    let parameters: Vec<Value> = if fields.properties.is_empty() {
        expansion
            .fallback
            .map(|name| vec![json!({ "name": name, "in": location, "required": true, "schema": { "$ref": expansion.target } })])
            .unwrap_or_default()
    } else {
        let Fields {
            properties,
            required,
            ..
        } = fields;
        properties
            .into_iter()
            .map(|(name, property)| {
                let required = location == "path" || required.contains(name);
                let mut property = property.clone();
                let mut parameter = Map::new();
                parameter.insert("name".into(), json!(name));
                parameter.insert("in".into(), json!(location));
                parameter.insert("required".into(), json!(required));
                lift_annotations(&mut property, &mut parameter);
                parameter.insert("schema".into(), strip_null(property));
                Value::Object(parameter)
            })
            .collect()
    };
    let size = parameters
        .iter()
        .map(|parameter| PARAMETER_COST + parameter.to_string().len())
        .fold(0, usize::saturating_add);
    (size <= left).then_some((parameters, size))
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
