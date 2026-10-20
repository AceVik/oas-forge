//! Generic instantiation of blueprints (`$Page<User>` → `Page_User`).
//!
//! Generic references are whole string scalars. Each distinct instantiation
//! produces one concrete schema; instantiated bodies are processed again until
//! no generic reference is left (bounded by [`MAX_DEPTH`]).

use super::registry::Registry;
use crate::diagnostics::{Code, Diagnostic, Diagnostics, Location};
use crate::schema::types::{TypeExpr, TypeMapper, is_container};
use regex::{Captures, Regex};
use serde_json::{Map, Value};
use std::collections::BTreeMap;
use std::sync::LazyLock;

/// Maximum nesting of instantiations (`$A<$B<$C<…>>>`).
pub const MAX_DEPTH: usize = 16;

/// A parsed generic reference.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GenericRef {
    /// The blueprint name.
    pub name: String,
    /// The arguments.
    pub args: Vec<GenericArg>,
}

/// One argument of a generic reference.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GenericArg {
    /// A plain type name (`User`).
    Name(String),
    /// A nested generic reference (`$Page<User>`).
    Generic(GenericRef),
}

impl GenericRef {
    /// Parses `$Name<Arg, $Other<X>>` (the `$` of arguments is optional).
    pub fn parse(text: &str) -> Option<Self> {
        let text = text.trim().strip_prefix('$')?;
        let (generic, rest) = parse_ref(text)?;
        rest.trim().is_empty().then_some(generic)
    }

    /// The concrete schema name (`Page_User`, `Map_String_Pair_A_B`).
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

    /// The reference as type text without `$` (`Vec<User>`).
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

fn is_name_char(c: char) -> bool {
    c.is_alphanumeric() || c == '_' || c == ':'
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
pub struct Instantiator<'a> {
    registry: &'a Registry,
    types: &'a TypeMapper,
    concrete: BTreeMap<String, (Value, Location)>,
    in_progress: Vec<String>,
}

impl<'a> Instantiator<'a> {
    /// An instantiator using the blueprints of `registry` and the built-in
    /// type table.
    pub fn new(registry: &'a Registry) -> Self {
        Self {
            registry,
            types: TypeMapper::builtin(),
            concrete: BTreeMap::new(),
            in_progress: Vec::new(),
        }
    }

    /// Uses `types` for type arguments such as `Vec<User>` or `Option<u8>`.
    pub fn with_types(mut self, types: &'a TypeMapper) -> Self {
        self.types = types;
        self
    }

    /// Replaces generic references in `value`; reports problems at `location`.
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
                // `{$ref: $Vec<User>}`: the shorthand replaces the whole schema;
                // sibling keywords such as `description` are kept.
                let inline = map.get("$ref").and_then(Value::as_str).and_then(|r| {
                    let generic = GenericRef::parse(r)?;
                    self.std_container(r, &generic, location, diagnostics)
                });
                if let Some(Value::Object(mut schema)) = inline {
                    map.remove("$ref");
                    for (key, mut child) in std::mem::take(map) {
                        self.apply(&mut child, location, diagnostics);
                        schema.insert(key, child);
                    }
                    *map = schema;
                    return;
                }
                for child in map.values_mut() {
                    self.apply(child, location, diagnostics);
                }
            }
            _ => {}
        }
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
        let mapped = self.types.map(&TypeExpr::parse(text)?);
        if !is_container(last_segment(&generic.name)) && mapped.schema.get("$ref").is_some() {
            return None;
        }
        let mut schema = mapped.into_schema();
        self.apply(&mut schema, location, diagnostics);
        Some(schema)
    }

    /// The concrete schemas created so far, by name, with the location of
    /// their first use.
    pub fn into_concrete(self) -> BTreeMap<String, (Value, Location)> {
        self.concrete
    }

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
}

static PLACEHOLDER_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"\$(\w+)").expect("constant pattern"));

/// Replaces whole `$Param` identifiers in every string (and key) of `schema`
/// in a single pass, so substituted names are never substituted again. In a
/// whole generic reference (`$Envelope<T>`), bare parameter names are bound
/// as well.
fn substitute(schema: &Value, bindings: &BTreeMap<&str, &str>) -> Value {
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
