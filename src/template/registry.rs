//! Registry of fragments, blueprints and schema name aliases, and the
//! cross-crate wire format (ADR 0005).

use crate::diagnostics::{Code, Diagnostic, Diagnostics, Location};
use crate::yaml;
use serde_json::{Map, Value, json};
use std::collections::BTreeMap;

/// Vendor extension carrying blueprints in headless outputs.
pub const TEMPLATES_KEY: &str = "x-oas-forge-templates";
/// Vendor extension carrying fragments in headless outputs.
pub const FRAGMENTS_KEY: &str = "x-oas-forge-fragments";
/// Vendor extension carrying the wire format version.
pub const FORMAT_KEY: &str = "x-oas-forge-format";
/// The wire format version written by this release.
pub const FORMAT_VERSION: u64 = 1;

/// A reusable text fragment (`@openapi-fragment Name(params)`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Fragment {
    /// Parameter names, substituted as `{{param}}`.
    pub params: Vec<String>,
    /// The body text.
    pub body: String,
}

/// A generic schema template (`@openapi<T>`).
#[derive(Debug, Clone, PartialEq)]
pub struct Blueprint {
    /// Type parameter names, referenced as `$T` in the schema.
    pub params: Vec<String>,
    /// The schema with `$T` placeholders.
    pub schema: Value,
}

/// Everything later stages need to know about templates and names.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Registry {
    fragments: BTreeMap<String, Fragment>,
    blueprints: BTreeMap<String, Blueprint>,
    aliases: BTreeMap<String, String>,
}

impl Registry {
    /// An empty registry.
    pub fn new() -> Self {
        Self::default()
    }

    /// Registers a fragment (a later definition replaces an earlier one).
    pub fn add_fragment(&mut self, name: impl Into<String>, fragment: Fragment) {
        self.fragments.insert(name.into(), fragment);
    }

    /// Registers a blueprint (a later definition replaces an earlier one).
    pub fn add_blueprint(&mut self, name: impl Into<String>, blueprint: Blueprint) {
        self.blueprints.insert(name.into(), blueprint);
    }

    /// Records that the Rust type `rust_name` is published as schema `name`.
    pub fn add_alias(&mut self, rust_name: impl Into<String>, name: impl Into<String>) {
        let (rust_name, name) = (rust_name.into(), name.into());
        if rust_name != name {
            self.aliases.insert(rust_name, name);
        }
    }

    /// A fragment by name.
    pub fn fragment(&self, name: &str) -> Option<&Fragment> {
        self.fragments.get(name)
    }

    /// A blueprint by name.
    pub fn blueprint(&self, name: &str) -> Option<&Blueprint> {
        self.blueprints.get(name)
    }

    /// The schema name of a Rust type that was renamed.
    pub fn alias(&self, rust_name: &str) -> Option<&str> {
        self.aliases.get(rust_name).map(String::as_str)
    }

    /// Every fragment, sorted by name.
    pub fn fragments(&self) -> impl Iterator<Item = (&String, &Fragment)> {
        self.fragments.iter()
    }

    /// Every blueprint, sorted by name.
    pub fn blueprints(&self) -> impl Iterator<Item = (&String, &Blueprint)> {
        self.blueprints.iter()
    }

    /// Whether there is anything to transport to other crates.
    pub fn has_templates(&self) -> bool {
        !self.fragments.is_empty() || !self.blueprints.is_empty()
    }

    /// Reads and removes the vendor extensions of an imported document's
    /// `components`, registering their templates.
    pub fn import(
        &mut self,
        document: &mut Value,
        location: &Location,
        diagnostics: &mut Diagnostics,
    ) {
        let Some(components) = document
            .get_mut("components")
            .and_then(Value::as_object_mut)
        else {
            return;
        };
        if let Some(version) = components.remove(FORMAT_KEY)
            && version.as_u64() != Some(FORMAT_VERSION)
        {
            diagnostics.push(
                Diagnostic::new(
                    Code::UnknownTemplateFormat,
                    format!("template wire format {version} is not supported; reading it as format {FORMAT_VERSION}"),
                )
                .at(location.clone()),
            );
        }
        if let Some(templates) = components.remove(TEMPLATES_KEY) {
            for (name, params, body) in entries(&templates) {
                match yaml::parse(&body) {
                    Ok(schema) => self.add_blueprint(name, Blueprint { params, schema }),
                    Err(e) => diagnostics.push(
                        Diagnostic::new(
                            Code::InvalidFragment,
                            format!(
                                "imported template `{name}` is not valid YAML (line {} of its body): {}",
                                e.line, e.message
                            ),
                        )
                        .at(location.clone()),
                    ),
                }
            }
        }
        if let Some(fragments) = components.remove(FRAGMENTS_KEY) {
            for (name, params, body) in entries(&fragments) {
                self.add_fragment(name, Fragment { params, body });
            }
        }
    }

    /// The vendor extensions that transport this registry's templates.
    pub fn export(&self) -> Map<String, Value> {
        let mut map = Map::new();
        if !self.blueprints.is_empty() {
            let templates: Map<String, Value> = self
                .blueprints
                .iter()
                .map(|(name, bp)| {
                    (
                        name.clone(),
                        json!({ "params": bp.params, "body": yaml::to_string(&bp.schema) }),
                    )
                })
                .collect();
            map.insert(TEMPLATES_KEY.into(), Value::Object(templates));
        }
        if !self.fragments.is_empty() {
            let fragments: Map<String, Value> = self
                .fragments
                .iter()
                .map(|(name, f)| (name.clone(), json!({ "params": f.params, "body": f.body })))
                .collect();
            map.insert(FRAGMENTS_KEY.into(), Value::Object(fragments));
        }
        if !map.is_empty() {
            map.insert(FORMAT_KEY.into(), json!(FORMAT_VERSION));
        }
        map
    }
}

/// `{Name: {params: [..], body: ".."}}` entries; malformed entries are skipped.
fn entries(value: &Value) -> Vec<(String, Vec<String>, String)> {
    let Some(map) = value.as_object() else {
        return Vec::new();
    };
    map.iter()
        .filter_map(|(name, entry)| {
            let body = entry.get("body")?.as_str()?.to_string();
            let params = entry
                .get("params")
                .and_then(Value::as_array)
                .map(|p| {
                    p.iter()
                        .filter_map(Value::as_str)
                        .map(str::to_string)
                        .collect()
                })
                .unwrap_or_default();
            Some((name.clone(), params, body))
        })
        .collect()
}
