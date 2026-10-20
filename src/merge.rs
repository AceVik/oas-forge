//! Merging of JSON values under explicit policies (ADR 0003).

use serde_json::{Map, Value};
use std::collections::HashSet;

/// How two values are combined.
///
/// # Examples
///
/// ```
/// use oas_forge::merge::{Policy, merge};
/// use serde_json::json;
///
/// // `@extend`: local values win, the fragment fills gaps.
/// let mut local = json!({ "description": "Local", "tags": ["a"] });
/// let fragment = json!({ "description": "From the fragment", "deprecated": true, "tags": ["b"] });
/// let conflicts = merge(&mut local, fragment, Policy::Extend);
/// assert_eq!(local, json!({ "description": "Local", "tags": ["a", "b"], "deprecated": true }));
/// assert!(conflicts.is_empty());
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum Policy {
    /// Combining documents: maps recursively, arrays appended and
    /// de-duplicated, scalars won by the source (conflicts are reported).
    Document,
    /// `@extend`: the target is local content, the source a fragment. Local
    /// scalars win; the fragment only fills gaps; arrays are appended.
    Extend,
    /// DSL operation (target) plus embedded YAML (source): `parameters` are
    /// matched by `name` and `in` and merged, scalars are won by the YAML
    /// (conflicts are reported), other arrays appended.
    Operation,
    /// `@openapi` overrides of reflected schemas: maps recursively,
    /// everything else replaced by the source.
    Override,
}

/// Keys whose array items are positional and therefore never de-duplicated.
const POSITIONAL: &[&str] = &["prefixItems", "allOf", "oneOf", "anyOf"];

/// Merges `source` into `target` and returns the JSON pointers of
/// conflicting values (always empty for [`Policy::Extend`] and [`Policy::Override`]).
///
/// # Examples
///
/// ```
/// use oas_forge::merge::{Policy, merge};
/// use serde_json::json;
///
/// let mut document = json!({
///     "info": { "title": "Shop", "version": "1.0.0" },
///     "tags": [{ "name": "orders" }]
/// });
/// let conflicts = merge(
///     &mut document,
///     json!({
///         "info": { "title": "Store" },
///         "tags": [{ "name": "orders" }, { "name": "users" }]
///     }),
///     Policy::Document,
/// );
/// assert_eq!(conflicts, ["/info/title"]);
/// assert_eq!(
///     document,
///     json!({
///         "info": { "title": "Store", "version": "1.0.0" },
///         "tags": [{ "name": "orders" }, { "name": "users" }]
///     })
/// );
/// ```
pub fn merge(target: &mut Value, source: Value, policy: Policy) -> Vec<String> {
    let mut conflicts = Vec::new();
    merge_at(target, source, policy, &mut String::new(), &mut conflicts);
    conflicts
}

/// Merges `source` into `target`, which sits at the JSON pointer `pointer`:
/// maps key by key, arrays by `policy` (replaced for [`Policy::Override`],
/// [`POSITIONAL`] arrays kept or replaced as a whole, operation `parameters`
/// matched by identity, others appended without duplicates) and differing scalars by
/// `policy`. Conflicts are recorded as pointers.
fn merge_at(
    target: &mut Value,
    source: Value,
    policy: Policy,
    pointer: &mut String,
    conflicts: &mut Vec<String>,
) {
    match (target, source) {
        (Value::Object(t), Value::Object(s)) => merge_maps(t, s, policy, pointer, conflicts),
        (Value::Array(t), Value::Array(s)) => {
            let key = pointer.rsplit('/').next().unwrap_or_default();
            match policy {
                Policy::Override => *t = s,
                _ if POSITIONAL.contains(&key) => {
                    if *t != s && policy != Policy::Extend {
                        conflicts.push(pointer.clone());
                        *t = s;
                    }
                }
                Policy::Operation if key == "parameters" => {
                    merge_parameters(t, s, pointer, conflicts);
                }
                _ => append_unique(t, s),
            }
        }
        (target, source) => {
            if *target == source {
                return;
            }
            match policy {
                Policy::Extend => {}
                Policy::Override => *target = source,
                Policy::Document | Policy::Operation => {
                    conflicts.push(pointer.clone());
                    *target = source;
                }
            }
        }
    }
}

/// Merges the map `source` into `target`: new keys are appended, existing
/// ones merged with `pointer` extended by the escaped key (RFC 6901).
fn merge_maps(
    target: &mut Map<String, Value>,
    source: Map<String, Value>,
    policy: Policy,
    pointer: &mut String,
    conflicts: &mut Vec<String>,
) {
    for (key, value) in source {
        match target.get_mut(&key) {
            Some(existing) => {
                let len = pointer.len();
                pointer.push('/');
                pointer.push_str(&key.replace('~', "~0").replace('/', "~1"));
                merge_at(existing, value, policy, pointer, conflicts);
                pointer.truncate(len);
            }
            None => {
                target.insert(key, value);
            }
        }
    }
}

/// Appends the items of `source` that `target` does not contain yet
/// (compared by their JSON text), in order.
fn append_unique(target: &mut Vec<Value>, source: Vec<Value>) {
    let mut seen: HashSet<String> = target.iter().map(Value::to_string).collect();
    for item in source {
        if seen.insert(item.to_string()) {
            target.push(item);
        }
    }
}

/// Parameters are identified by `name` + `in`; a source parameter is merged
/// into the target parameter with the same identity (the source wins
/// conflicts), others are appended.
fn merge_parameters(
    target: &mut Vec<Value>,
    source: Vec<Value>,
    pointer: &mut String,
    conflicts: &mut Vec<String>,
) {
    let identity = |p: &Value| match (p.get("name"), p.get("in")) {
        (Some(name), Some(location)) => Some((name.clone(), location.clone())),
        _ => None,
    };
    for item in source {
        let existing = identity(&item).and_then(|id| {
            target
                .iter_mut()
                .enumerate()
                .find(|(_, t)| identity(t).as_ref() == Some(&id))
        });
        match existing {
            Some((index, existing)) => {
                let len = pointer.len();
                pointer.push_str(&format!("/{index}"));
                merge_at(existing, item, Policy::Operation, pointer, conflicts);
                pointer.truncate(len);
            }
            None => {
                if !target.contains(&item) {
                    target.push(item);
                }
            }
        }
    }
}
