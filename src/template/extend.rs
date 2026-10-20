//! Structural `@extend`: fragments merged into the mapping that contains the
//! `@extend` line. Local values win; the fragment fills gaps (ADR 0003).
//!
//! A fragment body goes through the same text stage as any other YAML
//! ([`lexical::expand`]): its `@insert`s are pasted, its `@extend`s become
//! markers that are resolved here, and `{{variables}}` are filled in.

use super::lexical::{self, EXTEND_KEY, Variables, split_args, substitute_params};
use super::registry::Registry;
use crate::diagnostics::{Code, Diagnostic, Diagnostics, Location};
use crate::merge::{Policy, merge};
use crate::yaml;
use serde_json::Value;
use std::collections::BTreeSet;

const MAX_DEPTH: usize = 8;

/// Resolves every `@extend` marker in `value`.
pub fn apply(
    value: &mut Value,
    registry: &Registry,
    variables: &Variables,
    location: &Location,
    diagnostics: &mut Diagnostics,
) {
    let mut extension = Extension {
        registry,
        variables,
        location,
        diagnostics,
        stack: Vec::new(),
        reported: BTreeSet::new(),
    };
    extension.apply_at(value);
}

/// The state of an [`apply`] run.
struct Extension<'a> {
    registry: &'a Registry,
    variables: &'a Variables,
    location: &'a Location,
    diagnostics: &'a mut Diagnostics,
    /// The fragments being extended, outermost first.
    stack: Vec<String>,
    /// Fragments whose cycle or depth problem has been reported.
    reported: BTreeSet<String>,
}

impl Extension<'_> {
    fn apply_at(&mut self, value: &mut Value) {
        match value {
            Value::Object(map) => {
                let markers: Vec<String> = map
                    .keys()
                    .filter(|k| k.starts_with(EXTEND_KEY))
                    .cloned()
                    .collect();
                let mut fragments = Vec::new();
                for key in markers {
                    if let Some(spec) = map
                        .remove(&key)
                        .and_then(|v| v.as_str().map(str::to_string))
                    {
                        fragments.push(spec);
                    }
                }
                for child in map.values_mut() {
                    self.apply_at(child);
                }
                for spec in fragments {
                    if let Some((name, mut fragment)) = self.resolve(&spec) {
                        self.stack.push(name);
                        self.apply_at(&mut fragment);
                        self.stack.pop();
                        merge(value, fragment, Policy::Extend);
                    }
                }
            }
            Value::Array(items) => {
                for item in items {
                    self.apply_at(item);
                }
            }
            _ => {}
        }
    }

    fn report(&mut self, code: Code, message: String) {
        self.diagnostics
            .push(Diagnostic::new(code, message).at(self.location.clone()));
    }

    /// `Name` or `Name(args)` → the fragment's name and body as a value.
    fn resolve(&mut self, spec: &str) -> Option<(String, Value)> {
        let (name, args) = match spec.split_once('(') {
            Some((name, args)) => (
                name.trim(),
                split_args(args.trim_end().trim_end_matches(')')),
            ),
            None => (spec.trim(), Vec::new()),
        };
        let registry = self.registry;
        let Some(fragment) = registry.fragment(name) else {
            self.report(
                Code::UnknownFragment,
                format!("`@extend {name}`: unknown fragment"),
            );
            return None;
        };
        if self.stack.iter().any(|n| n == name) {
            if self.reported.insert(name.to_string()) {
                let path = self.stack.join(" → ");
                self.report(
                    Code::FragmentCycle,
                    format!(
                        "fragment `{name}` extends itself ({path} → {name}); the extend is skipped"
                    ),
                );
            }
            return None;
        }
        if self.stack.len() >= MAX_DEPTH {
            if self.reported.insert(name.to_string()) {
                self.report(
                    Code::UnknownFragment,
                    format!("`@extend {name}` is nested too deeply"),
                );
            }
            return None;
        }
        if args.len() < fragment.params.len() {
            self.report(
                Code::MissingFragmentArgument,
                format!(
                    "fragment `{name}` expects {} argument(s), got {}",
                    fragment.params.len(),
                    args.len()
                ),
            );
        }
        let body = substitute_params(&fragment.body, &fragment.params, &args);
        let body = lexical::expand(
            &body,
            registry,
            self.variables,
            self.location,
            self.diagnostics,
        );
        match yaml::parse(&body) {
            Ok(value) => Some((name.to_string(), value)),
            Err(e) => {
                self.report(
                    Code::InvalidFragment,
                    format!(
                        "fragment `{name}` is not valid YAML (line {} of its body): {}",
                        e.line, e.message
                    ),
                );
                None
            }
        }
    }
}
