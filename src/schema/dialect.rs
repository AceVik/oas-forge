//! Schema dialects of the supported targets (ADR 0002).
//!
//! The pipeline produces JSON Schema 2020-12 as used by OpenAPI 3.1 and 3.2.
//! [`lower`] rewrites the schemas of a document for an older target, e.g.
//! `type: [string, "null"]` to `type: string` plus `nullable: true` for
//! OpenAPI 3.0.

use crate::diagnostics::{Code, Diagnostic, Diagnostics};
use serde_json::{Map, Value, json};
use std::collections::BTreeSet;

/// A schema dialect.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[non_exhaustive]
pub enum Dialect {
    /// OpenAPI 3.0: an extended subset of JSON Schema draft 5 with `nullable`.
    OpenApi30,
    /// OpenAPI 3.1: JSON Schema 2020-12.
    OpenApi31,
    /// OpenAPI 3.2: JSON Schema 2020-12.
    OpenApi32,
}

impl Dialect {
    /// The dialect of a document with an `openapi` version field.
    pub fn of_document(document: &Value) -> Option<Self> {
        let version = document.get("openapi")?.as_str()?;
        Some(if version.starts_with("3.0") {
            Self::OpenApi30
        } else if version.starts_with("3.1") {
            Self::OpenApi31
        } else {
            Self::OpenApi32
        })
    }
}

/// Keywords whose value is a schema.
const SCHEMA_KEYWORDS: &[&str] = &[
    "items",
    "not",
    "additionalProperties",
    "contains",
    "propertyNames",
    "if",
    "then",
    "else",
    "unevaluatedItems",
    "unevaluatedProperties",
    "contentSchema",
];
/// Keywords whose value is an array of schemas.
const SCHEMA_ARRAYS: &[&str] = &["allOf", "anyOf", "oneOf", "prefixItems"];
/// Keywords whose value is a map of schemas.
const SCHEMA_MAPS: &[&str] = &[
    "properties",
    "patternProperties",
    "$defs",
    "definitions",
    "dependentSchemas",
];
/// Keys whose values are data, never schemas or document structure.
const DATA_KEYS: &[&str] = &["example", "examples", "default", "enum", "const", "value"];

/// Rewrites an OpenAPI `document` for `dialect`.
///
/// Operations and fields that need a newer version (OpenAPI 3.2: `query`,
/// `additionalOperations`, `querystring` parameters, `itemSchema`; 3.1:
/// `webhooks`) are removed and reported as [`Code::UnsupportedFeature`].
/// For OpenAPI 3.0 every schema is rewritten; constructs without an
/// equivalent are approximated and reported once per kind as
/// [`Code::LossyLowering`].
pub fn lower(document: &mut Value, dialect: Dialect, diagnostics: &mut Diagnostics) {
    if dialect < Dialect::OpenApi32 {
        restrict(document, dialect, diagnostics);
    }
    if dialect != Dialect::OpenApi30 {
        return;
    }
    let mut lost = BTreeSet::new();
    walk_document(document, &mut |schema| lower_30(schema, &mut lost));
    for what in lost {
        diagnostics.push(Diagnostic::new(
            Code::LossyLowering,
            format!("OpenAPI 3.0 has no equivalent for {what}; the schema was approximated"),
        ));
    }
}

/// Removes what `dialect` cannot express, reporting each kind once.
fn restrict(document: &mut Value, dialect: Dialect, diagnostics: &mut Diagnostics) {
    let mut removed = BTreeSet::new();
    let version = if dialect == Dialect::OpenApi30 {
        "3.0"
    } else {
        "3.1"
    };
    if dialect == Dialect::OpenApi30
        && let Some(map) = document.as_object_mut()
        && map.remove("webhooks").is_some()
    {
        removed.insert(("webhooks", "3.1"));
    }
    for section in ["paths", "webhooks"] {
        let Some(items) = document.get_mut(section).and_then(Value::as_object_mut) else {
            continue;
        };
        for item in items.values_mut().filter_map(Value::as_object_mut) {
            if item.remove("query").is_some() {
                removed.insert(("`QUERY` operations", "3.2"));
            }
            if item.remove("additionalOperations").is_some() {
                removed.insert(("`additionalOperations`", "3.2"));
            }
            for (key, value) in item.iter_mut() {
                if key == "parameters" {
                    remove_querystring(value, &mut removed);
                }
                if let Some(operation) = value.as_object_mut() {
                    if let Some(parameters) = operation.get_mut("parameters") {
                        remove_querystring(parameters, &mut removed);
                    }
                    remove_item_schemas(operation, &mut removed);
                }
            }
        }
    }
    for (what, needed) in removed {
        diagnostics.push(Diagnostic::new(
            Code::UnsupportedFeature,
            format!("{what} need OpenAPI {needed} or newer; the root declares {version}, so they were removed"),
        ));
    }
}

fn remove_querystring(
    parameters: &mut Value,
    removed: &mut BTreeSet<(&'static str, &'static str)>,
) {
    if let Value::Array(list) = parameters {
        let before = list.len();
        list.retain(|p| p.get("in").and_then(Value::as_str) != Some("querystring"));
        if list.len() != before {
            removed.insert(("`querystring` parameters", "3.2"));
        }
    }
}

/// Removes `itemSchema` from the media types of the request body and the
/// responses of an operation.
fn remove_item_schemas(
    operation: &mut Map<String, Value>,
    removed: &mut BTreeSet<(&'static str, &'static str)>,
) {
    let mut strip = |container: &mut Value| {
        let Some(Value::Object(content)) = container.get_mut("content") else {
            return;
        };
        for media in content.values_mut().filter_map(Value::as_object_mut) {
            if media.remove("itemSchema").is_some() {
                removed.insert(("streamed responses (`itemSchema`)", "3.2"));
            }
        }
    };
    if let Some(body) = operation.get_mut("requestBody") {
        strip(body);
    }
    if let Some(Value::Object(responses)) = operation.get_mut("responses") {
        responses.values_mut().for_each(&mut strip);
    }
}

/// Calls `f` for every schema object of an OpenAPI document, outermost first
/// (so `f` sees subschemas before they are rewritten).
fn walk_document(value: &mut Value, f: &mut impl FnMut(&mut Map<String, Value>)) {
    match value {
        Value::Object(map) => {
            for (key, child) in map.iter_mut() {
                match key.as_str() {
                    "schema" => walk_schema(child, f),
                    "schemas" => {
                        if let Value::Object(schemas) = child {
                            schemas.values_mut().for_each(|s| walk_schema(s, f));
                        }
                    }
                    k if DATA_KEYS.contains(&k) => {}
                    _ => walk_document(child, f),
                }
            }
        }
        Value::Array(items) => items.iter_mut().for_each(|i| walk_document(i, f)),
        _ => {}
    }
}

fn walk_schema(value: &mut Value, f: &mut impl FnMut(&mut Map<String, Value>)) {
    let Value::Object(map) = value else {
        return;
    };
    f(map);
    for (key, child) in map.iter_mut() {
        let key = key.as_str();
        if SCHEMA_KEYWORDS.contains(&key) {
            walk_schema(child, f);
        } else if SCHEMA_ARRAYS.contains(&key) {
            if let Value::Array(items) = child {
                items.iter_mut().for_each(|i| walk_schema(i, f));
            }
        } else if SCHEMA_MAPS.contains(&key)
            && let Value::Object(entries) = child
        {
            entries.values_mut().for_each(|s| walk_schema(s, f));
        }
    }
}

fn lower_30(schema: &mut Map<String, Value>, lost: &mut BTreeSet<&'static str>) {
    // `type: [T, "null"]` / `type: "null"`
    match schema.get("type").cloned() {
        Some(Value::Array(types)) => {
            let null = types.iter().any(|t| t == "null");
            let others: Vec<Value> = types.into_iter().filter(|t| *t != "null").collect();
            match others.as_slice() {
                [] => {
                    schema.remove("type");
                    schema.insert("enum".into(), json!([null]));
                }
                [single] => {
                    schema.insert("type".into(), single.clone());
                }
                _ => {
                    schema.remove("type");
                    let alternatives: Vec<Value> =
                        others.iter().map(|t| json!({ "type": t })).collect();
                    schema.insert("anyOf".into(), Value::Array(alternatives));
                }
            }
            if null {
                schema.insert("nullable".into(), json!(true));
            }
        }
        Some(Value::String(t)) if t == "null" => {
            schema.remove("type");
            schema.insert("enum".into(), json!([null]));
            schema.insert("nullable".into(), json!(true));
        }
        _ => {}
    }

    // `anyOf: [S, {type: null}]`
    if let Some(Value::Array(alternatives)) = schema.get_mut("anyOf")
        && let Some(position) = alternatives.iter().position(is_null_schema)
    {
        alternatives.remove(position);
        schema.insert("nullable".into(), json!(true));
        if let Some(Value::Array(alternatives)) = schema.get("anyOf")
            && let [single] = alternatives.as_slice()
        {
            let single = single.clone();
            schema.remove("anyOf");
            match single {
                Value::Object(inner) if inner.contains_key("$ref") => {
                    schema.insert("allOf".into(), json!([inner]));
                }
                Value::Object(inner) => {
                    for (k, v) in inner {
                        schema.entry(k).or_insert(v);
                    }
                }
                other => {
                    schema.insert("allOf".into(), json!([other]));
                }
            }
        }
    }

    if let Some(value) = schema.remove("const") {
        schema.insert("enum".into(), json!([value]));
    }
    for (exclusive, bound) in [
        ("exclusiveMinimum", "minimum"),
        ("exclusiveMaximum", "maximum"),
    ] {
        if let Some(limit) = schema.get(exclusive).filter(|v| v.is_number()).cloned() {
            schema.insert(bound.into(), limit);
            schema.insert(exclusive.into(), json!(true));
        }
    }
    if let Some(Value::Array(items)) = schema.remove("prefixItems") {
        let items: Vec<Value> = items.into_iter().filter(|i| !is_null_schema(i)).collect();
        schema.insert("items".into(), json!({ "anyOf": items }));
        lost.insert("tuple items (`prefixItems`)");
    }
    if schema.get("items") == Some(&Value::Bool(false)) {
        schema.remove("items");
    }
    for keyword in [
        "unevaluatedProperties",
        "unevaluatedItems",
        "propertyNames",
        "dependentSchemas",
    ] {
        if schema.remove(keyword).is_some() {
            lost.insert(match keyword {
                "unevaluatedProperties" => "`unevaluatedProperties`",
                "unevaluatedItems" => "`unevaluatedItems`",
                "propertyNames" => "`propertyNames`",
                _ => "`dependentSchemas`",
            });
        }
    }
    if let Some(Value::Array(examples)) = schema.remove("examples")
        && let Some(first) = examples.into_iter().next()
    {
        schema.entry("example").or_insert(first);
    }
    // 3.0 ignores the siblings of `$ref`.
    if schema.contains_key("$ref") && schema.len() > 1 {
        let reference = schema.remove("$ref").unwrap_or_default();
        let mut all_of = match schema.remove("allOf") {
            Some(Value::Array(items)) => items,
            _ => Vec::new(),
        };
        all_of.insert(0, json!({ "$ref": reference }));
        schema.insert("allOf".into(), Value::Array(all_of));
    }
}

fn is_null_schema(value: &Value) -> bool {
    *value == json!({ "type": "null" })
}
