//! Output strategies and file writing.
//!
//! Besides the OpenAPI document itself (in full or in parts), the generator
//! exports the same API as a JSON Schema bundle, an OpenAPI 3.0 or Swagger 2.0
//! document. Constructs a target cannot express are approximated and reported.

pub(crate) mod asyncapi;
mod json_schema;
mod openapi30;
mod refs;
mod swagger2;

use super::diagnostics::Diagnostics;
use super::error::{Error, Result};
use super::template::registry::{FORMAT_KEY, FRAGMENTS_KEY, Registry, TEMPLATES_KEY};
use super::yaml;
use serde_json::{Map, Value};
use std::path::Path;

/// What part of the merged document an output receives.
///
/// # Examples
///
/// ```
/// use oas_forge::{Generator, OutputKind};
/// use serde_json::json;
///
/// let src = tempfile::tempdir()?;
/// std::fs::write(
///     src.path().join("lib.rs"),
///     "/// @openapi\npub struct Tag { pub name: String }\n",
/// )?;
/// let spec = Generator::new().input(src.path()).without_package().build()?;
///
/// assert_eq!(
///     spec.render(OutputKind::Schemas)?,
///     json!({
///         "Tag": {
///             "type": "object",
///             "properties": { "name": { "type": "string" } },
///             "required": ["name"]
///         }
///     })
/// );
/// // The full document needs a root (`openapi` and `info`).
/// assert!(spec.render(OutputKind::Full).is_err());
/// # Ok::<(), Box<dyn std::error::Error>>(())
/// ```
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
    /// The component schemas as a JSON Schema 2020-12 bundle (`$defs`).
    JsonSchema,
    /// The full specification lowered to OpenAPI 3.0.3.
    OpenApi30,
    /// The full specification converted to Swagger 2.0.
    Swagger20,
    /// The AsyncAPI document of `@asyncapi` and `@channel` blocks; requires a
    /// root with `asyncapi` and `info`.
    AsyncApi,
}

/// File format, chosen by the file extension (`.json` → JSON, otherwise YAML).
///
/// # Examples
///
/// ```
/// use oas_forge::Format;
/// use serde_json::json;
///
/// let text = Format::Json.render(&json!({ "openapi": "3.1.0" }));
/// assert_eq!(text, "{\n  \"openapi\": \"3.1.0\"\n}\n");
/// ```
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
    ///
    /// # Examples
    ///
    /// ```
    /// use oas_forge::Format;
    /// use std::path::Path;
    ///
    /// assert_eq!(Format::of(Path::new("openapi.json")), Format::Json);
    /// assert_eq!(Format::of(Path::new("openapi.yaml")), Format::Yaml);
    /// assert_eq!(Format::of(Path::new("openapi")), Format::Yaml);
    /// ```
    pub fn of(path: &Path) -> Self {
        match path.extension().and_then(|e| e.to_str()) {
            Some("json") => Self::Json,
            _ => Self::Yaml,
        }
    }

    /// Serializes `value` in this format (always ending with a newline).
    ///
    /// # Examples
    ///
    /// ```
    /// use oas_forge::Format;
    /// use serde_json::json;
    ///
    /// let document = json!({
    ///     "openapi": "3.1.0",
    ///     "info": { "title": "Shop", "version": "1.0" },
    ///     "paths": {}
    /// });
    /// assert_eq!(
    ///     Format::Yaml.render(&document),
    ///     "openapi: \"3.1.0\"\ninfo:\n  title: Shop\n  version: \"1.0\"\npaths: {}\n"
    /// );
    /// ```
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

/// Top-level keys of an OpenAPI/Swagger/AsyncAPI document and of a JSON
/// Schema bundle in specification order.
const ROOT_ORDER: &[&str] = &[
    "$schema",
    "swagger",
    "openapi",
    "asyncapi",
    "$self",
    "info",
    "host",
    "basePath",
    "schemes",
    "consumes",
    "produces",
    "jsonSchemaDialect",
    "servers",
    "channels",
    "operations",
    "paths",
    "webhooks",
    "components",
    "definitions",
    "parameters",
    "responses",
    "securityDefinitions",
    "$defs",
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

/// What outputs are rendered from.
pub(crate) struct RenderInput<'a> {
    /// The merged document in the dialect of its root.
    pub document: &'a Value,
    /// The merged document in the canonical dialect (JSON Schema 2020-12),
    /// before version-specific lowering.
    pub canonical: &'a Value,
    /// Top-level keys only the root generated from the package defines.
    pub generated_keys: &'a [String],
    /// The merged AsyncAPI document, with the schemas it uses.
    pub asyncapi: &'a Value,
    /// The templates of the run.
    pub registry: &'a Registry,
}

/// The value an output of `kind` receives; approximations are reported to
/// `diagnostics`.
pub(crate) fn render(
    kind: OutputKind,
    input: &RenderInput<'_>,
    diagnostics: &mut Diagnostics,
) -> Result<Value> {
    let document = input.document;
    match kind {
        OutputKind::Full => {
            if !is_root(document) {
                return Err(Error::NoRootFound);
            }
            let mut full = document.clone();
            if let Value::Object(map) = &mut full {
                add_required_paths(map);
            }
            Ok(ordered(full))
        }
        OutputKind::Schemas => {
            let mut schemas = document
                .pointer("/components/schemas")
                .cloned()
                .unwrap_or_else(|| Value::Object(Map::new()));
            sort_entries(&mut schemas);
            Ok(schemas)
        }
        OutputKind::Paths => Ok(document
            .get("paths")
            .cloned()
            .unwrap_or_else(|| Value::Object(Map::new()))),
        OutputKind::Fragments => {
            // Fragments travel to other crates, which lower them for their
            // own root: keep the canonical dialect.
            let mut fragment = input.canonical.clone();
            if let Value::Object(map) = &mut fragment {
                for key in ["openapi", "info", "servers"] {
                    map.remove(key);
                }
                // A root generated from this crate's package is not part of
                // what other crates include.
                for key in input.generated_keys {
                    map.remove(key);
                }
                let transport = input.registry.export();
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
        OutputKind::JsonSchema => Ok(ordered(json_schema::bundle(input.canonical))),
        OutputKind::OpenApi30 => {
            if !is_root(input.canonical) {
                return Err(Error::NoRootFound);
            }
            Ok(ordered(openapi30::export(input.canonical, diagnostics)))
        }
        OutputKind::Swagger20 => {
            if !is_root(input.canonical) {
                return Err(Error::NoRootFound);
            }
            Ok(ordered(swagger2::export(input.canonical, diagnostics)))
        }
        OutputKind::AsyncApi => {
            let root = input
                .asyncapi
                .as_object()
                .is_some_and(|m| m.contains_key("asyncapi") && m.contains_key("info"));
            if !root {
                return Err(Error::NoAsyncApiRoot);
            }
            Ok(ordered(asyncapi::export(input.asyncapi, diagnostics)))
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
        components.values_mut().for_each(sort_entries);
    }
    for section in [
        "$defs",
        "definitions",
        "parameters",
        "responses",
        "securityDefinitions",
    ] {
        if let Some(entries) = out.get_mut(section) {
            sort_entries(entries);
        }
    }
    Value::Object(out)
}

/// `paths: {}` where the version requires it: OpenAPI 3.0 always, 3.1 and
/// 3.2 when the document has neither `components` nor `webhooks`.
fn add_required_paths(map: &mut Map<String, Value>) {
    if map.contains_key("paths") {
        return;
    }
    let version_30 = map
        .get("openapi")
        .and_then(Value::as_str)
        .is_some_and(|v| v.starts_with("3.0"));
    if version_30 || !(map.contains_key("components") || map.contains_key("webhooks")) {
        map.insert("paths".into(), Value::Object(Map::new()));
    }
}

/// Sorts the entries of a map by name.
fn sort_entries(section: &mut Value) {
    if let Value::Object(entries) = section {
        let mut sorted: Vec<_> = std::mem::take(entries).into_iter().collect();
        sorted.sort_by(|a, b| a.0.cmp(&b.0));
        entries.extend(sorted);
    }
}

/// Whether the file at `path` already has the content `text`. Line endings
/// do not count, so a file that Git checked out with `\r\n` is current.
pub(crate) fn is_current(path: &Path, text: &str) -> bool {
    std::fs::read_to_string(path).is_ok_and(|existing| {
        existing == text || (existing.contains('\r') && existing.replace("\r\n", "\n") == text)
    })
}

/// Writes `text` to `path` atomically, unless the file already has this
/// content ([`is_current`]). Returns whether the file was written.
pub(crate) fn write_if_changed(path: &Path, text: &str) -> Result<bool> {
    if is_current(path, text) {
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
