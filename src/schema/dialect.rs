//! Schema dialects of the supported targets (ADR 0002).
//!
//! The pipeline produces JSON Schema 2020-12 as used by OpenAPI 3.1 and 3.2.
//! [`lower`] rewrites a document for an older or different target, e.g.
//! `type: [string, "null"]` to `type: string` plus `nullable: true` for
//! OpenAPI 3.0.

use crate::diagnostics::{Code, Diagnostic, Diagnostics};
use serde_json::{Map, Value, json};
use std::collections::BTreeSet;

/// A target dialect.
///
/// # Examples
///
/// ```
/// use oas_forge::schema::dialect::Dialect;
///
/// // Messages name the target by its `Display` form.
/// assert_eq!(Dialect::OpenApi30.to_string(), "OpenAPI 3.0");
/// assert_eq!(Dialect::AsyncApi3.to_string(), "AsyncAPI 3");
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[non_exhaustive]
pub enum Dialect {
    /// OpenAPI 3.0: an extended subset of JSON Schema draft 5 with `nullable`.
    OpenApi30,
    /// OpenAPI 3.1: JSON Schema 2020-12.
    OpenApi31,
    /// OpenAPI 3.2: JSON Schema 2020-12.
    OpenApi32,
    /// Swagger 2.0: a subset of JSON Schema draft 4 without `oneOf`/`anyOf`.
    Swagger20,
    /// AsyncAPI 3: JSON Schema draft 7 with a string `discriminator`.
    AsyncApi3,
}

/// How the target is named in messages (`OpenAPI 3.0`).
impl std::fmt::Display for Dialect {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Self::OpenApi30 => "OpenAPI 3.0",
            Self::OpenApi31 => "OpenAPI 3.1",
            Self::OpenApi32 => "OpenAPI 3.2",
            Self::Swagger20 => "Swagger 2.0",
            Self::AsyncApi3 => "AsyncAPI 3",
        })
    }
}

impl Dialect {
    /// The dialect of a document with an `openapi` version field: `3.0.x`
    /// and `3.1.x` are recognized, any other version counts as 3.2. `None`
    /// without the field (a Swagger 2.0 or AsyncAPI document).
    ///
    /// # Examples
    ///
    /// ```
    /// use oas_forge::schema::dialect::Dialect;
    /// use serde_json::json;
    ///
    /// let document = json!({ "openapi": "3.0.3", "paths": {} });
    /// assert_eq!(Dialect::of_document(&document), Some(Dialect::OpenApi30));
    /// assert_eq!(Dialect::of_document(&json!({ "swagger": "2.0" })), None);
    /// ```
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

    /// Whether the target has OpenAPI 3.1 features (webhooks).
    fn has_webhooks(self) -> bool {
        matches!(self, Self::OpenApi31 | Self::OpenApi32)
    }

    /// Whether the target has OpenAPI 3.2 features (`query`, streams, ...).
    fn has_32_features(self) -> bool {
        self == Self::OpenApi32
    }
}

/// Keywords whose value is a schema.
const SCHEMA_KEYWORDS: &[&str] = &[
    "items",
    "not",
    "additionalProperties",
    "additionalItems",
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
const SCHEMA_ARRAYS: &[&str] = &["allOf", "anyOf", "oneOf", "prefixItems", "items"];
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

/// The keywords of an OpenAPI 3.0 Schema object (plus `$ref`).
const SCHEMA_30: &[&str] = &[
    "$ref",
    "title",
    "multipleOf",
    "maximum",
    "exclusiveMaximum",
    "minimum",
    "exclusiveMinimum",
    "maxLength",
    "minLength",
    "pattern",
    "maxItems",
    "minItems",
    "uniqueItems",
    "maxProperties",
    "minProperties",
    "required",
    "enum",
    "type",
    "not",
    "allOf",
    "oneOf",
    "anyOf",
    "items",
    "properties",
    "additionalProperties",
    "description",
    "format",
    "default",
    "nullable",
    "discriminator",
    "readOnly",
    "writeOnly",
    "example",
    "externalDocs",
    "deprecated",
    "xml",
];

/// The keywords of a Swagger 2.0 Schema object.
const SCHEMA_20: &[&str] = &[
    "$ref",
    "title",
    "multipleOf",
    "maximum",
    "exclusiveMaximum",
    "minimum",
    "exclusiveMinimum",
    "maxLength",
    "minLength",
    "pattern",
    "maxItems",
    "minItems",
    "uniqueItems",
    "maxProperties",
    "minProperties",
    "required",
    "enum",
    "type",
    "allOf",
    "items",
    "properties",
    "additionalProperties",
    "description",
    "format",
    "default",
    "discriminator",
    "readOnly",
    "example",
    "externalDocs",
    "xml",
];

/// Rewrites an OpenAPI `document` for `dialect`.
///
/// Operations and fields that need a newer version (OpenAPI 3.2: `query`,
/// `additionalOperations`, `querystring` parameters, `itemSchema`; 3.1:
/// `webhooks`) are removed and reported as [`Code::UnsupportedFeature`].
/// Schemas are rewritten for the dialect; constructs without an equivalent
/// are approximated and reported once per kind as [`Code::LossyLowering`].
/// The `openapi` version field is left to the caller.
///
/// # Examples
///
/// ```
/// use oas_forge::diagnostics::{Code, Diagnostics};
/// use oas_forge::schema::dialect::{self, Dialect};
/// use serde_json::json;
///
/// let mut document = json!({
///     "openapi": "3.1.0",
///     "webhooks": { "petAdded": {} },
///     "components": {
///         "schemas": { "Name": { "type": ["string", "null"], "examples": ["Rex"] } }
///     }
/// });
/// let mut diagnostics = Diagnostics::new();
/// dialect::lower(&mut document, Dialect::OpenApi30, &mut diagnostics);
///
/// assert_eq!(
///     document,
///     json!({
///         "openapi": "3.1.0",
///         "components": {
///             "schemas": { "Name": { "type": "string", "nullable": true, "example": "Rex" } }
///         }
///     })
/// );
/// let codes: Vec<Code> = diagnostics.iter().map(|d| d.code()).collect();
/// assert_eq!(codes, [Code::UnsupportedFeature]); // the webhooks
/// ```
pub fn lower(document: &mut Value, dialect: Dialect, diagnostics: &mut Diagnostics) {
    if !dialect.has_32_features() && dialect != Dialect::AsyncApi3 {
        restrict(document, dialect, diagnostics);
    }
    if matches!(dialect, Dialect::OpenApi31 | Dialect::OpenApi32) {
        return;
    }
    let mut lost = BTreeSet::new();
    walk_document(document, &mut |schema| {
        lower_schema(schema, dialect, &mut lost)
    });
    report_lost(lost, dialect, diagnostics);
}

/// Rewrites a single schema (and its subschemas) for `dialect`, like
/// [`lower`] does for the schemas of a document.
///
/// # Examples
///
/// ```
/// use oas_forge::diagnostics::{Code, Diagnostics};
/// use oas_forge::schema::dialect::{self, Dialect};
/// use serde_json::json;
///
/// let mut schema = json!({
///     "type": "object",
///     "properties": {
///         "quantity": { "type": "integer", "exclusiveMinimum": 0 },
///         "id": { "oneOf": [{ "type": "string" }, { "type": "integer" }] }
///     }
/// });
/// let mut diagnostics = Diagnostics::new();
/// dialect::lower_schema_value(&mut schema, Dialect::Swagger20, &mut diagnostics);
///
/// assert_eq!(
///     schema,
///     json!({
///         "type": "object",
///         "properties": {
///             "quantity": { "type": "integer", "minimum": 0, "exclusiveMinimum": true },
///             "id": { "x-oneOf": [{ "type": "string" }, { "type": "integer" }] }
///         }
///     })
/// );
/// let codes: Vec<Code> = diagnostics.iter().map(|d| d.code()).collect();
/// assert_eq!(codes, [Code::LossyLowering]); // `oneOf`
/// ```
pub fn lower_schema_value(schema: &mut Value, dialect: Dialect, diagnostics: &mut Diagnostics) {
    if matches!(dialect, Dialect::OpenApi31 | Dialect::OpenApi32) {
        return;
    }
    let mut lost = BTreeSet::new();
    walk_schema(schema, &mut |schema| {
        lower_schema(schema, dialect, &mut lost)
    });
    report_lost(lost, dialect, diagnostics);
}

/// Reports each entry of `lost` once as [`Code::LossyLowering`], in sorted
/// order.
fn report_lost(lost: BTreeSet<String>, dialect: Dialect, diagnostics: &mut Diagnostics) {
    for what in lost {
        diagnostics.push(Diagnostic::new(
            Code::LossyLowering,
            format!(
                "{} has no equivalent for {what}; the schema was approximated",
                dialect
            ),
        ));
    }
}

/// Rewrites one schema object, not its subschemas, for `dialect`: OpenAPI
/// 3.0 and Swagger 2.0 lower step by step and drop the keywords they do not
/// know, AsyncAPI 3 lowers to draft 7. Approximations are added to `lost`.
fn lower_schema(schema: &mut Map<String, Value>, dialect: Dialect, lost: &mut BTreeSet<String>) {
    match dialect {
        Dialect::OpenApi30 => {
            lower_30(schema, lost);
            allow(schema, SCHEMA_30, lost);
            wrap_ref_siblings(schema);
        }
        Dialect::Swagger20 => {
            lower_30(schema, lost);
            lower_20(schema, lost);
            allow(schema, SCHEMA_20, lost);
            wrap_ref_siblings(schema);
        }
        Dialect::AsyncApi3 => lower_draft7(schema, lost),
        Dialect::OpenApi31 | Dialect::OpenApi32 => {}
    }
}

/// Removes what `dialect` cannot express, reporting each kind once.
fn restrict(document: &mut Value, dialect: Dialect, diagnostics: &mut Diagnostics) {
    let mut removed = BTreeSet::new();
    if !dialect.has_webhooks()
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
            format!(
                "{what} need OpenAPI {needed} or newer and were left out of the {} document",
                dialect
            ),
        ));
    }
}

/// Removes the `in: querystring` parameters (OpenAPI 3.2) of a `parameters`
/// array.
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

/// Calls `f` for every schema object of a document, outermost first (so `f`
/// sees subschemas before they are rewritten).
fn walk_document(value: &mut Value, f: &mut impl FnMut(&mut Map<String, Value>)) {
    match value {
        Value::Object(map) => {
            for (key, child) in map.iter_mut() {
                match key.as_str() {
                    // `payload`: AsyncAPI messages.
                    "schema" | "payload" => walk_schema(child, f),
                    "schemas" | "definitions" => {
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

/// Calls `f` for the schema object `value`, then for its subschemas (the
/// keywords of [`SCHEMA_KEYWORDS`], [`SCHEMA_ARRAYS`] and [`SCHEMA_MAPS`]).
/// Subschemas are visited after `f` has rewritten their parent, so the ones
/// `f` creates are walked too.
fn walk_schema(value: &mut Value, f: &mut impl FnMut(&mut Map<String, Value>)) {
    let Value::Object(map) = value else {
        return;
    };
    f(map);
    for (key, child) in map.iter_mut() {
        let key = key.as_str();
        match child {
            Value::Array(items) if SCHEMA_ARRAYS.contains(&key) => {
                items.iter_mut().for_each(|i| walk_schema(i, f));
            }
            Value::Object(entries) if SCHEMA_MAPS.contains(&key) => {
                entries.values_mut().for_each(|s| walk_schema(s, f));
            }
            child if SCHEMA_KEYWORDS.contains(&key) => walk_schema(child, f),
            _ => {}
        }
    }
}

/// OpenAPI 3.0: `nullable`, no `const`, boolean exclusive bounds, no tuples.
fn lower_30(schema: &mut Map<String, Value>, lost: &mut BTreeSet<String>) {
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
        lost.insert("tuple items (`prefixItems`)".into());
    }
    if schema.get("items") == Some(&Value::Bool(false)) {
        schema.remove("items");
    }
    if let Some(Value::Array(examples)) = schema.remove("examples")
        && let Some(first) = examples.into_iter().next()
    {
        schema.entry("example").or_insert(first);
    }
    if schema
        .get("required")
        .and_then(Value::as_array)
        .is_some_and(Vec::is_empty)
    {
        schema.remove("required");
    }
}

/// Swagger 2.0 on top of 3.0: no `nullable`, `oneOf`, `anyOf`, `not`,
/// `writeOnly` or `deprecated` (kept as `x-` extensions), and a string
/// `discriminator`.
fn lower_20(schema: &mut Map<String, Value>, lost: &mut BTreeSet<String>) {
    for (keyword, extension) in [
        ("nullable", "x-nullable"),
        ("writeOnly", "x-writeOnly"),
        ("deprecated", "x-deprecated"),
    ] {
        if let Some(value) = schema.remove(keyword) {
            schema.insert(extension.into(), value);
        }
    }
    for keyword in ["oneOf", "anyOf", "not"] {
        if let Some(value) = schema.remove(keyword) {
            schema.insert(format!("x-{keyword}"), value);
            lost.insert(format!("`{keyword}` (kept as `x-{keyword}`)"));
        }
    }
    discriminator_as_string(schema);
}

/// AsyncAPI 3 schemas are draft 7: tuples are item arrays and the
/// discriminator is a property name.
fn lower_draft7(schema: &mut Map<String, Value>, lost: &mut BTreeSet<String>) {
    if let Some(Value::Array(items)) = schema.remove("prefixItems") {
        if let Some(rest) = schema.remove("items") {
            schema.insert("additionalItems".into(), rest);
        }
        schema.insert("items".into(), Value::Array(items));
    }
    for keyword in [
        "unevaluatedProperties",
        "unevaluatedItems",
        "dependentSchemas",
    ] {
        if schema.remove(keyword).is_some() {
            lost.insert(format!("`{keyword}`"));
        }
    }
    if let Some(definitions) = schema.remove("$defs") {
        schema.insert("definitions".into(), definitions);
    }
    discriminator_as_string(schema);
}

/// Replaces a `discriminator` object by its `propertyName` (`null` without
/// one): Swagger 2.0 and AsyncAPI 3 take only the property name.
fn discriminator_as_string(schema: &mut Map<String, Value>) {
    if let Some(Value::Object(discriminator)) = schema.get("discriminator") {
        let name = discriminator
            .get("propertyName")
            .cloned()
            .unwrap_or_default();
        schema.insert("discriminator".into(), name);
    }
}

/// Removes keywords the target does not know (`x-` extensions stay).
fn allow(schema: &mut Map<String, Value>, allowed: &[&str], lost: &mut BTreeSet<String>) {
    let unknown: Vec<String> = schema
        .keys()
        .filter(|k| !k.starts_with("x-") && !allowed.contains(&k.as_str()))
        .cloned()
        .collect();
    for keyword in unknown {
        schema.remove(&keyword);
        lost.insert(format!("`{keyword}`"));
    }
}

/// Moves a `$ref` that has siblings to the front of `allOf`: OpenAPI 3.0 and
/// Swagger 2.0 ignore the siblings of `$ref`.
fn wrap_ref_siblings(schema: &mut Map<String, Value>) {
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

/// Whether `value` is exactly `{"type": "null"}`.
fn is_null_schema(value: &Value) -> bool {
    *value == json!({ "type": "null" })
}
