//! Checks of the merged document that the official schemas cannot express.

use super::diagnostics::{Code, Diagnostic, Diagnostics};
use serde_json::Value;
use std::collections::BTreeMap;

/// Reports dangling component references and duplicate operationIds.
pub(crate) fn check(document: &Value, diagnostics: &mut Diagnostics) {
    let mut refs = Vec::new();
    collect_refs(document, &mut refs);
    for target in refs {
        let Some(pointer) = target.strip_prefix('#') else {
            continue;
        };
        if pointer.starts_with("/components/") && document.pointer(&unescape(pointer)).is_none() {
            diagnostics.push(Diagnostic::new(
                Code::DanglingReference,
                format!("`$ref: {target}` points to a component that does not exist"),
            ));
        }
    }

    let mut ids = BTreeMap::<&str, usize>::new();
    if let Some(paths) = document.get("paths").and_then(Value::as_object) {
        for item in paths.values().filter_map(Value::as_object) {
            for operation in item.values().filter_map(Value::as_object) {
                if let Some(id) = operation.get("operationId").and_then(Value::as_str) {
                    *ids.entry(id).or_default() += 1;
                }
            }
        }
    }
    for (id, count) in ids.into_iter().filter(|(_, n)| *n > 1) {
        diagnostics.push(Diagnostic::new(
            Code::DuplicateOperationId,
            format!("operationId `{id}` is used by {count} operations"),
        ));
    }
}

fn collect_refs(value: &Value, out: &mut Vec<String>) {
    match value {
        Value::Object(map) => {
            for (key, child) in map {
                if key.starts_with("x-oas-forge-") {
                    continue;
                }
                match (key.as_str(), child) {
                    ("$ref", Value::String(target)) => {
                        if !out.contains(target) {
                            out.push(target.clone());
                        }
                    }
                    _ => collect_refs(child, out),
                }
            }
        }
        Value::Array(items) => items.iter().for_each(|i| collect_refs(i, out)),
        _ => {}
    }
}

/// JSON pointer segments are written with `~1` for `/` and `~0` for `~`; the
/// pointer API expects them escaped, URIs may contain percent-encoding.
fn unescape(pointer: &str) -> String {
    pointer.replace("%7B", "{").replace("%7D", "}")
}
