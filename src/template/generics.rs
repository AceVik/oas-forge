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
use std::sync::OnceLock;

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
                    GenericArg::Name(name) => name.clone(),
                    GenericArg::Generic(inner) => inner.concrete_name(),
                })
                .collect::<Vec<_>>()
                .join("_")
        };
        format!("{}_{suffix}", self.name)
    }
}

fn is_name_char(c: char) -> bool {
    c.is_alphanumeric() || c == '_' || c == ':'
}

/// Parses `Name<args>` at the start of `text`; returns the rest.
fn parse_ref(text: &str) -> Option<(GenericRef, &str)> {
    let end = text.find(|c: char| !is_name_char(c)).unwrap_or(text.len());
    let name = &text[..end];
    if name.is_empty() {
        return None;
    }
    let rest = text[end..].trim_start().strip_prefix('<')?;
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
    let end = text.find(|c: char| !is_name_char(c)).unwrap_or(text.len());
    if end == 0 {
        return None;
    }
    if text[end..].trim_start().starts_with('<') {
        let (generic, rest) = parse_ref(text)?;
        Some((GenericArg::Generic(generic), rest))
    } else {
        Some((GenericArg::Name(text[..end].to_string()), &text[end..]))
    }
}

/// Instantiates blueprints referenced by whole string scalars.
pub struct Instantiator<'a> {
    registry: &'a Registry,
    concrete: BTreeMap<String, (Value, Location)>,
    in_progress: Vec<String>,
}

impl<'a> Instantiator<'a> {
    /// An instantiator using the blueprints of `registry`.
    pub fn new(registry: &'a Registry) -> Self {
        Self {
            registry,
            concrete: BTreeMap::new(),
            in_progress: Vec::new(),
        }
    }

    /// Replaces generic references in `value`; reports problems at `location`.
    pub fn apply(&mut self, value: &mut Value, location: &Location, diagnostics: &mut Diagnostics) {
        match value {
            Value::String(s) if s.starts_with('$') && s.contains('<') => {
                if let Some(replacement) = self.replace(s, location, diagnostics) {
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
                let inline = map
                    .get("$ref")
                    .and_then(Value::as_str)
                    .and_then(|r| self.std_container(r, location, diagnostics));
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

    /// The inline schema of a `$Vec<T>`-style shorthand (std containers that
    /// are not shadowed by a blueprint of the same name).
    fn std_container(
        &mut self,
        text: &str,
        location: &Location,
        diagnostics: &mut Diagnostics,
    ) -> Option<Value> {
        let generic = GenericRef::parse(text)?;
        if !is_container(&generic.name) || self.registry.blueprint(&generic.name).is_some() {
            return None;
        }
        let mut schema = TypeMapper::new().map(&TypeExpr::parse(text)?).schema;
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
        location: &Location,
        diagnostics: &mut Diagnostics,
    ) -> Option<Value> {
        // `$Vec<T>` and other std containers are inline arrays, not templates.
        if let Some(schema) = self.std_container(text, location, diagnostics) {
            return Some(schema);
        }
        let generic = GenericRef::parse(text)?;
        self.instantiate(&generic, location, diagnostics)
            .map(|name| Value::String(format!("${name}")))
    }

    /// Creates the concrete schema for `generic`; returns its name.
    fn instantiate(
        &mut self,
        generic: &GenericRef,
        location: &Location,
        diagnostics: &mut Diagnostics,
    ) -> Option<String> {
        let Some(blueprint) = self.registry.blueprint(&generic.name) else {
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
        let mut arg_names = Vec::new();
        for arg in &generic.args {
            arg_names.push(match arg {
                GenericArg::Name(name) => name.clone(),
                GenericArg::Generic(inner) => self.instantiate(inner, location, diagnostics)?,
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

fn placeholder_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"\$(\w+)").unwrap())
}

/// Replaces whole `$Param` identifiers in every string (and key) of `schema`
/// in a single pass, so substituted names are never substituted again.
fn substitute(schema: &Value, bindings: &BTreeMap<&str, &str>) -> Value {
    let replace = |text: &str| -> String {
        placeholder_re()
            .replace_all(text, |caps: &Captures| match bindings.get(&caps[1]) {
                Some(arg) => format!("${arg}"),
                None => caps[0].to_string(),
            })
            .into_owned()
    };
    match schema {
        Value::String(s) => Value::String(replace(s)),
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
