//! Output strategies and file writing.

use super::error::{Error, Result};
use super::template::registry::{FORMAT_KEY, FRAGMENTS_KEY, Registry, TEMPLATES_KEY};
use super::yaml;
use serde_json::{Map, Value};
use std::path::Path;

/// What part of the merged document an output receives.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[non_exhaustive]
pub enum OutputKind {
    /// The complete specification; requires a root with `openapi` and `info`.
    Full,
    /// Only `components.schemas` (a map of schemas).
    Schemas,
    /// Only `paths` (a map of path items).
    Paths,
    /// The specification without `openapi`, `info` and `servers`, plus the
    /// templates of this crate for consumption by other crates.
    Fragments,
}

/// File format, chosen by the file extension (`.json` → JSON, otherwise YAML).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum Format {
    /// YAML
    Yaml,
    /// Pretty-printed JSON
    Json,
}

impl Format {
    /// The format for `path`.
    pub fn of(path: &Path) -> Self {
        match path.extension().and_then(|e| e.to_str()) {
            Some("json") => Self::Json,
            _ => Self::Yaml,
        }
    }

    /// Serializes `value` in this format (always ending with a newline).
    pub fn render(self, value: &Value) -> String {
        match self {
            Self::Json => {
                let mut text = serde_json::to_string_pretty(value).unwrap_or_default();
                text.push('\n');
                text
            }
            Self::Yaml => yaml::to_string(value),
        }
    }
}

/// Top-level keys of an OpenAPI/AsyncAPI document in specification order.
const ROOT_ORDER: &[&str] = &[
    "openapi",
    "asyncapi",
    "$self",
    "info",
    "jsonSchemaDialect",
    "servers",
    "channels",
    "operations",
    "paths",
    "webhooks",
    "components",
    "security",
    "tags",
    "externalDocs",
];

/// Whether `document` has the root keys `openapi` and `info`.
pub(crate) fn is_root(document: &Value) -> bool {
    document
        .as_object()
        .is_some_and(|m| m.contains_key("openapi") && m.contains_key("info"))
}

/// The value an output of `kind` receives.
pub(crate) fn render(kind: OutputKind, document: &Value, registry: &Registry) -> Result<Value> {
    match kind {
        OutputKind::Full => {
            if !is_root(document) {
                return Err(Error::NoRootFound);
            }
            Ok(ordered(document.clone()))
        }
        OutputKind::Schemas => Ok(document
            .pointer("/components/schemas")
            .cloned()
            .unwrap_or_else(|| Value::Object(Map::new()))),
        OutputKind::Paths => Ok(document
            .get("paths")
            .cloned()
            .unwrap_or_else(|| Value::Object(Map::new()))),
        OutputKind::Fragments => {
            let mut fragment = document.clone();
            if let Value::Object(map) = &mut fragment {
                for key in ["openapi", "info", "servers"] {
                    map.remove(key);
                }
                let transport = registry.export();
                if !transport.is_empty() {
                    let components = map
                        .entry("components")
                        .or_insert_with(|| Value::Object(Map::new()));
                    if let Value::Object(components) = components {
                        components.extend(transport);
                    }
                }
            }
            Ok(ordered(fragment))
        }
    }
}

/// Removes the template transport keys (they never belong in a final spec).
pub(crate) fn strip_transport(document: &mut Value) {
    let Some(map) = document.as_object_mut() else {
        return;
    };
    let Some(Value::Object(components)) = map.get_mut("components") else {
        return;
    };
    for key in [TEMPLATES_KEY, FRAGMENTS_KEY, FORMAT_KEY] {
        components.remove(key);
    }
    if components.is_empty() {
        map.remove("components");
    }
}

/// Specification order at the top level, sorted `components.*` maps (ADR 0007).
fn ordered(document: Value) -> Value {
    let Value::Object(mut map) = document else {
        return document;
    };
    let mut out = Map::with_capacity(map.len());
    for key in ROOT_ORDER {
        if let Some(value) = map.remove(*key) {
            out.insert((*key).to_string(), value);
        }
    }
    out.extend(map);
    if let Some(Value::Object(components)) = out.get_mut("components") {
        for section in components.values_mut() {
            if let Value::Object(entries) = section {
                let mut sorted: Vec<_> = std::mem::take(entries).into_iter().collect();
                sorted.sort_by(|a, b| a.0.cmp(&b.0));
                entries.extend(sorted);
            }
        }
    }
    Value::Object(out)
}

/// Writes `text` to `path` atomically, unless the file already has exactly
/// this content. Returns whether the file was written.
pub(crate) fn write_if_changed(path: &Path, text: &str) -> Result<bool> {
    if std::fs::read_to_string(path).is_ok_and(|existing| existing == text) {
        return Ok(false);
    }
    if let Some(parent) = path.parent().filter(|p| !p.as_os_str().is_empty()) {
        std::fs::create_dir_all(parent).map_err(|e| Error::io(parent, e))?;
    }
    let file_name = path
        .file_name()
        .ok_or_else(|| Error::io(path, std::io::Error::other("output path has no file name")))?;
    let mut temp_name = std::ffi::OsString::from(".");
    temp_name.push(file_name);
    temp_name.push(format!(".{}.tmp", std::process::id()));
    let temp = path.with_file_name(temp_name);
    std::fs::write(&temp, text).map_err(|e| Error::io(&temp, e))?;
    std::fs::rename(&temp, path).map_err(|e| {
        let _ = std::fs::remove_file(&temp);
        Error::io(path, e)
    })?;
    Ok(true)
}
