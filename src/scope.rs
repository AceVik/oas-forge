//! Scopes: several documents from one source set.
//!
//! A `@scope name[, name…]` line puts a route, webhook or channel block, a
//! module's `@route-defaults` or a YAML block (`@openapi`, `@asyncapi`) into
//! the named scopes. Blocks without one are shared: they are part of every
//! scope's document. A scoped document is merged from the shared snippets
//! and those of its scope, then [pruned](prune).

use crate::document::for_each_operation_ref;
use crate::lint;
use serde_json::Value;
use std::collections::BTreeSet;

/// The directive that names the scopes of a block.
const DIRECTIVE: &str = "@scope";

/// The scope names of a `@scope` directive (the text after `@scope`):
/// comma-separated, each made of letters, digits, `.`, `-` and `_`. `None`
/// when there is no name or one is invalid.
pub(crate) fn names(text: &str) -> Option<Vec<String>> {
    let mut names = Vec::new();
    for name in text.split(',').map(str::trim) {
        if !is_name(name) {
            return None;
        }
        if !names.iter().any(|n| n == name) {
            names.push(name.to_string());
        }
    }
    Some(names)
}

/// Whether `name` is a valid scope name: not empty, only letters, digits,
/// `.`, `-` and `_`.
fn is_name(name: &str) -> bool {
    !name.is_empty()
        && name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || "._-".contains(c))
}

/// Appends the `names` that `list` does not contain yet, in order.
pub(crate) fn add(list: &mut Vec<String>, names: Vec<String>) {
    for name in names {
        if !list.contains(&name) {
            list.push(name);
        }
    }
}

/// The text after `@scope` when `line` is a `@scope` directive.
pub(crate) fn directive(line: &str) -> Option<&str> {
    let rest = line.trim().strip_prefix(DIRECTIVE)?;
    (rest.is_empty() || rest.starts_with(char::is_whitespace)).then(|| rest.trim())
}

/// The `@scope` lines of a YAML block, taken out of `text`.
#[derive(Debug, Default, PartialEq, Eq)]
pub(crate) struct ScopeLines {
    /// `text` with every `@scope` line blanked, so line numbers of later
    /// findings stay right.
    pub(crate) text: String,
    /// The scope names of the valid lines, in order and without
    /// duplicates.
    pub(crate) names: Vec<String>,
    /// The 0-based line numbers of `@scope` lines without valid names.
    pub(crate) invalid: Vec<usize>,
}

/// Takes the `@scope` lines out of the YAML `text` of a block (a `@scope`
/// line is no valid YAML).
pub(crate) fn take_lines(text: &str) -> ScopeLines {
    let mut found = ScopeLines::default();
    if !text.contains(DIRECTIVE) {
        found.text = text.to_string();
        return found;
    }
    let mut lines = Vec::new();
    for (index, line) in text.split('\n').enumerate() {
        match directive(line) {
            Some(rest) => {
                match names(rest) {
                    Some(names) => add(&mut found.names, names),
                    None => found.invalid.push(index),
                }
                lines.push("");
            }
            None => lines.push(line),
        }
    }
    found.text = lines.join("\n");
    found
}

/// Whether a snippet of the scopes `scopes` (empty: shared) is part of the
/// document of `scope` (`None`: the document of every snippet).
pub(crate) fn includes(scopes: &[String], scope: Option<&str>) -> bool {
    match scope {
        None => true,
        Some(scope) => scopes.is_empty() || scopes.iter().any(|s| s == scope),
    }
}

/// Removes what a scoped document does not use: the components nothing
/// outside `components` refers to (directly or through other components;
/// security schemes and extensions stay), and the top-level tags that no
/// operation uses and that are no parent of a used tag. A section (or
/// `components`, or `tags`) emptied this way is removed.
pub(crate) fn prune(document: &mut Value) {
    let used = lint::reachable_components(document);
    if let Some(Value::Object(components)) = document.get_mut("components") {
        let mut emptied = Vec::new();
        for (section, entries) in components.iter_mut() {
            if section == "securitySchemes" || section.starts_with("x-") {
                continue;
            }
            let Value::Object(entries) = entries else {
                continue;
            };
            let before = entries.len();
            entries.retain(|name, _| {
                let escaped = name.replace('~', "~0").replace('/', "~1");
                used.contains(&(section.clone(), escaped))
            });
            if entries.is_empty() && before > 0 {
                emptied.push(section.clone());
            }
        }
        for section in &emptied {
            components.shift_remove(section);
        }
        if components.is_empty()
            && !emptied.is_empty()
            && let Some(map) = document.as_object_mut()
        {
            map.shift_remove("components");
        }
    }
    prune_tags(document);
}

/// Removes the top-level tags no operation uses, keeping the parents
/// (OpenAPI 3.2 `parent`) of used tags.
fn prune_tags(document: &mut Value) {
    let mut used = BTreeSet::new();
    for_each_operation_ref(document, |_, operation| {
        if let Some(Value::Array(tags)) = operation.get("tags") {
            used.extend(tags.iter().filter_map(Value::as_str).map(str::to_string));
        }
    });
    let Some(map) = document.as_object_mut() else {
        return;
    };
    let Some(Value::Array(tags)) = map.get_mut("tags") else {
        return;
    };
    let name = |tag: &Value| tag.get("name").and_then(Value::as_str).map(str::to_string);
    // Parents of used tags are used too, up the hierarchy.
    loop {
        let parents: Vec<String> = tags
            .iter()
            .filter(|tag| name(tag).is_some_and(|n| used.contains(&n)))
            .filter_map(|tag| tag.get("parent").and_then(Value::as_str))
            .filter(|parent| !used.contains(*parent))
            .map(str::to_string)
            .collect();
        if parents.is_empty() {
            break;
        }
        used.extend(parents);
    }
    let before = tags.len();
    tags.retain(|tag| name(tag).is_some_and(|n| used.contains(&n)));
    if tags.is_empty() && before > 0 {
        map.shift_remove("tags");
    }
}
