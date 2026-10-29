//! Smart references: whole string scalars `$Name` that name a known schema
//! become `#/components/schemas/<name>` (ADR 0001). A `{"$ref": "$Name"}`
//! that names a built-in type (`$String`, `$Uuid`, e.g. from a blueprint
//! instantiated with `$Page<String>`) becomes that type's schema.

use super::aliases::{self, Found, Lookup};
use super::generics::{MAX_INSTANTIATED_SIZE, alias_target};
use super::lexical::ESCAPED_DOLLAR;
use super::registry::Registry;
use crate::diagnostics::{Code, Diagnostic, Diagnostics, Location};
use crate::schema::{TypeExpr, TypeMapper, shadow};
use serde_json::Value;
use std::collections::{BTreeMap, BTreeSet, HashSet};

/// Marks the reference of a field whose value is the `Default` of its type
/// (`#[serde(default)]`): the references stage copies the `default` of the
/// referenced schema next to it, then removes the marker.
pub(crate) const TYPE_DEFAULT_KEY: &str = "x-oas-forge-type-default";

/// Marks the reference of a Rust type name that does not identify one
/// documented type (ambiguous, or an undocumented type of its module); the
/// value is the finding. The references stage reports it and leaves the
/// schema without the reference.
pub(crate) const UNRESOLVED_KEY: &str = "x-oas-forge-unresolved";

/// The schema names smart references may resolve to.
///
/// # Examples
///
/// ```
/// use oas_forge::template::refs::KnownSchemas;
/// use serde_json::json;
///
/// let mut known = KnownSchemas::default();
/// known.collect_from(&json!({ "components": { "schemas": { "User": {} } } }));
/// known.insert("Page_User");
/// assert!(known.contains("User") && known.contains("Page_User"));
/// ```
#[derive(Debug, Clone, Default)]
pub struct KnownSchemas {
    /// The names, sorted.
    names: BTreeSet<String>,
}

impl KnownSchemas {
    /// The empty set, for stages run without documented names.
    pub(crate) fn none() -> &'static Self {
        /// The shared empty set.
        static NONE: KnownSchemas = KnownSchemas {
            names: BTreeSet::new(),
        };
        &NONE
    }

    /// Adds a schema name.
    ///
    /// # Examples
    ///
    /// ```
    /// use oas_forge::template::refs::KnownSchemas;
    ///
    /// let mut known = KnownSchemas::default();
    /// known.insert("User");
    /// assert!(known.contains("User"));
    /// ```
    pub fn insert(&mut self, name: impl Into<String>) {
        self.names.insert(name.into());
    }

    /// Whether `name` is known.
    ///
    /// # Examples
    ///
    /// ```
    /// use oas_forge::template::refs::KnownSchemas;
    ///
    /// let mut known = KnownSchemas::default();
    /// known.insert("User");
    /// assert!(known.contains("User"));
    /// assert!(!known.contains("user"));
    /// ```
    pub fn contains(&self, name: &str) -> bool {
        self.names.contains(name)
    }

    /// Every `components.schemas` name defined in `document`.
    ///
    /// # Examples
    ///
    /// ```
    /// use oas_forge::template::refs::KnownSchemas;
    /// use serde_json::json;
    ///
    /// let mut known = KnownSchemas::default();
    /// known.collect_from(&json!({
    ///     "paths": {},
    ///     "components": { "schemas": { "User": {}, "Team": {} } }
    /// }));
    /// assert!(known.contains("Team"));
    /// assert!(!known.contains("paths"));
    /// ```
    pub fn collect_from(&mut self, document: &Value) {
        if let Some(schemas) = document
            .pointer("/components/schemas")
            .and_then(Value::as_object)
        {
            self.names.extend(schemas.keys().cloned());
        }
    }
}

/// The name of a smart reference: `text` is `$` followed by identifiers
/// (a letter or `_`, then letters, digits or `_`) joined by `.` (the
/// qualified name of a type whose name collides, `crate.module.Name`).
/// `None` for other text, e.g. a generic reference (`$Page<User>`) or prose.
pub(crate) fn reference_name(text: &str) -> Option<&str> {
    let name = text.strip_prefix('$')?;
    name.split('.')
        .all(|part| {
            let mut chars = part.chars();
            chars
                .next()
                .is_some_and(|first| first.is_alphabetic() || first == '_')
                && chars.all(|c| c.is_alphanumeric() || c == '_')
        })
        .then_some(name)
}

/// What smart references may resolve to.
///
/// # Examples
///
/// ```
/// use oas_forge::diagnostics::{Diagnostics, Location};
/// use oas_forge::schema::TypeMapper;
/// use oas_forge::template::Registry;
/// use oas_forge::template::refs::{self, KnownSchemas, Targets};
/// use serde_json::json;
///
/// let mut known = KnownSchemas::default();
/// known.insert("User");
/// let mut registry = Registry::new();
/// // `#[serde(rename = "User")] struct UserDto`
/// registry.add_alias("UserDto", "User");
/// let targets = Targets { known: &known, registry: &registry, types: TypeMapper::builtin() };
///
/// let mut value = json!({ "$ref": "$UserDto" });
/// let mut diagnostics = Diagnostics::new();
/// refs::resolve(&mut value, targets, &Location::new("src/lib.rs", 1), &mut diagnostics);
/// assert_eq!(value, json!({ "$ref": "#/components/schemas/User" }));
/// ```
#[derive(Debug, Clone, Copy)]
pub struct Targets<'a> {
    /// The documented schema names.
    pub known: &'a KnownSchemas,
    /// Aliases from Rust names to schema names.
    pub registry: &'a Registry,
    /// The type table for built-in types.
    pub types: &'a TypeMapper,
}

/// Resolves smart references in `value`; unknown names are reported once.
///
/// Every whole string `$Name` that names a known schema, directly or by the
/// Rust name of a renamed type, becomes a pointer to that schema. A `{"$ref": "$Name"}` of a
/// built-in type is replaced by its schema; one of an unknown name is
/// reported and still becomes a pointer, so the lints see it.
///
/// # Examples
///
/// ```
/// use oas_forge::diagnostics::{Code, Diagnostics, Location};
/// use oas_forge::schema::TypeMapper;
/// use oas_forge::template::Registry;
/// use oas_forge::template::refs::{self, KnownSchemas, Targets};
/// use serde_json::json;
///
/// let mut known = KnownSchemas::default();
/// known.insert("User");
/// let registry = Registry::new();
/// let targets = Targets { known: &known, registry: &registry, types: TypeMapper::builtin() };
///
/// let mut value = json!({
///     "properties": {
///         "owner": { "$ref": "$User" },
///         "id": { "$ref": "$Uuid" },
///         "group": { "$ref": "$Group" }
///     }
/// });
/// let mut diagnostics = Diagnostics::new();
/// refs::resolve(&mut value, targets, &Location::new("src/lib.rs", 4), &mut diagnostics);
///
/// assert_eq!(
///     value,
///     json!({
///         "properties": {
///             "owner": { "$ref": "#/components/schemas/User" },
///             "id": { "type": "string", "format": "uuid" },
///             "group": { "$ref": "#/components/schemas/Group" }
///         }
///     })
/// );
/// let codes: Vec<Code> = diagnostics.iter().map(|d| d.code()).collect();
/// assert_eq!(codes, [Code::UnresolvedReference]);
/// ```
pub fn resolve(
    value: &mut Value,
    targets: Targets<'_>,
    location: &Location,
    diagnostics: &mut Diagnostics,
) {
    resolve_within(
        value,
        targets,
        location,
        diagnostics,
        &mut AliasBudget::new(),
    );
}

/// [`resolve`] with a budget for the targets of type aliases that the
/// generator shares between its calls.
pub(crate) fn resolve_within(
    value: &mut Value,
    targets: Targets<'_>,
    location: &Location,
    diagnostics: &mut Diagnostics,
    budget: &mut AliasBudget,
) {
    let mut reported = HashSet::new();
    let mut state = State {
        diagnostics,
        reported: &mut reported,
        expanding: Vec::new(),
        budget,
    };
    resolve_at(value, targets, location, &mut state);
}

/// What is left of [`MAX_INSTANTIATED_SIZE`] for the targets of replaced
/// type aliases: a target that names the next alias twice doubles per link.
#[derive(Debug)]
pub(crate) struct AliasBudget {
    /// The size left.
    left: usize,
    /// Whether running out was reported.
    reported: bool,
    /// The alias findings reported so far, each once.
    findings: BTreeSet<String>,
}

impl AliasBudget {
    /// The whole budget.
    pub(crate) fn new() -> Self {
        Self::after((MAX_INSTANTIATED_SIZE, false, BTreeSet::new()))
    }

    /// The budget `left` after an earlier stage, which `reported` running
    /// out already and reported the alias `findings`.
    pub(crate) fn after((left, reported, findings): (usize, bool, BTreeSet<String>)) -> Self {
        Self {
            left,
            reported,
            findings,
        }
    }

    /// The target `schema` of the alias `name`, if it fits; otherwise
    /// reports once at `location` and uses the budget up.
    fn take(
        &mut self,
        name: &str,
        schema: &Value,
        location: &Location,
        diagnostics: &mut Diagnostics,
    ) -> Option<Value> {
        let mut left = self.left;
        if let Some(target) = alias_target(schema, &BTreeMap::new(), &mut left) {
            self.left = left;
            return Some(target);
        }
        self.left = 0;
        if !self.reported {
            self.reported = true;
            diagnostics.push(
                Diagnostic::new(
                    Code::InstantiationLimit,
                    format!(
                        "type alias `{name}` exceeds the total size of {MAX_INSTANTIATED_SIZE} bytes for concrete schemas and type alias targets; it and later aliases are not replaced"
                    ),
                )
                .at(location.clone()),
            );
        }
        None
    }
}

/// The mutable state of [`resolve_at`].
struct State<'d> {
    /// Where findings go.
    diagnostics: &'d mut Diagnostics,
    /// The unknown names reported so far.
    reported: &'d mut HashSet<String>,
    /// The type aliases being replaced around the value.
    expanding: Vec<String>,
    /// The budget for alias targets.
    budget: &'d mut AliasBudget,
}

/// The schema of a built-in or custom-mapped type named by
/// `{"$ref": "$Name"}`. A type the table does not know maps to the smart
/// reference `$Name` itself, which is no answer.
fn builtin(name: &str, targets: Targets<'_>) -> Option<serde_json::Map<String, Value>> {
    if targets.known.contains(name) || targets.registry.alias(name).is_some() {
        return None;
    }
    let schema = targets.types.map(&TypeExpr::parse(name)?).into_schema();
    match schema {
        Value::Object(map)
            if !map
                .get("$ref")
                .and_then(Value::as_str)
                .is_some_and(|target| target.starts_with('$')) =>
        {
            Some(map)
        }
        _ => None,
    }
}

/// [`resolve`] at `value` and below, with the findings, reported names,
/// aliases being replaced and the alias budget in `state`. In a mapping,
/// the built-in alternatives are chosen first ([`shadow::choose`]), then a
/// `$ref` of a type alias is replaced by its target (resolved in the alias's
/// file) while the budget lasts, or one of a built-in type by its schema,
/// then the children are resolved.
fn resolve_at(value: &mut Value, targets: Targets<'_>, location: &Location, state: &mut State<'_>) {
    let Targets {
        known, registry, ..
    } = targets;
    match value {
        Value::String(s) => {
            let Some(name) = reference_name(s) else {
                return;
            };
            let target = if known.contains(name) {
                Some(name.to_string())
            } else {
                registry
                    .alias(name)
                    .filter(|alias| known.contains(alias))
                    .map(str::to_string)
            };
            match target {
                Some(target) => *s = format!("#/components/schemas/{target}"),
                None => {
                    if state.reported.insert(name.to_string()) {
                        state.diagnostics.push(
                            Diagnostic::new(
                                Code::UnresolvedReference,
                                format!("`${name}` does not name a known schema"),
                            )
                            .at(location.clone()),
                        );
                    }
                }
            }
        }
        Value::Array(items) => {
            for item in items {
                resolve_at(item, targets, location, state);
            }
        }
        Value::Object(map) => {
            let lookup = Lookup { registry, known };
            // A bare built-in name (`Duration`) that is also a documented
            // schema, or a type alias of the file, refers to that.
            shadow::choose(map, |name| {
                known.contains(name)
                    || registry
                        .alias(name)
                        .is_some_and(|alias| known.contains(alias))
                    || lookup.is_local(name, location)
            });
            // A Rust type name that does not say which documented type it
            // means: reported, and not resolved. The marker is the finding
            // text; a key of its name with another value is not the
            // generator's and stays for the sweep.
            if let Some(finding) = map
                .get(UNRESOLVED_KEY)
                .and_then(Value::as_str)
                .map(str::to_string)
            {
                map.shift_remove(UNRESOLVED_KEY);
                map.shift_remove("$ref");
                if state.reported.insert(finding.clone()) {
                    state.diagnostics.push(
                        Diagnostic::new(Code::AmbiguousTypeName, finding).at(location.clone()),
                    );
                }
            }
            if let Some(name) = map
                .get("$ref")
                .and_then(Value::as_str)
                .and_then(reference_name)
                .map(str::to_string)
            {
                match lookup.find(
                    &name,
                    0,
                    location,
                    &state.expanding,
                    state.diagnostics,
                    &mut state.budget.findings,
                ) {
                    Found::Alias(alias) => {
                        if let Some(target) =
                            state
                                .budget
                                .take(&name, &alias.schema, location, state.diagnostics)
                        {
                            aliases::replace(map, target);
                            state.expanding.push(name);
                            resolve_at(value, targets, &alias.location, state);
                            state.expanding.pop();
                            return;
                        }
                    }
                    Found::Reported => {
                        state.reported.insert(name);
                    }
                    Found::Nothing => {}
                }
            }
            // `#[serde(default)]` of a documented type with a `default`. The
            // marker is `true`; a key of its name with another value is not
            // the generator's and stays for the sweep.
            if map.get(TYPE_DEFAULT_KEY) == Some(&Value::Bool(true)) {
                map.shift_remove(TYPE_DEFAULT_KEY);
                if let Some(default) = map
                    .get("$ref")
                    .and_then(Value::as_str)
                    .and_then(reference_name)
                    .map(|name| registry.alias(name).unwrap_or(name))
                    .and_then(|name| registry.default_of(name))
                    .cloned()
                {
                    map.entry("default").or_insert(default);
                }
            }
            let schema = map
                .get("$ref")
                .and_then(Value::as_str)
                .and_then(reference_name)
                .and_then(|name| builtin(name, targets));
            if let Some(schema) = schema {
                map.remove("$ref");
                for (key, value) in schema {
                    map.entry(key).or_insert(value);
                }
            }
            for child in map.values_mut() {
                resolve_at(child, targets, location, state);
            }
            // An unknown name (reported above) still becomes a pointer, so
            // the reference stays well-formed and the lints see it.
            if let Some(Value::String(target)) = map.get_mut("$ref")
                && let Some(name) = reference_name(target)
            {
                *target = format!("#/components/schemas/{name}");
            }
            // A `$` reference that is no name (`$Bad Name`) stays as written,
            // reported; generic ones (`$Page<…>`) the generics stage reports.
            if let Some(Value::String(target)) = map.get("$ref")
                && target.starts_with('$')
                && !target.contains('<')
                && state.reported.insert(target.clone())
            {
                state.diagnostics.push(
                    Diagnostic::new(
                        Code::UnresolvedReference,
                        format!("`{target}` is not a schema name"),
                    )
                    .at(location.clone()),
                );
            }
        }
        _ => {}
    }
}

/// Turns protected `\$` escapes back into `$` (the very last text step).
/// Regular expressions keep `\$`, their escape of a literal dollar sign:
/// `pattern` values and the keys of `patternProperties`.
///
/// # Examples
///
/// ```
/// use oas_forge::template::lexical::ESCAPED_DOLLAR;
/// use oas_forge::template::refs;
/// use serde_json::json;
///
/// // `\$5` and `^\$[0-9]+$` after the text stage:
/// let mut value = json!({
///     "description": format!("Costs {ESCAPED_DOLLAR}5"),
///     "pattern": format!("^{ESCAPED_DOLLAR}[0-9]+$")
/// });
/// refs::unescape(&mut value);
/// assert_eq!(value, json!({ "description": "Costs $5", "pattern": "^\\$[0-9]+$" }));
/// ```
pub fn unescape(value: &mut Value) {
    unescape_at(value, false);
}

/// `regex`: a string `value` is a regular expression (the value of
/// `pattern`), or the keys of an object `value` are (`patternProperties`).
fn unescape_at(value: &mut Value, regex: bool) {
    match value {
        Value::String(s) if s.contains(ESCAPED_DOLLAR) => *s = restore(s, regex),
        Value::Array(items) => items.iter_mut().for_each(|item| unescape_at(item, false)),
        Value::Object(map) => {
            if map.keys().any(|k| k.contains(ESCAPED_DOLLAR)) {
                let entries: Vec<_> = std::mem::take(map).into_iter().collect();
                for (key, mut child) in entries {
                    unescape_child(&key, &mut child);
                    map.insert(restore(&key, regex), child);
                }
            } else {
                for (key, child) in map.iter_mut() {
                    unescape_child(key, child);
                }
            }
        }
        _ => {}
    }
}

/// Unescapes the value of `key`; the keys of `patternProperties` and the
/// value of `pattern` are regular expressions.
fn unescape_child(key: &str, child: &mut Value) {
    match child {
        Value::Object(_) => unescape_at(child, key == "patternProperties"),
        _ => unescape_at(child, key == "pattern"),
    }
}

/// `text` with each [`ESCAPED_DOLLAR`] turned back into `$`, or into `\$`
/// in a regular expression.
fn restore(text: &str, regex: bool) -> String {
    text.replace(ESCAPED_DOLLAR, if regex { "\\$" } else { "$" })
}
