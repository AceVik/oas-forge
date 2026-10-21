//! Registry of fragments, blueprints and schema name aliases, and the
//! cross-crate wire format (ADR 0005).

use crate::diagnostics::{Code, Diagnostic, Diagnostics, Location};
use crate::extract::AliasItem;
use crate::yaml;
use serde_json::{Map, Value, json};
use std::collections::BTreeMap;
use std::path::Path;

/// Vendor extension carrying blueprints in headless outputs.
pub const TEMPLATES_KEY: &str = "x-oas-forge-templates";
/// Vendor extension carrying fragments in headless outputs.
pub const FRAGMENTS_KEY: &str = "x-oas-forge-fragments";
/// Vendor extension carrying the wire format version.
pub const FORMAT_KEY: &str = "x-oas-forge-format";
/// The wire format version written by this release.
pub const FORMAT_VERSION: u64 = 1;

/// A reusable text fragment (`@openapi-fragment Name(params)`).
///
/// # Examples
///
/// ```
/// use oas_forge::diagnostics::{Diagnostics, Location};
/// use oas_forge::template::{Fragment, Registry, Variables, lexical};
///
/// let mut registry = Registry::new();
/// registry.add_fragment(
///     "Limit",
///     Fragment {
///         params: vec!["max".into()],
///         body: "limit:\n  type: integer\n  maximum: {{max}}\n".into(),
///     },
/// );
/// let mut diagnostics = Diagnostics::new();
/// let text = lexical::expand(
///     "@insert Limit(100)",
///     &registry,
///     &Variables::new(),
///     &Location::new("src/lib.rs", 1),
///     &mut diagnostics,
/// );
/// assert_eq!(text, "limit:\n  type: integer\n  maximum: 100");
/// ```
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Fragment {
    /// Parameter names, substituted as `{{param}}`.
    pub params: Vec<String>,
    /// The body text.
    pub body: String,
}

/// A generic schema template (`@openapi<T>`).
///
/// # Examples
///
/// ```
/// use oas_forge::diagnostics::{Diagnostics, Location};
/// use oas_forge::template::generics::Instantiator;
/// use oas_forge::template::{Blueprint, Registry};
/// use serde_json::json;
///
/// let mut registry = Registry::new();
/// registry.add_blueprint(
///     "Envelope",
///     Blueprint {
///         params: vec!["T".into()],
///         schema: json!({ "properties": { "data": { "$ref": "$T" } } }),
///     },
/// );
/// let mut value = json!({ "$ref": "$Envelope<Order>" });
/// let mut diagnostics = Diagnostics::new();
/// let mut instantiator = Instantiator::new(&registry);
/// instantiator.apply(&mut value, &Location::new("src/lib.rs", 1), &mut diagnostics);
///
/// assert_eq!(value, json!({ "$ref": "$Envelope_Order" }));
/// assert_eq!(
///     instantiator.into_concrete()["Envelope_Order"].0,
///     json!({ "properties": { "data": { "$ref": "$Order" } } })
/// );
/// ```
#[derive(Debug, Clone, PartialEq)]
pub struct Blueprint {
    /// Type parameter names, referenced as `$T` in the schema.
    pub params: Vec<String>,
    /// The schema with `$T` placeholders.
    pub schema: Value,
}

/// Everything later stages need to know about templates and names.
///
/// # Examples
///
/// The registry of a generator run holds the fragments of its sources:
///
/// ```
/// use oas_forge::Generator;
///
/// let dir = tempfile::tempdir()?;
/// std::fs::write(
///     dir.path().join("lib.rs"),
///     "//! @openapi-fragment Paging\n//! limit: {type: integer}\n",
/// )?;
/// let spec = Generator::new().input(dir.path()).build()?;
/// let paging = spec.registry().fragment("Paging").ok_or("no fragment")?;
/// assert_eq!(paging.body, "limit: {type: integer}");
/// # Ok::<(), Box<dyn std::error::Error>>(())
/// ```
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Registry {
    /// The fragments by name.
    fragments: BTreeMap<String, Fragment>,
    /// The blueprints by name.
    blueprints: BTreeMap<String, Blueprint>,
    /// The schema names of renamed Rust types, by Rust type name.
    aliases: BTreeMap<String, String>,
    /// Type aliases without `@openapi`, by Rust name, in source order.
    transparent: BTreeMap<String, Vec<AliasItem>>,
}

/// What a name means as a transparent type alias where it is used
/// ([`Registry::transparent`]).
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum Transparent<'a> {
    /// No type alias has the name.
    None,
    /// The alias of that name in the file where the name is used.
    Local(&'a AliasItem),
    /// The alias of that name in other files (all definitions agree).
    Foreign(&'a AliasItem),
    /// Several aliases of that name with different targets.
    Ambiguous(Vec<&'a AliasItem>),
}

impl Registry {
    /// An empty registry.
    ///
    /// # Examples
    ///
    /// ```
    /// let registry = oas_forge::template::Registry::new();
    /// assert!(!registry.has_templates());
    /// assert_eq!(registry.fragments().count(), 0);
    /// ```
    pub fn new() -> Self {
        Self::default()
    }

    /// Registers a fragment (a later definition replaces an earlier one).
    ///
    /// # Examples
    ///
    /// ```
    /// use oas_forge::template::{Fragment, Registry};
    ///
    /// let mut registry = Registry::new();
    /// for body in ["type: string", "type: integer"] {
    ///     registry.add_fragment("Id", Fragment { params: Vec::new(), body: body.into() });
    /// }
    /// let id = registry.fragment("Id").map(|f| f.body.as_str());
    /// assert_eq!(id, Some("type: integer"));
    /// ```
    pub fn add_fragment(&mut self, name: impl Into<String>, fragment: Fragment) {
        self.fragments.insert(name.into(), fragment);
    }

    /// Registers a blueprint (a later definition replaces an earlier one).
    ///
    /// # Examples
    ///
    /// ```
    /// use oas_forge::template::{Blueprint, Registry};
    /// use serde_json::json;
    ///
    /// let mut registry = Registry::new();
    /// let schema = json!({ "type": "array", "items": { "$ref": "$T" } });
    /// registry.add_blueprint("List", Blueprint { params: vec!["T".into()], schema });
    /// assert_eq!(registry.blueprint("List").map(|b| b.params.len()), Some(1));
    /// ```
    pub fn add_blueprint(&mut self, name: impl Into<String>, blueprint: Blueprint) {
        self.blueprints.insert(name.into(), blueprint);
    }

    /// Records that the Rust type `rust_name` is published as schema `name`;
    /// a name equal to `rust_name` is no alias and is not recorded.
    ///
    /// # Examples
    ///
    /// ```
    /// let mut registry = oas_forge::template::Registry::new();
    /// registry.add_alias("UserDto", "User");
    /// registry.add_alias("Order", "Order");
    /// assert_eq!(registry.alias("UserDto"), Some("User"));
    /// assert_eq!(registry.alias("Order"), None);
    /// ```
    pub fn add_alias(&mut self, rust_name: impl Into<String>, name: impl Into<String>) {
        let (rust_name, name) = (rust_name.into(), name.into());
        if rust_name != name {
            self.aliases.insert(rust_name, name);
        }
    }

    /// Records a type alias without `@openapi`
    /// ([`Item::Alias`](crate::extract::Item::Alias)).
    pub(crate) fn add_transparent(&mut self, alias: AliasItem) {
        self.transparent
            .entry(alias.rust_name.clone())
            .or_default()
            .push(alias);
    }

    /// What `name` means as a transparent type alias in `file`: an alias
    /// defined in `file` wins over those of other files; aliases of the same
    /// name and target count as one.
    pub(crate) fn transparent(&self, name: &str, file: &Path) -> Transparent<'_> {
        let Some(all) = self.transparent.get(name) else {
            return Transparent::None;
        };
        let local: Vec<&AliasItem> = all.iter().filter(|a| a.location.file() == file).collect();
        let (candidates, is_local) = if local.is_empty() {
            (all.iter().collect(), false)
        } else {
            (local, true)
        };
        let mut distinct: Vec<&AliasItem> = Vec::new();
        for alias in candidates {
            if !distinct
                .iter()
                .any(|d| d.params == alias.params && d.schema == alias.schema)
            {
                distinct.push(alias);
            }
        }
        match distinct[..] {
            [alias] if is_local => Transparent::Local(alias),
            [alias] => Transparent::Foreign(alias),
            _ => Transparent::Ambiguous(distinct),
        }
    }

    /// A fragment by name.
    ///
    /// # Examples
    ///
    /// ```
    /// use oas_forge::template::{Fragment, Registry};
    ///
    /// let mut registry = Registry::new();
    /// let fragment = Fragment { params: vec!["role".into()], body: "x-role: {{role}}".into() };
    /// registry.add_fragment("Secured", fragment.clone());
    /// assert_eq!(registry.fragment("Secured"), Some(&fragment));
    /// assert_eq!(registry.fragment("Paging"), None);
    /// ```
    pub fn fragment(&self, name: &str) -> Option<&Fragment> {
        self.fragments.get(name)
    }

    /// A blueprint by name.
    ///
    /// # Examples
    ///
    /// ```
    /// use oas_forge::template::{Blueprint, Registry};
    /// use serde_json::json;
    ///
    /// let mut registry = Registry::new();
    /// let page = Blueprint { params: vec!["T".into()], schema: json!({ "items": "$T" }) };
    /// registry.add_blueprint("Page", page.clone());
    /// assert_eq!(registry.blueprint("Page"), Some(&page));
    /// assert_eq!(registry.blueprint("Envelope"), None);
    /// ```
    pub fn blueprint(&self, name: &str) -> Option<&Blueprint> {
        self.blueprints.get(name)
    }

    /// The schema name of a Rust type that was renamed.
    ///
    /// # Examples
    ///
    /// ```
    /// let mut registry = oas_forge::template::Registry::new();
    /// registry.add_alias("UserDto", "User");
    /// assert_eq!(registry.alias("UserDto"), Some("User"));
    /// assert_eq!(registry.alias("User"), None);
    /// ```
    pub fn alias(&self, rust_name: &str) -> Option<&str> {
        self.aliases.get(rust_name).map(String::as_str)
    }

    /// Every fragment, sorted by name.
    ///
    /// # Examples
    ///
    /// ```
    /// use oas_forge::template::{Fragment, Registry};
    ///
    /// let mut registry = Registry::new();
    /// for name in ["Secured", "Paging"] {
    ///     registry.add_fragment(name, Fragment { params: Vec::new(), body: String::new() });
    /// }
    /// let names: Vec<&String> = registry.fragments().map(|(name, _)| name).collect();
    /// assert_eq!(names, ["Paging", "Secured"]);
    /// ```
    pub fn fragments(&self) -> impl Iterator<Item = (&String, &Fragment)> {
        self.fragments.iter()
    }

    /// Every blueprint, sorted by name.
    ///
    /// # Examples
    ///
    /// ```
    /// use oas_forge::template::{Blueprint, Registry};
    /// use serde_json::json;
    ///
    /// let mut registry = Registry::new();
    /// for name in ["Page", "Envelope"] {
    ///     let schema = json!({ "items": "$T" });
    ///     registry.add_blueprint(name, Blueprint { params: vec!["T".into()], schema });
    /// }
    /// let names: Vec<&String> = registry.blueprints().map(|(name, _)| name).collect();
    /// assert_eq!(names, ["Envelope", "Page"]);
    /// ```
    pub fn blueprints(&self) -> impl Iterator<Item = (&String, &Blueprint)> {
        self.blueprints.iter()
    }

    /// Whether there is anything to transport to other crates: fragments or
    /// blueprints (aliases are not transported).
    ///
    /// # Examples
    ///
    /// ```
    /// use oas_forge::template::{Fragment, Registry};
    ///
    /// let mut registry = Registry::new();
    /// registry.add_alias("UserDto", "User");
    /// assert!(!registry.has_templates());
    /// registry.add_fragment("Paging", Fragment { params: Vec::new(), body: String::new() });
    /// assert!(registry.has_templates());
    /// ```
    pub fn has_templates(&self) -> bool {
        !self.fragments.is_empty() || !self.blueprints.is_empty()
    }

    /// Reads and removes the vendor extensions of an imported document's
    /// `components`, registering their templates. An unknown wire format
    /// version ([`Code::UnknownTemplateFormat`]) and blueprint bodies that
    /// are not valid YAML ([`Code::InvalidFragment`]) are reported at
    /// `location`; malformed entries are skipped.
    ///
    /// # Examples
    ///
    /// ```
    /// use oas_forge::diagnostics::{Diagnostics, Location};
    /// use oas_forge::template::{Fragment, Registry};
    /// use serde_json::json;
    ///
    /// let mut shared = Registry::new();
    /// shared.add_fragment("Paging", Fragment { params: Vec::new(), body: "limit: 10".into() });
    /// let mut document = json!({ "openapi": "3.1.0", "components": shared.export() });
    ///
    /// let mut registry = Registry::new();
    /// let mut diagnostics = Diagnostics::new();
    /// let location = Location::new("shared/openapi.json", 1);
    /// registry.import(&mut document, &location, &mut diagnostics);
    ///
    /// assert_eq!(registry, shared);
    /// assert_eq!(document, json!({ "openapi": "3.1.0", "components": {} }));
    /// assert!(diagnostics.is_empty());
    /// ```
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

    /// The vendor extensions that transport this registry's templates
    /// (empty without templates); blueprints travel as YAML text.
    ///
    /// # Examples
    ///
    /// ```
    /// use oas_forge::template::{Blueprint, Fragment, Registry};
    /// use serde_json::{Value, json};
    ///
    /// let mut registry = Registry::new();
    /// registry.add_fragment(
    ///     "Limit",
    ///     Fragment { params: vec!["max".into()], body: "maximum: {{max}}".into() },
    /// );
    /// registry.add_blueprint(
    ///     "List",
    ///     Blueprint { params: vec!["T".into()], schema: json!({ "items": { "$ref": "$T" } }) },
    /// );
    /// assert_eq!(
    ///     Value::Object(registry.export()),
    ///     json!({
    ///         "x-oas-forge-templates": {
    ///             "List": { "params": ["T"], "body": "items:\n  $ref: $T\n" }
    ///         },
    ///         "x-oas-forge-fragments": {
    ///             "Limit": { "params": ["max"], "body": "maximum: {{max}}" }
    ///         },
    ///         "x-oas-forge-format": 1
    ///     })
    /// );
    /// ```
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
