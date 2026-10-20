//! The specification lowered to OpenAPI 3.0.3.

use crate::diagnostics::{Code, Diagnostic, Diagnostics};
use crate::schema::dialect::{self, Dialect};
use serde_json::{Map, Value, json};

/// The OpenAPI version of the export.
pub(super) const VERSION: &str = "3.0.3";

/// The canonical document as OpenAPI 3.0.3: schemas lowered, and fields 3.0
/// does not know removed (reported once per kind).
pub(super) fn export(canonical: &Value, diagnostics: &mut Diagnostics) -> Value {
    let mut document = canonical.clone();
    let mut removed: Vec<&str> = Vec::new();
    if let Value::Object(map) = &mut document {
        map.insert("openapi".into(), json!(VERSION));
        for (key, what) in [
            ("jsonSchemaDialect", "`jsonSchemaDialect`"),
            ("$self", "`$self`"),
        ] {
            if map.remove(key).is_some() {
                removed.push(what);
            }
        }
        map.entry("paths").or_insert_with(|| json!({}));
        if let Some(Value::Object(info)) = map.get_mut("info") {
            if info.remove("summary").is_some() {
                removed.push("`info.summary`");
            }
            // An identifier that repeats the name loses nothing.
            if let Some(Value::Object(license)) = info.get_mut("license")
                && let Some(identifier) = license.remove("identifier")
                && license.get("name") != Some(&identifier)
            {
                removed.push("`license.identifier`");
            }
        }
        if let Some(Value::Array(tags)) = map.get_mut("tags") {
            for tag in tags.iter_mut().filter_map(Value::as_object_mut) {
                for key in ["summary", "parent", "kind"] {
                    if tag.remove(key).is_some() {
                        removed.push("tag `summary`/`parent`/`kind`");
                    }
                }
            }
        }
        if let Some(Value::Object(components)) = map.get_mut("components") {
            for key in ["pathItems", "mediaTypes"] {
                if components.remove(key).is_some() {
                    removed.push("`components.pathItems`/`mediaTypes`");
                }
            }
            if let Some(Value::Object(schemes)) = components.get_mut("securitySchemes") {
                let before = schemes.len();
                schemes.retain(|_, scheme| scheme["type"] != "mutualTLS");
                if schemes.len() != before {
                    removed.push("`mutualTLS` security schemes");
                }
            }
            if let Some(Value::Object(responses)) = components.get_mut("responses") {
                remove_response_summaries(responses, &mut removed);
            }
        }
        remove_operation_response_summaries(map, &mut removed);
    }
    dialect::lower(&mut document, Dialect::OpenApi30, diagnostics);
    removed.sort_unstable();
    removed.dedup();
    if !removed.is_empty() {
        diagnostics.push(Diagnostic::new(
            Code::UnsupportedFeature,
            format!(
                "OpenAPI 3.0 has no {}; they were left out of the 3.0 export",
                removed.join(", ")
            ),
        ));
    }
    document
}

fn remove_response_summaries(responses: &mut Map<String, Value>, removed: &mut Vec<&str>) {
    for response in responses.values_mut().filter_map(Value::as_object_mut) {
        if response.remove("summary").is_some() {
            removed.push("response `summary`");
        }
    }
}

fn remove_operation_response_summaries(document: &mut Map<String, Value>, removed: &mut Vec<&str>) {
    let mut value = Value::Object(std::mem::take(document));
    crate::document::for_each_operation(&mut value, |_, operation| {
        if let Some(Value::Object(responses)) = operation.get_mut("responses") {
            remove_response_summaries(responses, removed);
        }
    });
    if let Value::Object(map) = value {
        *document = map;
    }
}
