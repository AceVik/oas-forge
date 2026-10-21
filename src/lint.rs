//! Checks of the merged document that the official schemas cannot express.
//!
//! Always on: dangling component references, duplicate operationIds,
//! undefined security schemes and path parameters that do not match their
//! path. With strict lints also: unused components and missing descriptions.
//!
//! Findings point to the snippet that defined the part they concern (an
//! operation, a component, a top-level field), see [`Origins`].

use super::diagnostics::{Code, Diagnostic, Diagnostics, Location};
use super::document::{OperationRef, for_each_operation_ref};
use serde_json::{Map, Value};
use std::collections::{BTreeMap, BTreeSet};

/// Where the parts of a merged document were defined: for each path item,
/// operation (also in `additionalOperations`), webhook, AsyncAPI channel and
/// operation, component and other top-level field, by JSON pointer, the
/// location of the first snippet that defined it.
#[derive(Debug, Default)]
pub(crate) struct Origins {
    /// Locations by JSON pointer (`/paths/~1users/get`).
    by_pointer: BTreeMap<String, Location>,
}

impl Origins {
    /// Records the parts `snippet` defines at `location`, unless an earlier
    /// snippet defined them.
    pub(crate) fn record(&mut self, snippet: &Value, location: &Location) {
        let Some(map) = snippet.as_object() else {
            return;
        };
        for (key, value) in map {
            let top = format!("/{}", escape(key));
            match key.as_str() {
                "paths" | "webhooks" => {
                    for (name, item) in entries(value) {
                        let item_pointer = format!("{top}/{}", escape(name));
                        self.add(&item_pointer, location);
                        for (field, operation) in entries(item) {
                            let field_pointer = format!("{item_pointer}/{}", escape(field));
                            self.add(&field_pointer, location);
                            if field == "additionalOperations" {
                                for (method, _) in entries(operation) {
                                    self.add(
                                        &format!("{field_pointer}/{}", escape(method)),
                                        location,
                                    );
                                }
                            }
                        }
                    }
                }
                "components" => {
                    for (section, components) in entries(value) {
                        for (name, _) in entries(components) {
                            let pointer = format!("{top}/{}/{}", escape(section), escape(name));
                            self.add(&pointer, location);
                        }
                    }
                }
                "channels" | "operations" => {
                    for (name, _) in entries(value) {
                        self.add(&format!("{top}/{}", escape(name)), location);
                    }
                }
                _ => self.add(&top, location),
            }
        }
    }

    /// Records `pointer` at `location` unless it is known.
    fn add(&mut self, pointer: &str, location: &Location) {
        if !self.by_pointer.contains_key(pointer) {
            self.by_pointer
                .insert(pointer.to_string(), location.clone());
        }
    }

    /// Where the part at `pointer` was defined.
    fn get(&self, pointer: &str) -> Option<&Location> {
        self.by_pointer.get(pointer)
    }

    /// Where an operation (a fixed field of its path item or one of its
    /// `additionalOperations`) was defined.
    fn operation(&self, at: &OperationRef<'_>) -> Option<&Location> {
        let item = format!("/{}/{}", at.section, escape(at.name));
        let method = escape(at.method);
        self.get(&format!("{item}/{method}"))
            .or_else(|| self.get(&format!("{item}/additionalOperations/{method}")))
    }
}

/// The entries of an object; nothing for other values.
fn entries(value: &Value) -> impl Iterator<Item = (&String, &Value)> {
    value.as_object().into_iter().flatten()
}

/// Escapes a JSON pointer segment: `~` is `~0`, `/` is `~1` (RFC 6901).
fn escape(segment: &str) -> String {
    segment.replace('~', "~0").replace('/', "~1")
}

/// Which lints run.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(crate) struct Lints {
    /// Unused components and missing descriptions.
    pub(crate) strict: bool,
}

/// Runs the lints on an OpenAPI or AsyncAPI document whose parts were
/// defined where `origins` says.
pub(crate) fn check(
    document: &Value,
    lints: Lints,
    origins: &Origins,
    diagnostics: &mut Diagnostics,
) {
    let mut refs = References::default();
    collect_refs(document, &Position::root(origins), &mut refs);
    for (target, location) in &refs.list {
        let Some(pointer) = target.strip_prefix('#') else {
            continue;
        };
        if pointer.starts_with("/components/") && document.pointer(&unescape(pointer)).is_none() {
            diagnostics.push(
                Diagnostic::new(
                    Code::DanglingReference,
                    format!("`$ref: {target}` points to a component that does not exist"),
                )
                .at_opt(*location),
            );
        }
    }

    // Per operationId: how many operations use it, and where the second one
    // (the first duplicate) is defined.
    let mut ids = BTreeMap::<String, (usize, Option<&Location>)>::new();
    let mut requirements: Vec<(Value, Option<&Location>)> = Vec::new();
    if let Some(Value::Array(security)) = document.get("security") {
        let location = origins.get("/security");
        requirements.extend(security.iter().map(|r| (r.clone(), location)));
    }
    let mut undescribed = Vec::new();
    for_each_operation_ref(document, |at, operation| {
        let location = origins.operation(&at);
        if let Some(id) = operation.get("operationId").and_then(Value::as_str) {
            let (count, duplicate) = ids.entry(id.to_string()).or_default();
            *count += 1;
            if *count == 2 {
                *duplicate = location;
            }
        }
        if let Some(Value::Array(security)) = operation.get("security") {
            requirements.extend(security.iter().map(|r| (r.clone(), location)));
        }
        if lints.strict
            && !operation.contains_key("summary")
            && !operation.contains_key("description")
        {
            undescribed.push((format!("operation {} {}", at.method, at.name), location));
        }
    });
    for (id, (count, location)) in ids.into_iter().filter(|(_, (n, _))| *n > 1) {
        diagnostics.push(
            Diagnostic::new(
                Code::DuplicateOperationId,
                format!("operationId `{id}` is used by {count} operations"),
            )
            .at_opt(location),
        );
    }
    security_schemes(document, &requirements, diagnostics);
    path_parameters(document, origins, diagnostics);
    if lints.strict {
        unused_components(document, origins, diagnostics);
        if let Some(Value::Object(schemas)) = document.pointer("/components/schemas") {
            for (name, schema) in schemas {
                if schema.get("description").is_none() {
                    let pointer = format!("/components/schemas/{}", escape(name));
                    undescribed.push((format!("schema `{name}`"), origins.get(&pointer)));
                }
            }
        }
        for (what, location) in undescribed {
            diagnostics.push(
                Diagnostic::new(
                    Code::MissingDescription,
                    format!("{what} has no description"),
                )
                .at_opt(location),
            );
        }
    }
}

/// Security requirements must name schemes of `components.securitySchemes`;
/// each undefined scheme is reported once, where it is first required.
fn security_schemes(
    document: &Value,
    requirements: &[(Value, Option<&Location>)],
    diagnostics: &mut Diagnostics,
) {
    let defined: BTreeSet<&String> = document
        .pointer("/components/securitySchemes")
        .and_then(Value::as_object)
        .map(|schemes| schemes.keys().collect())
        .unwrap_or_default();
    let mut reported = BTreeSet::new();
    for (requirement, location) in requirements {
        let Some(requirement) = requirement.as_object() else {
            continue;
        };
        for name in requirement.keys() {
            if !defined.contains(name) && reported.insert(name.clone()) {
                diagnostics.push(Diagnostic::new(
                    Code::UndefinedSecurityScheme,
                    format!("security scheme `{name}` is required but not defined in `components.securitySchemes`"),
                ).at_opt(*location));
            }
        }
    }
}

/// Every `{placeholder}` of a path needs a path parameter and every path
/// parameter a placeholder.
fn path_parameters(document: &Value, origins: &Origins, diagnostics: &mut Diagnostics) {
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
        let mut operations: Vec<(&str, &Map<String, Value>)> = crate::dsl::METHODS
            .iter()
            .filter_map(|method| Some((*method, item.get(*method)?.as_object()?)))
            .collect();
        if let Some(Value::Object(additional)) = item.get("additionalOperations") {
            operations.extend(
                additional
                    .iter()
                    .filter_map(|(method, o)| Some((method.as_str(), o.as_object()?))),
            );
        }
        for (method, operation) in operations {
            let location = origins.operation(&OperationRef {
                section: "paths",
                name: path,
                method,
            });
            let mut declared = shared.clone();
            declared.extend(path_parameter_names(
                operation.get("parameters"),
                components,
            ));
            let missing: Vec<&String> = placeholders.difference(&declared).collect();
            let extra: Vec<&String> = declared.difference(&placeholders).collect();
            for name in missing {
                diagnostics.push(
                    Diagnostic::new(
                        Code::PathParameterMismatch,
                        format!("`{{{name}}}` of `{path}` has no path parameter"),
                    )
                    .at_opt(location),
                );
            }
            for name in extra {
                diagnostics.push(
                    Diagnostic::new(
                        Code::PathParameterMismatch,
                        format!("path parameter `{name}` does not appear in `{path}`"),
                    )
                    .at_opt(location),
                );
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
fn unused_components(document: &Value, origins: &Origins, diagnostics: &mut Diagnostics) {
    let Some(Value::Object(components)) = document.get("components") else {
        return;
    };
    let used = reachable_components(document);
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
                diagnostics.push(
                    Diagnostic::new(
                        Code::UnusedComponent,
                        format!("`{target}` is never referenced"),
                    )
                    .at_opt(origins.get(&format!("/components/{}/{escaped}", escape(section)))),
                );
            }
        }
    }
}

/// The components that references from outside `components` reach,
/// directly or through other components, as `(section, name)` with the
/// name as a JSON pointer segment (`~1` for `/`).
pub(crate) fn reachable_components(document: &Value) -> BTreeSet<(String, String)> {
    let Some(root) = document.as_object() else {
        return BTreeSet::new();
    };
    // Reachability needs no locations.
    let nowhere = Origins::default();
    let position = Position::root(&nowhere);
    let mut found = References::default();
    for (key, value) in root {
        if key != "components" {
            collect_entry(key, value, &position, &mut found);
        }
    }
    let mut pending: Vec<String> = found.list.into_iter().map(|(target, _)| target).collect();
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
            collect_refs(value, &position, &mut inner);
            pending.extend(inner.list.into_iter().map(|(target, _)| target));
        }
    }
    used
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

/// Reference targets in the order they are found, each once, with the
/// origin of the part where each was first found.
#[derive(Default)]
struct References<'a> {
    /// The targets in the order they were first found.
    list: Vec<(String, Option<&'a Location>)>,
    /// The targets of `list`, for the membership test.
    seen: BTreeSet<String>,
}

impl<'a> References<'a> {
    /// Appends `target`, found at `position`, unless it was added before.
    fn add(&mut self, target: &str, position: &Position<'a>) {
        if !self.seen.contains(target) {
            self.seen.insert(target.to_string());
            self.list.push((target.to_string(), position.location));
        }
    }
}

/// Where a walk for references is: the JSON pointer of the value and the
/// origin of the innermost recorded part around it.
struct Position<'a> {
    /// The origins of the document.
    origins: &'a Origins,
    /// The JSON pointer of the value.
    pointer: String,
    /// Where the innermost recorded part around the value was defined.
    location: Option<&'a Location>,
}

impl<'a> Position<'a> {
    /// The top of a document.
    fn root(origins: &'a Origins) -> Self {
        Self {
            origins,
            pointer: String::new(),
            location: None,
        }
    }

    /// The position of the member `segment` (a key or an index).
    fn child(&self, segment: &str) -> Self {
        let pointer = format!("{}/{}", self.pointer, escape(segment));
        let location = self.origins.get(&pointer).or(self.location);
        Self {
            origins: self.origins,
            pointer,
            location,
        }
    }
}

/// The `$ref` targets and discriminator mappings below `value`.
fn collect_refs<'a>(value: &Value, position: &Position<'a>, out: &mut References<'a>) {
    match value {
        Value::Object(map) => {
            for (key, child) in map {
                collect_entry(key, child, position, out);
            }
        }
        Value::Array(items) => {
            for (index, item) in items.iter().enumerate() {
                collect_refs(item, &position.child(&index.to_string()), out);
            }
        }
        _ => {}
    }
}

/// The references of the entry `key: child` of an object whose keys are
/// keywords (a schema, an operation, a document, …) at `parent`.
fn collect_entry<'a>(key: &str, child: &Value, parent: &Position<'a>, out: &mut References<'a>) {
    if key.starts_with("x-oas-forge-") || DATA_KEYS.contains(&key) {
        return;
    }
    let position = parent.child(key);
    match (key, child) {
        ("$ref", Value::String(target)) => out.add(target, &position),
        ("discriminator", Value::Object(discriminator)) => {
            let Some(Value::Object(mapping)) = discriminator.get("mapping") else {
                return;
            };
            for target in mapping.values().filter_map(Value::as_str) {
                // A mapping value is a reference or a schema name.
                if target.contains(['#', '/', '.']) {
                    out.add(target, &position);
                } else {
                    out.add(&format!("#/components/schemas/{target}"), &position);
                }
            }
        }
        // A JSON Schema `examples` list is data; an OpenAPI `examples` map
        // holds Example Objects or references.
        ("examples", Value::Array(_)) => {}
        (_, Value::Object(entries)) if NAME_MAPS.contains(&key) || key == "examples" => {
            for (name, entry) in entries {
                collect_refs(entry, &position.child(name), out);
            }
        }
        _ => collect_refs(child, &position, out),
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
