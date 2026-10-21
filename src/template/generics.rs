//! Generic instantiation of blueprints (`$Page<User>` → `Page_User`).
//!
//! Generic references are whole string scalars. Each distinct instantiation
//! produces one concrete schema; instantiated bodies are processed again until
//! no generic reference is left (bounded by [`MAX_DEPTH`]).

use super::aliases::{self, Found, Lookup};
use super::refs::{KnownSchemas, reference_name};
use super::registry::Registry;
use crate::diagnostics::{Code, Diagnostic, Diagnostics, Location};
use crate::extract::AliasItem;
use crate::schema::shadow;
use crate::schema::types::{MappedType, TypeExpr, TypeMapper, is_container};
use regex::{Captures, Regex};
use serde_json::{Map, Value};
use std::collections::BTreeMap;
use std::sync::LazyLock;

/// Maximum nesting of instantiations (`$A<$B<$C<…>>>`).
pub const MAX_DEPTH: usize = 16;

/// Keywords whose value is a schema (also of media types and parameters).
const SCHEMA_KEYS: &[&str] = &[
    "items",
    "not",
    "additionalProperties",
    "unevaluatedProperties",
    "unevaluatedItems",
    "contains",
    "propertyNames",
    "if",
    "then",
    "else",
    "contentSchema",
    "schema",
    "itemSchema",
];

/// Keywords whose value is a list of schemas.
const SCHEMA_LIST_KEYS: &[&str] = &["allOf", "anyOf", "oneOf", "prefixItems"];

/// Keywords whose value maps names to schemas.
const SCHEMA_MAP_KEYS: &[&str] = &[
    "properties",
    "patternProperties",
    "$defs",
    "dependentSchemas",
];

/// Keywords whose value is data, not schemas.
const DATA_KEYS: &[&str] = &["example", "examples", "default", "enum", "const", "value"];

/// Turns each whole `$Name` (or `$Page<User>`) string of `map` that stands
/// where a schema is expected into `{"$ref": "$Name"}`.
fn wrap_references(map: &mut Map<String, Value>) {
    let wrap = |value: &mut Value| {
        if let Value::String(text) = value
            && text.starts_with('$')
            && (reference_name(text).is_some() || GenericRef::parse(text).is_some())
        {
            let mut reference = Map::new();
            reference.insert("$ref".into(), Value::String(std::mem::take(text)));
            *value = Value::Object(reference);
        }
    };
    for (key, value) in map.iter_mut() {
        let key = key.as_str();
        if SCHEMA_KEYS.contains(&key) {
            wrap(value);
        } else if SCHEMA_LIST_KEYS.contains(&key)
            && let Value::Array(items) = value
        {
            items.iter_mut().for_each(wrap);
        } else if SCHEMA_MAP_KEYS.contains(&key)
            && let Value::Object(schemas) = value
        {
            schemas.values_mut().for_each(wrap);
        }
    }
}

/// A parsed generic reference.
///
/// # Examples
///
/// ```
/// use oas_forge::template::generics::{GenericArg, GenericRef};
///
/// assert_eq!(
///     GenericRef::parse("$Page<$Pair<User, Order>>"),
///     Some(GenericRef {
///         name: "Page".into(),
///         args: vec![GenericArg::Generic(GenericRef {
///             name: "Pair".into(),
///             args: vec![
///                 GenericArg::Name("User".into()),
///                 GenericArg::Name("Order".into()),
///             ],
///         })],
///     })
/// );
/// ```
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GenericRef {
    /// The blueprint name.
    pub name: String,
    /// The arguments.
    pub args: Vec<GenericArg>,
}

/// One argument of a generic reference.
///
/// # Examples
///
/// ```
/// use oas_forge::template::generics::{GenericArg, GenericRef};
///
/// let generic = GenericRef::parse("$Map<String, $Page<User>>").ok_or("not generic")?;
/// assert_eq!(generic.args[0], GenericArg::Name("String".into()));
/// assert!(matches!(&generic.args[1], GenericArg::Generic(inner) if inner.name == "Page"));
/// # Ok::<(), Box<dyn std::error::Error>>(())
/// ```
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GenericArg {
    /// A plain type name (`User`).
    Name(String),
    /// A nested generic reference (`$Page<User>`).
    Generic(GenericRef),
}

impl GenericRef {
    /// Parses `$Name<Arg, $Other<X>>` (the `$` of arguments is optional).
    /// `None` unless the whole text, apart from surrounding whitespace, is
    /// one generic reference.
    ///
    /// # Examples
    ///
    /// ```
    /// use oas_forge::template::generics::{GenericArg, GenericRef};
    ///
    /// assert_eq!(
    ///     GenericRef::parse("$Page<User>"),
    ///     Some(GenericRef {
    ///         name: "Page".into(),
    ///         args: vec![GenericArg::Name("User".into())],
    ///     })
    /// );
    /// assert_eq!(GenericRef::parse("Page<User>"), None);
    /// assert_eq!(GenericRef::parse("$Page<User> or more"), None);
    /// ```
    pub fn parse(text: &str) -> Option<Self> {
        let text = text.trim().strip_prefix('$')?;
        let (generic, rest) = parse_ref(text)?;
        rest.trim().is_empty().then_some(generic)
    }

    /// The concrete schema name (`Page_User`, `Map_String_Pair_A_B`): only
    /// the last segment of a path is used, and a reference without
    /// arguments gets the suffix `_Generic`.
    ///
    /// # Examples
    ///
    /// ```
    /// use oas_forge::template::generics::GenericRef;
    ///
    /// let generic = GenericRef::parse("$Map<String, $Pair<models::A, B>>").ok_or("not generic")?;
    /// assert_eq!(generic.concrete_name(), "Map_String_Pair_A_B");
    /// # Ok::<(), Box<dyn std::error::Error>>(())
    /// ```
    pub fn concrete_name(&self) -> String {
        let suffix = if self.args.is_empty() {
            "Generic".to_string()
        } else {
            self.args
                .iter()
                .map(|arg| match arg {
                    GenericArg::Name(name) => last_segment(name).to_string(),
                    GenericArg::Generic(inner) => inner.concrete_name(),
                })
                .collect::<Vec<_>>()
                .join("_")
        };
        format!("{}_{suffix}", last_segment(&self.name))
    }

    /// The reference as type text without `$` (`Vec<User>`); paths are
    /// shortened to their last segment.
    ///
    /// # Examples
    ///
    /// ```
    /// use oas_forge::template::generics::GenericRef;
    ///
    /// let generic = GenericRef::parse("$Page<$Vec<models::User>>").ok_or("not generic")?;
    /// assert_eq!(generic.text(), "Page<Vec<User>>");
    /// # Ok::<(), Box<dyn std::error::Error>>(())
    /// ```
    pub fn text(&self) -> String {
        let args: Vec<String> = self
            .args
            .iter()
            .map(|arg| match arg {
                GenericArg::Name(name) => last_segment(name).to_string(),
                GenericArg::Generic(inner) => inner.text(),
            })
            .collect();
        format!("{}<{}>", last_segment(&self.name), args.join(", "))
    }
}

/// `User` for `models::User`.
fn last_segment(name: &str) -> &str {
    name.rsplit("::").next().unwrap_or(name)
}

/// Whether `c` can be part of a (path-qualified) type name or of a
/// qualified schema name (`crate.module.Name`).
fn is_name_char(c: char) -> bool {
    c.is_alphanumeric() || c == '_' || c == ':' || c == '.'
}

/// Splits `text` after its leading name characters.
fn split_name(text: &str) -> (&str, &str) {
    let end = text.find(|c: char| !is_name_char(c)).unwrap_or(text.len());
    text.split_at_checked(end).unwrap_or((text, ""))
}

/// Parses `Name<args>` at the start of `text`; returns the rest.
fn parse_ref(text: &str) -> Option<(GenericRef, &str)> {
    let (name, rest) = split_name(text);
    if name.is_empty() {
        return None;
    }
    let rest = rest.trim_start().strip_prefix('<')?;
    let mut args = Vec::new();
    let mut rest = rest;
    loop {
        rest = rest.trim_start();
        if let Some(after) = rest.strip_prefix('>') {
            return Some((
                GenericRef {
                    name: name.to_string(),
                    args,
                },
                after,
            ));
        }
        let (arg, after) = parse_arg(rest)?;
        args.push(arg);
        rest = after.trim_start();
        if let Some(after) = rest.strip_prefix(',') {
            rest = after;
        } else if !rest.starts_with('>') {
            return None;
        }
    }
}

/// Parses one argument at the start of `text`, a name or a nested generic
/// reference with an optional `$`; returns the rest.
fn parse_arg(text: &str) -> Option<(GenericArg, &str)> {
    let text = text.strip_prefix('$').unwrap_or(text);
    let (name, rest) = split_name(text);
    if name.is_empty() {
        return None;
    }
    if rest.trim_start().starts_with('<') {
        let (generic, rest) = parse_ref(text)?;
        Some((GenericArg::Generic(generic), rest))
    } else {
        Some((GenericArg::Name(name.to_string()), rest))
    }
}

/// Instantiates blueprints referenced by whole string scalars.
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
///     "Page",
///     Blueprint {
///         params: vec!["T".into()],
///         schema: json!({
///             "type": "object",
///             "properties": { "items": { "type": "array", "items": { "$ref": "$T" } } }
///         }),
///     },
/// );
/// // Generic references stay raw until this stage.
/// let mut value = json!({ "$ref": "$Page<User>" });
/// let mut diagnostics = Diagnostics::new();
/// let mut instantiator = Instantiator::new(&registry);
/// instantiator.apply(&mut value, &Location::new("src/lib.rs", 12), &mut diagnostics);
///
/// // Smart references to the concrete schema and to `User` remain.
/// assert_eq!(value, json!({ "$ref": "$Page_User" }));
/// let concrete = instantiator.into_concrete();
/// assert_eq!(
///     concrete["Page_User"].0,
///     json!({
///         "type": "object",
///         "properties": { "items": { "type": "array", "items": { "$ref": "$User" } } }
///     })
/// );
/// assert!(diagnostics.is_empty());
/// ```
pub struct Instantiator<'a> {
    /// The blueprints to instantiate.
    registry: &'a Registry,
    /// The type table for std containers and other mapped types
    /// (`$Vec<User>`), which are inlined instead of instantiated.
    types: &'a TypeMapper,
    /// The concrete schemas created so far, by name, with the location of
    /// their first use.
    concrete: BTreeMap<String, (Value, Location)>,
    /// The concrete names being instantiated, outermost first; a recursive
    /// use of one of them is not instantiated again.
    in_progress: Vec<String>,
    /// The documented schema names; a type alias of another file does not
    /// replace a reference to one of them.
    known: &'a KnownSchemas,
    /// The type aliases being replaced, outermost first.
    aliasing: Vec<String>,
    /// Whether the value being applied is data (`example`, `enum`, …),
    /// where `$Name` is no schema.
    in_data: bool,
}

impl<'a> Instantiator<'a> {
    /// An instantiator using the blueprints of `registry` and the built-in
    /// type table.
    ///
    /// # Examples
    ///
    /// ```
    /// use oas_forge::diagnostics::{Diagnostics, Location};
    /// use oas_forge::template::Registry;
    /// use oas_forge::template::generics::Instantiator;
    /// use serde_json::json;
    ///
    /// let registry = Registry::new();
    /// let mut instantiator = Instantiator::new(&registry);
    /// // `Vec` is no blueprint: the built-in table makes it an inline array.
    /// let mut value = json!({ "$ref": "$Vec<User>" });
    /// let mut diagnostics = Diagnostics::new();
    /// instantiator.apply(&mut value, &Location::new("src/lib.rs", 1), &mut diagnostics);
    /// assert_eq!(value, json!({ "type": "array", "items": { "$ref": "$User" } }));
    /// ```
    pub fn new(registry: &'a Registry) -> Self {
        Self {
            registry,
            types: TypeMapper::builtin(),
            concrete: BTreeMap::new(),
            in_progress: Vec::new(),
            known: KnownSchemas::none(),
            aliasing: Vec::new(),
            in_data: false,
        }
    }

    /// Uses `known` as the documented schema names: a type alias of another
    /// file does not replace a reference to one of them.
    pub(crate) fn with_known(mut self, known: &'a KnownSchemas) -> Self {
        self.known = known;
        self
    }

    /// Uses `types` for type arguments such as `Vec<User>` or `Option<u8>`.
    ///
    /// # Examples
    ///
    /// ```
    /// use oas_forge::diagnostics::{Diagnostics, Location};
    /// use oas_forge::schema::TypeMapper;
    /// use oas_forge::template::Registry;
    /// use oas_forge::template::generics::Instantiator;
    /// use serde_json::json;
    ///
    /// let types = TypeMapper::new().with_type("Money", json!({ "type": "string" }));
    /// let registry = Registry::new();
    /// let mut instantiator = Instantiator::new(&registry).with_types(&types);
    /// let mut value = json!({ "$ref": "$Vec<Money>" });
    /// let mut diagnostics = Diagnostics::new();
    /// instantiator.apply(&mut value, &Location::new("src/lib.rs", 1), &mut diagnostics);
    /// assert_eq!(value, json!({ "type": "array", "items": { "type": "string" } }));
    /// ```
    pub fn with_types(mut self, types: &'a TypeMapper) -> Self {
        self.types = types;
        self
    }

    /// Replaces generic references in `value`; reports problems at `location`.
    ///
    /// A whole `$Name` or `$Page<User>` string where a schema is expected
    /// (in `items`, `oneOf`, `properties`, a media type's `schema`, …; not in
    /// data such as `example` or `enum`, nor in extensions) becomes
    /// `{"$ref": …}` first.
    /// A `$ref` to a std container (`$Vec<User>`) is replaced by the inline
    /// schema and keeps its sibling keywords. A reference to an unknown
    /// blueprint becomes the concrete name, so a schema written by hand can
    /// satisfy it, and is reported as [`Code::UnknownBlueprint`].
    ///
    /// # Examples
    ///
    /// ```
    /// use oas_forge::diagnostics::{Code, Diagnostics, Location};
    /// use oas_forge::template::Registry;
    /// use oas_forge::template::generics::Instantiator;
    /// use serde_json::json;
    ///
    /// let registry = Registry::new();
    /// let mut instantiator = Instantiator::new(&registry);
    /// let mut value = json!({
    ///     "users": { "$ref": "$Vec<User>", "description": "All users" },
    ///     "page": { "$ref": "$Missing<User>" }
    /// });
    /// let mut diagnostics = Diagnostics::new();
    /// instantiator.apply(&mut value, &Location::new("src/lib.rs", 7), &mut diagnostics);
    ///
    /// assert_eq!(
    ///     value,
    ///     json!({
    ///         "users": {
    ///             "type": "array",
    ///             "items": { "$ref": "$User" },
    ///             "description": "All users"
    ///         },
    ///         "page": { "$ref": "$Missing_User" }
    ///     })
    /// );
    /// let finding = diagnostics.iter().next().ok_or("no finding")?;
    /// assert_eq!(finding.code(), Code::UnknownBlueprint);
    /// assert_eq!(finding.location(), Some(&Location::new("src/lib.rs", 7)));
    /// # Ok::<(), Box<dyn std::error::Error>>(())
    /// ```
    pub fn apply(&mut self, value: &mut Value, location: &Location, diagnostics: &mut Diagnostics) {
        match value {
            Value::String(s) if s.starts_with('$') && s.contains('<') => {
                if let Some(generic) = GenericRef::parse(s)
                    && let Some(replacement) = self.replace(s, &generic, location, diagnostics)
                {
                    *value = replacement;
                }
            }
            Value::Array(items) => {
                for item in items {
                    self.apply(item, location, diagnostics);
                }
            }
            Value::Object(map) => {
                if !self.in_data {
                    wrap_references(map);
                }
                // A type alias stands for its target (`$Owner`, `$Paged<User>`),
                // resolved in the alias's file.
                if let Some((alias, schema)) = self.alias(map, location, diagnostics) {
                    aliases::replace(map, schema);
                    self.aliasing.push(alias.rust_name.clone());
                    self.apply(value, &alias.location, diagnostics);
                    self.aliasing.pop();
                    return;
                }
                // `{$ref: $Vec<User>}`: the shorthand replaces the whole schema;
                // sibling keywords such as `description` are kept.
                let inline = map.get("$ref").and_then(Value::as_str).and_then(|r| {
                    let generic = GenericRef::parse(r)?;
                    self.std_container(r, &generic, location, diagnostics)
                });
                if let Some(Value::Object(mut schema)) = inline {
                    map.remove("$ref");
                    for (key, mut child) in std::mem::take(map) {
                        self.apply_child(&key, &mut child, location, diagnostics);
                        schema.insert(key, child);
                    }
                    *map = schema;
                    return;
                }
                for (key, child) in map.iter_mut() {
                    self.apply_child(key, child, location, diagnostics);
                }
            }
            _ => {}
        }
    }

    /// [`Self::apply`] to the value of `key`; below a data keyword
    /// (`example`, `enum`, …) or an extension (`x-…`) no `$Name` is a
    /// schema.
    fn apply_child(
        &mut self,
        key: &str,
        child: &mut Value,
        location: &Location,
        diagnostics: &mut Diagnostics,
    ) {
        let outer = self.in_data;
        self.in_data |= DATA_KEYS.contains(&key) || key.starts_with("x-");
        self.apply(child, location, diagnostics);
        self.in_data = outer;
    }

    /// The type alias that the `$ref` of `map` names (`$Owner`,
    /// `$Paged<User>`) and its schema with the type arguments bound.
    fn alias(
        &self,
        map: &Map<String, Value>,
        location: &Location,
        diagnostics: &mut Diagnostics,
    ) -> Option<(&'a AliasItem, Value)> {
        let text = map.get("$ref")?.as_str()?;
        let (name, args) = match GenericRef::parse(text) {
            Some(generic) => (
                last_segment(&generic.name).to_string(),
                generic
                    .args
                    .iter()
                    .map(|arg| match arg {
                        GenericArg::Name(name) => last_segment(name).to_string(),
                        GenericArg::Generic(inner) => inner.text(),
                    })
                    .collect(),
            ),
            None => (reference_name(text)?.to_string(), Vec::new()),
        };
        let lookup = Lookup {
            registry: self.registry,
            known: self.known,
        };
        let Found::Alias(alias) =
            lookup.find(&name, args.len(), location, &self.aliasing, diagnostics)
        else {
            return None;
        };
        let bindings: BTreeMap<&str, &str> = alias
            .params
            .iter()
            .map(String::as_str)
            .zip(args.iter().map(String::as_str))
            .collect();
        Some((alias, substitute(&alias.schema, &bindings)))
    }

    /// The inline schema of a `$Vec<T>`-style shorthand: std containers and
    /// other types of the type table that are not shadowed by a blueprint of
    /// the same name.
    fn std_container(
        &mut self,
        text: &str,
        generic: &GenericRef,
        location: &Location,
        diagnostics: &mut Diagnostics,
    ) -> Option<Value> {
        if self
            .registry
            .blueprint(last_segment(&generic.name))
            .is_some()
        {
            return None;
        }
        let ty = TypeExpr::parse(text)?;
        let mapped = self.types.map(&ty);
        if !is_container(last_segment(&generic.name)) && mapped.schema.get("$ref").is_some() {
            return None;
        }
        // A bare built-in name (`$Vec<Duration>`) may stand for a documented
        // schema.
        let mut schema = shadow::typed(self.types, &ty, MappedType::into_schema);
        self.apply(&mut schema, location, diagnostics);
        Some(schema)
    }

    /// The concrete schemas created so far, by name, with the location of
    /// their first use.
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
    ///     "Page",
    ///     Blueprint { params: vec!["T".into()], schema: json!({ "items": { "$ref": "$T" } }) },
    /// );
    /// let mut instantiator = Instantiator::new(&registry);
    /// let mut diagnostics = Diagnostics::new();
    /// let mut users = json!({ "$ref": "$Page<User>" });
    /// instantiator.apply(&mut users, &Location::new("src/users.rs", 10), &mut diagnostics);
    /// let mut nested = json!({ "$ref": "$Page<$Page<User>>" });
    /// instantiator.apply(&mut nested, &Location::new("src/orders.rs", 20), &mut diagnostics);
    ///
    /// let concrete = instantiator.into_concrete();
    /// let names: Vec<&str> = concrete.keys().map(String::as_str).collect();
    /// assert_eq!(names, ["Page_Page_User", "Page_User"]);
    /// assert_eq!(
    ///     concrete["Page_Page_User"].0,
    ///     json!({ "items": { "$ref": "$Page_User" } })
    /// );
    /// assert_eq!(concrete["Page_User"].1, Location::new("src/users.rs", 10));
    /// ```
    pub fn into_concrete(self) -> BTreeMap<String, (Value, Location)> {
        self.concrete
    }

    /// The replacement of the generic reference `text`: the inline schema of
    /// a std container, else a smart reference (`$Page_User`) to the
    /// concrete schema. `None` keeps `text` (instantiation failed).
    fn replace(
        &mut self,
        text: &str,
        generic: &GenericRef,
        location: &Location,
        diagnostics: &mut Diagnostics,
    ) -> Option<Value> {
        // `$Vec<T>` and other std containers are inline arrays, not templates.
        if let Some(schema) = self.std_container(text, generic, location, diagnostics) {
            return Some(schema);
        }
        self.instantiate(generic, location, diagnostics)
            .map(|name| Value::String(format!("${name}")))
    }

    /// Creates the concrete schema for `generic`; returns its name.
    fn instantiate(
        &mut self,
        generic: &GenericRef,
        location: &Location,
        diagnostics: &mut Diagnostics,
    ) -> Option<String> {
        let generic = &self.unaliased(generic, location);
        let Some(blueprint) = self.registry.blueprint(last_segment(&generic.name)) else {
            diagnostics.push(
                Diagnostic::new(
                    Code::UnknownBlueprint,
                    format!(
                        "`${}<…>`: no blueprint named `{}`",
                        generic.name, generic.name
                    ),
                )
                .at(location.clone()),
            );
            // The concrete name keeps the document valid and lets a schema
            // written by hand (`Missing_User`) satisfy the reference.
            return Some(generic.concrete_name());
        };
        // Arguments that are blueprints become concrete schemas; other
        // generic types (`Vec<User>`) are substituted as written and resolved
        // in the instantiated body.
        let mut arg_names = Vec::new();
        for arg in &generic.args {
            arg_names.push(match arg {
                GenericArg::Name(name) => last_segment(name).to_string(),
                GenericArg::Generic(inner)
                    if self.registry.blueprint(last_segment(&inner.name)).is_some() =>
                {
                    self.instantiate(inner, location, diagnostics)?
                }
                GenericArg::Generic(inner) => inner.text(),
            });
        }
        let name = generic.concrete_name();
        if self.concrete.contains_key(&name) || self.in_progress.contains(&name) {
            return Some(name);
        }
        if self.in_progress.len() >= MAX_DEPTH {
            diagnostics.push(
                Diagnostic::new(
                    Code::InstantiationDepth,
                    format!("instantiating `{name}` exceeds the nesting limit of {MAX_DEPTH}"),
                )
                .at(location.clone()),
            );
            return None;
        }
        if arg_names.len() != blueprint.params.len() {
            diagnostics.push(
                Diagnostic::new(
                    Code::BlueprintArity,
                    format!(
                        "blueprint `{}` expects {} argument(s), got {}",
                        generic.name,
                        blueprint.params.len(),
                        arg_names.len()
                    ),
                )
                .at(location.clone()),
            );
        }
        let bindings: BTreeMap<&str, &str> = blueprint
            .params
            .iter()
            .map(String::as_str)
            .zip(arg_names.iter().map(String::as_str))
            .collect();
        let mut body = substitute(&blueprint.schema, &bindings);

        self.in_progress.push(name.clone());
        self.apply(&mut body, location, diagnostics);
        self.in_progress.pop();
        self.concrete.insert(name.clone(), (body, location.clone()));
        Some(name)
    }

    /// `generic` with every argument that names a transparent type alias
    /// replaced by the alias's target, also inside nested arguments, so that
    /// `Page<Owner>` (`type Owner = User;`) is the instance `Page_User`. An
    /// argument whose alias cannot replace it (ambiguous, a cycle) stays; it
    /// is reported where its schema is resolved.
    fn unaliased(&self, generic: &GenericRef, location: &Location) -> GenericRef {
        let mut expanding = self.aliasing.clone();
        GenericRef {
            name: generic.name.clone(),
            args: generic
                .args
                .iter()
                .map(|arg| self.unalias(arg, location, &mut expanding))
                .collect(),
        }
    }

    /// `arg` with transparent aliases replaced by their targets (see
    /// [`Self::unaliased`]); names in a target are looked up at the alias.
    /// `expanding` holds the aliases being replaced, so that a cycle stops
    /// with the name as written.
    fn unalias(
        &self,
        arg: &GenericArg,
        location: &Location,
        expanding: &mut Vec<String>,
    ) -> GenericArg {
        let (name, args): (&str, Vec<GenericArg>) = match arg {
            GenericArg::Name(name) => (last_segment(name), Vec::new()),
            GenericArg::Generic(inner) => (
                last_segment(&inner.name),
                inner
                    .args
                    .iter()
                    .map(|arg| self.unalias(arg, location, expanding))
                    .collect(),
            ),
        };
        let lookup = Lookup {
            registry: self.registry,
            known: self.known,
        };
        // The canonical findings come from resolving the schemas.
        let found = lookup.find(
            name,
            args.len(),
            location,
            expanding,
            &mut Diagnostics::new(),
        );
        if let Found::Alias(alias) = found
            && let Some(target) = bound_target(alias, &args)
        {
            expanding.push(alias.rust_name.clone());
            let target = self.unalias(&target, &alias.location, expanding);
            expanding.pop();
            return target;
        }
        match arg {
            GenericArg::Name(_) => arg.clone(),
            GenericArg::Generic(inner) => GenericArg::Generic(GenericRef {
                name: inner.name.clone(),
                args,
            }),
        }
    }
}

/// The target of `alias` with its type parameters bound to `args`, as a
/// generic argument; `None` when the target is no named type (a tuple, an
/// array).
fn bound_target(alias: &AliasItem, args: &[GenericArg]) -> Option<GenericArg> {
    let texts: Vec<String> = args
        .iter()
        .map(|arg| match arg {
            GenericArg::Name(name) => name.clone(),
            GenericArg::Generic(inner) => inner.text(),
        })
        .collect();
    let bindings: BTreeMap<&str, &str> = alias
        .params
        .iter()
        .map(String::as_str)
        .zip(texts.iter().map(String::as_str))
        .collect();
    let Value::String(target) = substitute(&Value::String(alias.target.clone()), &bindings) else {
        return None;
    };
    let (arg, rest) = parse_arg(&target)?;
    rest.trim().is_empty().then_some(arg)
}

/// A `$Param` identifier in a blueprint string; captures the name.
static PLACEHOLDER_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"\$(\w+)").expect("constant pattern"));

/// Replaces whole `$Param` identifiers in every string (and key) of `schema`
/// in a single pass, so substituted names are never substituted again. In a
/// whole generic reference (`$Envelope<T>`), bare parameter names are bound
/// as well.
pub(crate) fn substitute(schema: &Value, bindings: &BTreeMap<&str, &str>) -> Value {
    let replace = |text: &str| -> String {
        PLACEHOLDER_RE
            .replace_all(text, |caps: &Captures| match bindings.get(&caps[1]) {
                Some(arg) => format!("${arg}"),
                None => caps[0].to_string(),
            })
            .into_owned()
    };
    match schema {
        // A whole generic reference: parameters are bound with or without
        // `$` (reflection writes `$Envelope<T>` for a field `Envelope<T>`).
        Value::String(s) => Value::String(match GenericRef::parse(s) {
            Some(generic) => format!("${}", rebind(&generic, bindings)),
            None => replace(s),
        }),
        Value::Array(items) => {
            Value::Array(items.iter().map(|i| substitute(i, bindings)).collect())
        }
        Value::Object(map) => Value::Object(
            map.iter()
                .map(|(k, v)| (replace(k), substitute(v, bindings)))
                .collect::<Map<_, _>>(),
        ),
        other => other.clone(),
    }
}

/// `generic` as type text without the leading `$`, with every argument that
/// names a parameter replaced by its binding (`Envelope<T>` → `Envelope<User>`).
fn rebind(generic: &GenericRef, bindings: &BTreeMap<&str, &str>) -> String {
    let args: Vec<String> = generic
        .args
        .iter()
        .map(|arg| match arg {
            GenericArg::Name(name) => bindings
                .get(name.as_str())
                .map_or_else(|| name.clone(), |bound| (*bound).to_string()),
            GenericArg::Generic(inner) => rebind(inner, bindings),
        })
        .collect();
    format!("{}<{}>", generic.name, args.join(", "))
}
