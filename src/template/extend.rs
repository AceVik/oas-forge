//! Structural `@extend`: fragments merged into the mapping that contains the
//! `@extend` line. Local values win; the fragment fills gaps (ADR 0003).

use super::lexical::{EXTEND_KEY, split_args, substitute_params};
use super::registry::Registry;
use crate::diagnostics::{Code, Diagnostic, Diagnostics, Location};
use crate::merge::{Policy, merge};
use crate::yaml;
use serde_json::Value;

const MAX_DEPTH: usize = 8;

/// Resolves every `@extend` marker in `value`.
pub fn apply(
    value: &mut Value,
    registry: &Registry,
    location: &Location,
    diagnostics: &mut Diagnostics,
) {
    apply_at(value, registry, location, diagnostics, 0);
}

fn apply_at(
    value: &mut Value,
    registry: &Registry,
    location: &Location,
    diagnostics: &mut Diagnostics,
    depth: usize,
) {
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
                apply_at(child, registry, location, diagnostics, depth);
            }
            for spec in fragments {
                if let Some(mut fragment) = resolve(&spec, registry, location, diagnostics) {
                    if depth < MAX_DEPTH {
                        apply_at(&mut fragment, registry, location, diagnostics, depth + 1);
                    }
                    merge(value, fragment, Policy::Extend);
                }
            }
        }
        Value::Array(items) => {
            for item in items {
                apply_at(item, registry, location, diagnostics, depth);
            }
        }
        _ => {}
    }
}

/// `Name` or `Name(args)` → the fragment body as a value.
fn resolve(
    spec: &str,
    registry: &Registry,
    location: &Location,
    diagnostics: &mut Diagnostics,
) -> Option<Value> {
    let (name, args) = match spec.find('(') {
        Some(open) => (
            spec[..open].trim(),
            split_args(spec[open + 1..].trim_end().trim_end_matches(')')),
        ),
        None => (spec.trim(), Vec::new()),
    };
    let Some(fragment) = registry.fragment(name) else {
        diagnostics.push(
            Diagnostic::new(
                Code::UnknownFragment,
                format!("`@extend {name}`: unknown fragment"),
            )
            .at(location.clone()),
        );
        return None;
    };
    if args.len() < fragment.params.len() {
        diagnostics.push(
            Diagnostic::new(
                Code::MissingFragmentArgument,
                format!(
                    "fragment `{name}` expects {} argument(s), got {}",
                    fragment.params.len(),
                    args.len()
                ),
            )
            .at(location.clone()),
        );
    }
    let body = substitute_params(&fragment.body, &fragment.params, &args);
    match yaml::parse(&body) {
        Ok(value) => Some(value),
        Err(e) => {
            diagnostics.push(
                Diagnostic::new(
                    Code::InvalidFragment,
                    format!(
                        "fragment `{name}` is not valid YAML (line {} of its body): {}",
                        e.line, e.message
                    ),
                )
                .at(location.clone()),
            );
            None
        }
    }
}
