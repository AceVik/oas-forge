//! The AsyncAPI document: `@asyncapi` blocks, `@channel` blocks and the
//! component schemas their messages use.

use crate::diagnostics::Diagnostics;
use crate::schema::dialect::{self, Dialect};
use serde_json::{Map, Value};
use std::collections::BTreeSet;

/// The `$ref` prefix of a component schema, shared by OpenAPI and AsyncAPI.
const SCHEMA_PREFIX: &str = "#/components/schemas/";

/// Copies every component schema of the OpenAPI document that the AsyncAPI
/// document uses (directly or through other schemas) into its components,
/// unless it defines a schema of that name itself.
pub(crate) fn add_shared_schemas(asyncapi: &mut Map<String, Value>, openapi: &Value) {
    let Some(schemas) = openapi
        .pointer("/components/schemas")
        .and_then(Value::as_object)
    else {
        return;
    };
    let mut wanted = Vec::new();
    asyncapi
        .values()
        .for_each(|value| collect_refs(value, &mut wanted));
    let mut copied = BTreeSet::new();
    while let Some(name) = wanted.pop() {
        if !copied.insert(name.clone()) {
            continue;
        }
        let exists = asyncapi
            .get("components")
            .and_then(|components| components.pointer(&format!("/schemas/{name}")))
            .is_some();
        let Some(schema) = schemas.get(&name) else {
            continue;
        };
        collect_refs(schema, &mut wanted);
        if exists {
            continue;
        }
        let components = asyncapi
            .entry("components")
            .or_insert_with(|| Value::Object(Map::new()));
        if let Value::Object(components) = components
            && let Value::Object(target) = components
                .entry("schemas")
                .or_insert_with(|| Value::Object(Map::new()))
        {
            target.insert(name, schema.clone());
        }
    }
}

/// The names of the component schemas `value` refers to.
fn collect_refs(value: &Value, out: &mut Vec<String>) {
    match value {
        Value::Object(map) => {
            for (key, child) in map {
                match (key.as_str(), child) {
                    ("$ref", Value::String(target)) => {
                        if let Some(name) = target.strip_prefix(SCHEMA_PREFIX) {
                            out.push(name.to_string());
                        }
                    }
                    (_, child) => collect_refs(child, out),
                }
            }
        }
        Value::Array(items) => items.iter().for_each(|i| collect_refs(i, out)),
        _ => {}
    }
}

/// The AsyncAPI document with its schemas in the AsyncAPI dialect.
pub(super) fn export(asyncapi: &Value, diagnostics: &mut Diagnostics) -> Value {
    let mut document = asyncapi.clone();
    dialect::lower(&mut document, Dialect::AsyncApi3, diagnostics);
    document
}
