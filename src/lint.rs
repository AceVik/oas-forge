//! Checks of the merged document that the official schemas cannot express.
//!
//! Always on: dangling component references, duplicate operationIds,
//! undefined security schemes and path parameters that do not match their
//! path. With strict lints also: unused components and missing descriptions.

use super::diagnostics::{Code, Diagnostic, Diagnostics};
use super::document::for_each_operation_ref;
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
    let mut refs = References::default();
    collect_refs(document, &mut refs);
    for target in &refs.list {
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

    let mut ids = BTreeMap::<String, usize>::new();
    let mut requirements = Vec::new();
    if let Some(Value::Array(security)) = document.get("security") {
        requirements.extend(security.iter().cloned());
    }
    let mut undescribed = Vec::new();
    for_each_operation_ref(document, |at, operation| {
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

/// The names of the `in: path` parameters of a `parameters` array; a
/// reference is followed into `components.parameters`, and one that does
/// not resolve there is skipped.
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
                Some(target) => components?.get(&decode_segment(
                    target.strip_prefix("#/components/parameters/")?,
                ))?,
                None => parameter,
            };
            (parameter.get("in")? == "path")
                .then(|| parameter.get("name")?.as_str().map(str::to_string))?
        })
        .collect()
}

/// The `{placeholder}` names of a path template; an unclosed `{` ends the
/// scan.
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
    let mut found = References::default();
    for (key, value) in root {
        if key != "components" {
            collect_entry(key, value, &mut found);
        }
    }
    let mut pending = found.list;
    let mut reached = BTreeSet::new();
    // The components a reference points into, as `(section, escaped name)`.
    let mut used = BTreeSet::new();
    while let Some(target) = pending.pop() {
        if !reached.insert(target.clone()) {
            continue;
        }
        let Some(pointer) = target.strip_prefix('#').map(unescape) else {
            continue;
        };
        if let Some((section, rest)) = pointer
            .strip_prefix("/components/")
            .and_then(|rest| rest.split_once('/'))
        {
            let name = rest.split_once('/').map_or(rest, |(name, _)| name);
            used.insert((section.to_string(), name.to_string()));
        }
        if let Some(value) = document.pointer(&pointer) {
            let mut inner = References::default();
            collect_refs(value, &mut inner);
            pending.extend(inner.list);
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
            let escaped = name.replace('~', "~0").replace('/', "~1");
            if !used.contains(&(section.clone(), escaped.clone())) {
                let target = format!("#/components/{section}/{escaped}");
                diagnostics.push(Diagnostic::new(
                    Code::UnusedComponent,
                    format!("`{target}` is never referenced"),
                ));
            }
        }
    }
}

/// Keys whose values are data (examples, defaults, enumerations), never
/// references.
const DATA_KEYS: &[&str] = &["example", "default", "enum", "const", "value"];

/// Keys whose value maps names (property names, status codes, component
/// names, …) to objects; a name may look like a keyword (`default`).
const NAME_MAPS: &[&str] = &[
    "properties",
    "patternProperties",
    "$defs",
    "definitions",
    "dependentSchemas",
    "schemas",
    "responses",
    "parameters",
    "headers",
    "requestBodies",
    "securitySchemes",
    "links",
    "callbacks",
    "pathItems",
    "mediaTypes",
    "content",
    "encoding",
    "paths",
    "webhooks",
    "channels",
    "messages",
    "operations",
    "servers",
    "variables",
];

/// Reference targets in the order they are found, each once.
#[derive(Default)]
struct References {
    /// The targets in the order they were first found.
    list: Vec<String>,
    /// The targets of `list`, for the membership test.
    seen: BTreeSet<String>,
}

impl References {
    /// Appends `target` unless it was added before.
    fn add(&mut self, target: &str) {
        if !self.seen.contains(target) {
            self.seen.insert(target.to_string());
            self.list.push(target.to_string());
        }
    }
}

/// The `$ref` targets and discriminator mappings below `value`.
fn collect_refs(value: &Value, out: &mut References) {
    match value {
        Value::Object(map) => {
            for (key, child) in map {
                collect_entry(key, child, out);
            }
        }
        Value::Array(items) => items.iter().for_each(|i| collect_refs(i, out)),
        _ => {}
    }
}

/// The references of the entry `key: child` of an object whose keys are
/// keywords (a schema, an operation, a document, …).
fn collect_entry(key: &str, child: &Value, out: &mut References) {
    if key.starts_with("x-oas-forge-") || DATA_KEYS.contains(&key) {
        return;
    }
    match (key, child) {
        ("$ref", Value::String(target)) => out.add(target),
        ("discriminator", Value::Object(discriminator)) => {
            let Some(Value::Object(mapping)) = discriminator.get("mapping") else {
                return;
            };
            for target in mapping.values().filter_map(Value::as_str) {
                // A mapping value is a reference or a schema name.
                if target.contains(['#', '/', '.']) {
                    out.add(target);
                } else {
                    out.add(&format!("#/components/schemas/{target}"));
                }
            }
        }
        // A JSON Schema `examples` list is data; an OpenAPI `examples` map
        // holds Example Objects or references.
        ("examples", Value::Array(_)) => {}
        (_, Value::Object(entries)) if NAME_MAPS.contains(&key) || key == "examples" => {
            entries.values().for_each(|entry| collect_refs(entry, out));
        }
        _ => collect_refs(child, out),
    }
}

/// Decodes one JSON pointer segment (`~1` is `/`, `~0` is `~`, RFC 6901).
fn decode_segment(segment: &str) -> String {
    segment.replace("~1", "/").replace("~0", "~")
}

/// Decodes the percent-encoded braces (`%7B`, `%7D`) of a reference pointer.
///
/// JSON pointer segments are written with `~1` for `/` and `~0` for `~`; the
/// pointer API expects them escaped, URIs may contain percent-encoding.
fn unescape(pointer: &str) -> String {
    pointer.replace("%7B", "{").replace("%7D", "}")
}
