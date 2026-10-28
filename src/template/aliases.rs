//! Transparent type aliases: a reference to a type alias without `@openapi`
//! (`{"$ref": "$Owner"}` for `type Owner = User;`) is replaced by the schema
//! of the alias's target, as if the target had been written.
//!
//! An alias defined in the file of the reference wins; an alias of another
//! file applies when no documented schema has the name and all aliases of
//! that name agree. The generics stage replaces references to generic
//! aliases (`$Paged<User>`) and to aliases whose target is generic; the
//! references stage replaces the rest.

use super::refs::KnownSchemas;
use super::registry::{Registry, Transparent};
use crate::diagnostics::{Code, Diagnostic, Diagnostics, Location};
use crate::extract::AliasItem;
use crate::schema::types::undocumentable;
use serde_json::{Map, Value};

/// The longest chain of type aliases that is followed (`A` → `B` → …). A
/// longer one is reported and not resolved; this also bounds the recursion
/// of the stages that replace aliases.
pub(crate) const MAX_ALIAS_DEPTH: usize = 64;

/// The outcome of [`Lookup::find`].
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) enum Found<'a> {
    /// The alias that replaces the reference.
    Alias(&'a AliasItem),
    /// The name is ambiguous or part of a cycle; this was reported.
    Reported,
    /// The reference stays as it is.
    Nothing,
}

/// Where an alias lookup happens.
#[derive(Clone, Copy)]
pub(crate) struct Lookup<'a> {
    /// The transparent aliases.
    pub(crate) registry: &'a Registry,
    /// The documented schema names, which win over aliases of other files.
    pub(crate) known: &'a KnownSchemas,
}

impl<'a> Lookup<'a> {
    /// The alias that replaces a reference to `name` with `args` type
    /// arguments at `location`. `expanding` holds the aliases being replaced
    /// around the reference, innermost last: the name of the innermost one
    /// means another type (`type User = models::User;`), an outer one is a
    /// cycle. An ambiguous name and a cycle are reported (once) and replace
    /// nothing; so does an alias with another number of type parameters.
    /// Documented schemas and blueprints are documented names.
    pub(crate) fn find(
        self,
        name: &str,
        args: usize,
        location: &Location,
        expanding: &[String],
        diagnostics: &mut Diagnostics,
    ) -> Found<'a> {
        if expanding.last().is_some_and(|last| last == name) {
            return Found::Nothing;
        }
        let documented = self.known.contains(name)
            || self.registry.blueprint(name).is_some()
            || self
                .registry
                .alias(name)
                .is_some_and(|schema| self.known.contains(schema));
        let alias = match self.registry.transparent(name, location.file()) {
            Transparent::Local(alias) => alias,
            Transparent::Foreign(alias) if !documented => alias,
            Transparent::Ambiguous(aliases) if !documented => {
                let places: Vec<String> = aliases.iter().map(|a| a.location.to_string()).collect();
                push_once(
                    diagnostics,
                    Diagnostic::new(
                        Code::AmbiguousAlias,
                        format!(
                            "`{name}` names type aliases with different targets ({}); the reference is not resolved",
                            places.join(", ")
                        ),
                    )
                    .at(location.clone()),
                );
                return Found::Reported;
            }
            _ => return Found::Nothing,
        };
        if expanding.iter().any(|outer| outer == name) {
            let cycle: Vec<&str> = expanding
                .iter()
                .skip_while(|outer| *outer != name)
                .map(String::as_str)
                .chain([name])
                .collect();
            push_once(
                diagnostics,
                Diagnostic::new(
                    Code::AliasCycle,
                    format!(
                        "type alias `{name}` refers to itself (`{}`); it is not resolved",
                        cycle.join("` → `")
                    ),
                )
                .at(alias.location.clone()),
            );
            return Found::Reported;
        }
        if expanding.len() >= MAX_ALIAS_DEPTH {
            push_once(
                diagnostics,
                Diagnostic::new(
                    Code::AliasDepthLimit,
                    format!(
                        "type alias `{name}` is reached through more than {MAX_ALIAS_DEPTH} type aliases; it is not resolved"
                    ),
                )
                .at(alias.location.clone()),
            );
            return Found::Reported;
        }
        if alias.params.len() != args {
            return Found::Nothing;
        }
        for part in &alias.undocumentable {
            push_once(diagnostics, undocumentable(part).at(alias.location.clone()));
        }
        Found::Alias(alias)
    }

    /// Whether `name` is the alias of the same name defined in the file of
    /// `location` (it then wins over built-in types of that name).
    pub(crate) fn is_local(self, name: &str, location: &Location) -> bool {
        matches!(
            self.registry.transparent(name, location.file()),
            Transparent::Local(_)
        )
    }
}

/// Replaces `map`, a schema with a `$ref` to an alias, by the alias's
/// `schema`; the other keywords of `map` (a description, an override) win.
pub(crate) fn replace(map: &mut Map<String, Value>, schema: Value) {
    let siblings = std::mem::take(map);
    if let Value::Object(schema) = schema {
        *map = schema;
    }
    for (key, value) in siblings {
        if key != "$ref" {
            map.insert(key, value);
        }
    }
}

/// Adds `diagnostic` unless the same finding was reported before.
fn push_once(diagnostics: &mut Diagnostics, diagnostic: Diagnostic) {
    if !diagnostics.iter().any(|d| *d == diagnostic) {
        diagnostics.push(diagnostic);
    }
}
