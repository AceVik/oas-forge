//! Checks of the merged document that the official schemas cannot express.
//!
//! Always on: dangling component references, duplicate operationIds,
//! undefined security schemes and path parameters that do not match their
//! path. With strict lints also: unused components and missing descriptions.

use super::diagnostics::{Code, Diagnostic, Diagnostics};
use super::document::for_each_operation;
use serde_json::{Map, Value};
use std::collections::{BTreeMap, BTreeSet};

/// Which lints run.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(crate) struct Lints {
    /// Unused components and missing descriptions.
    pub(crate) strict: bool,
}

/// Runs the lints on an OpenAPI or AsyncAPI document.
pub(crate) fn check(document: &Value, lints: Lints, diagnostics: &mut Diagnostics) {
    let mut refs = Vec::new();
    collect_refs(document, &mut refs);
    for target in &refs {
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

    let mut copy = document.clone();
    let mut ids = BTreeMap::<String, usize>::new();
    let mut requirements = Vec::new();
    if let Some(Value::Array(security)) = document.get("security") {
        requirements.extend(security.iter().cloned());
    }
    let mut undescribed = Vec::new();
    for_each_operation(&mut copy, |at, operation| {
        if let Some(id) = operation.get("operationId").and_then(Value::as_str) {
            *ids.entry(id.to_string()).or_default() += 1;
        }
        if let Some(Value::Array(security)) = operation.get("security") {
            requirements.extend(security.iter().cloned());
        }
        if lints.strict
            && !operation.contains_key("summary")
            && !operation.contains_key("description")
        {
            undescribed.push(format!("operation {} {}", at.method, at.name));
        }
    });
    for (id, count) in ids.into_iter().filter(|(_, n)| *n > 1) {
        diagnostics.push(Diagnostic::new(
            Code::DuplicateOperationId,
            format!("operationId `{id}` is used by {count} operations"),
        ));
    }
    security_schemes(document, &requirements, diagnostics);
    path_parameters(document, diagnostics);
    if lints.strict {
        unused_components(document, diagnostics);
        if let Some(Value::Object(schemas)) = document.pointer("/components/schemas") {
            for (name, schema) in schemas {
                if schema.get("description").is_none() {
                    undescribed.push(format!("schema `{name}`"));
                }
            }
        }
        for what in undescribed {
            diagnostics.push(Diagnostic::new(
                Code::MissingDescription,
                format!("{what} has no description"),
            ));
        }
    }
}

/// Security requirements must name schemes of `components.securitySchemes`.
fn security_schemes(document: &Value, requirements: &[Value], diagnostics: &mut Diagnostics) {
    let defined: BTreeSet<&String> = document
        .pointer("/components/securitySchemes")
        .and_then(Value::as_object)
        .map(|schemes| schemes.keys().collect())
        .unwrap_or_default();
    let mut reported = BTreeSet::new();
    for requirement in requirements.iter().filter_map(Value::as_object) {
        for name in requirement.keys() {
            if !defined.contains(name) && reported.insert(name.clone()) {
                diagnostics.push(Diagnostic::new(
                    Code::UndefinedSecurityScheme,
                    format!("security scheme `{name}` is required but not defined in `components.securitySchemes`"),
                ));
            }
        }
    }
}

/// Every `{placeholder}` of a path needs a path parameter and every path
/// parameter a placeholder.
fn path_parameters(document: &Value, diagnostics: &mut Diagnostics) {
    let Some(Value::Object(paths)) = document.get("paths") else {
        return;
    };
    let components = document
        .pointer("/components/parameters")
        .and_then(Value::as_object);
    for (path, item) in paths {
        let Some(item) = item.as_object() else {
            continue;
        };
        let placeholders: BTreeSet<String> = placeholders(path);
        let shared = path_parameter_names(item.get("parameters"), components);
        let mut operations: Vec<&Map<String, Value>> = crate::dsl::METHODS
            .iter()
            .filter_map(|method| item.get(*method)?.as_object())
            .collect();
        if let Some(Value::Object(additional)) = item.get("additionalOperations") {
            operations.extend(additional.values().filter_map(Value::as_object));
        }
        for operation in operations {
            let mut declared = shared.clone();
            declared.extend(path_parameter_names(
                operation.get("parameters"),
                components,
            ));
            let missing: Vec<&String> = placeholders.difference(&declared).collect();
            let extra: Vec<&String> = declared.difference(&placeholders).collect();
            for name in missing {
                diagnostics.push(Diagnostic::new(
                    Code::PathParameterMismatch,
                    format!("`{{{name}}}` of `{path}` has no path parameter"),
                ));
            }
            for name in extra {
                diagnostics.push(Diagnostic::new(
                    Code::PathParameterMismatch,
                    format!("path parameter `{name}` does not appear in `{path}`"),
                ));
            }
        }
    }
}

fn path_parameter_names(
    parameters: Option<&Value>,
    components: Option<&Map<String, Value>>,
) -> BTreeSet<String> {
    let Some(Value::Array(parameters)) = parameters else {
        return BTreeSet::new();
    };
    parameters
        .iter()
        .filter_map(|parameter| {
            let parameter = match parameter.get("$ref").and_then(Value::as_str) {
                Some(target) => {
                    components?.get(target.strip_prefix("#/components/parameters/")?)?
                }
                None => parameter,
            };
            (parameter.get("in")? == "path")
                .then(|| parameter.get("name")?.as_str().map(str::to_string))?
        })
        .collect()
}

fn placeholders(path: &str) -> BTreeSet<String> {
    let mut out = BTreeSet::new();
    let mut rest = path;
    while let Some((_, after)) = rest.split_once('{') {
        let Some((name, tail)) = after.split_once('}') else {
            break;
        };
        out.insert(name.to_string());
        rest = tail;
    }
    out
}

/// Components that cannot be reached from outside `components`: a schema
/// that only references itself (or a cycle of unused schemas) is unused.
fn unused_components(document: &Value, diagnostics: &mut Diagnostics) {
    let (Some(Value::Object(components)), Some(root)) =
        (document.get("components"), document.as_object())
    else {
        return;
    };
    let mut pending = Vec::new();
    for (key, value) in root {
        if key != "components" {
            collect_refs(value, &mut pending);
        }
    }
    let mut reached = BTreeSet::new();
    while let Some(target) = pending.pop() {
        if !reached.insert(target.clone()) {
            continue;
        }
        if let Some(pointer) = target.strip_prefix('#')
            && let Some(value) = document.pointer(&unescape(pointer))
        {
            collect_refs(value, &mut pending);
        }
    }
    for (section, entries) in components {
        // Security schemes are used by name, not by reference.
        if section == "securitySchemes" || section.starts_with("x-") {
            continue;
        }
        let Some(entries) = entries.as_object() else {
            continue;
        };
        for name in entries.keys() {
            let target = format!(
                "#/components/{section}/{}",
                name.replace('~', "~0").replace('/', "~1")
            );
            let inside = format!("{target}/");
            let used = reached
                .iter()
                .any(|r| *r == target || r.starts_with(&inside));
            if !used {
                diagnostics.push(Diagnostic::new(
                    Code::UnusedComponent,
                    format!("`{target}` is never referenced"),
                ));
            }
        }
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
                    ("mapping", Value::Object(mapping)) => {
                        for target in mapping.values().filter_map(Value::as_str) {
                            // A mapping value is a reference or a schema name.
                            let target = if target.contains(['#', '/', '.']) {
                                target.to_string()
                            } else {
                                format!("#/components/schemas/{target}")
                            };
                            if !out.contains(&target) {
                                out.push(target);
                            }
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
