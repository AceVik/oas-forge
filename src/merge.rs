//! Merging of JSON values under explicit policies (ADR 0003).

use serde_json::{Map, Value};
use std::collections::{BTreeMap, BTreeSet, HashSet};
use std::ops::Bound;

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
    /// scalars win; the fragment only fills gaps; arrays are appended,
    /// except `security`: a local list wins, since another alternative would
    /// accept more than the local one does.
    Extend,
    /// DSL operation (target) plus embedded YAML (source): `parameters` are
    /// matched by `name` and `in` and merged, scalars are won by the YAML
    /// (conflicts are reported), other arrays appended.
    Operation,
    /// `@openapi` overrides of reflected schemas: maps recursively,
    /// everything else replaced by the source.
    Override,
}

/// Whether the `security` requirements `items` admit anonymous requests:
/// none at all, or an empty alternative.
fn admits_anonymous(items: &[Value]) -> bool {
    items.is_empty()
        || items
            .iter()
            .any(|item| item.as_object().is_some_and(Map::is_empty))
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
    Merger::default().merge(target, source, policy)
}

/// How a later source changed a `security` or `servers` list (see
/// [`Merger::watching_lists`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ListChange {
    /// Entries were appended to the list.
    Joined,
    /// The list was added to an existing object below the root, an
    /// operation or a path, where the document's list applied before.
    Introduced,
    /// A source added to an object below the root whose `servers`, or
    /// `security` that admits anonymous requests (`[]`, `{}`), another
    /// source set: the list and the rest of the operation or path come from
    /// different annotations, in either order.
    Elsewhere,
}

/// Merges sources one after another into one target, as [`merge`] does
/// with each. It remembers the items of the lists it appended to, so a
/// source costs time in proportion to its own size, not to the target's.
#[derive(Debug, Default)]
pub(crate) struct Merger {
    /// The JSON texts of the items of each list appended to, by pointer.
    lists: BTreeMap<String, HashSet<String>>,
    /// The `security` and `servers` lists that sources changed, by pointer.
    changes: Vec<(String, ListChange)>,
    /// The pointers of lists reported as [`ListChange::Elsewhere`].
    elsewhere: BTreeSet<String>,
    /// Whether `changes` are recorded.
    watch_lists: bool,
}

impl Merger {
    /// A merger that records the `security` and `servers` lists a source
    /// joins with the target's or adds to an existing operation or path:
    /// either can make authentication optional without a conflict.
    pub(crate) fn watching_lists() -> Self {
        Self {
            watch_lists: true,
            ..Self::default()
        }
    }

    /// Merges `source` into `target` like [`merge`]; `target` must be the
    /// value of the earlier merges, changed by nothing else.
    pub(crate) fn merge(
        &mut self,
        target: &mut Value,
        source: Value,
        policy: Policy,
    ) -> Vec<String> {
        let mut conflicts = Vec::new();
        self.merge_at(target, source, policy, &mut String::new(), &mut conflicts);
        conflicts
    }

    /// The `security` and `servers` lists changed since the last call.
    pub(crate) fn take_changes(&mut self) -> Vec<(String, ListChange)> {
        std::mem::take(&mut self.changes)
    }

    /// Merges `source` into `target`, which sits at the JSON pointer `pointer`:
    /// maps key by key, arrays by `policy` (replaced for [`Policy::Override`],
    /// [`POSITIONAL`] arrays kept or replaced as a whole, operation `parameters`
    /// matched by identity, others appended without duplicates) and differing scalars by
    /// `policy`. Conflicts are recorded as pointers.
    fn merge_at(
        &mut self,
        target: &mut Value,
        source: Value,
        policy: Policy,
        pointer: &mut String,
        conflicts: &mut Vec<String>,
    ) {
        match (target, source) {
            (Value::Object(t), Value::Object(s)) => {
                if self.watch_lists && !pointer.is_empty() && !s.is_empty() {
                    for key in ["security", "servers"] {
                        let list = format!("{pointer}/{key}");
                        if t.get(key)
                            .and_then(Value::as_array)
                            .is_some_and(|items| key == "servers" || admits_anonymous(items))
                            && !s.contains_key(key)
                            && self.elsewhere.insert(list.clone())
                        {
                            self.changes.push((list, ListChange::Elsewhere));
                        }
                    }
                }
                self.merge_maps(t, s, policy, pointer, conflicts);
            }
            (Value::Array(t), Value::Array(s)) => {
                let key = pointer.rsplit('/').next().unwrap_or_default();
                match policy {
                    Policy::Override => {
                        self.forget(pointer);
                        *t = s;
                    }
                    _ if POSITIONAL.contains(&key) => {
                        if *t != s && policy != Policy::Extend {
                            conflicts.push(pointer.clone());
                            self.forget(pointer);
                            *t = s;
                        }
                    }
                    Policy::Operation if key == "parameters" => {
                        self.merge_parameters(t, s, pointer, conflicts);
                    }
                    Policy::Extend if key == "security" => {}
                    _ => {
                        let watched = self.watch_lists
                            && matches!(key, "security" | "servers")
                            && !t.is_empty();
                        if self.append_unique(t, s, pointer) && watched {
                            self.changes.push((pointer.clone(), ListChange::Joined));
                        }
                    }
                }
            }
            (target, source) => {
                if *target == source {
                    return;
                }
                match policy {
                    Policy::Extend => {}
                    Policy::Override => {
                        self.forget(pointer);
                        *target = source;
                    }
                    Policy::Document | Policy::Operation => {
                        conflicts.push(pointer.clone());
                        self.forget(pointer);
                        *target = source;
                    }
                }
            }
        }
    }

    /// Merges the map `source` into `target`: new keys are appended, existing
    /// ones merged with `pointer` extended by the escaped key (RFC 6901).
    fn merge_maps(
        &mut self,
        target: &mut Map<String, Value>,
        source: Map<String, Value>,
        policy: Policy,
        pointer: &mut String,
        conflicts: &mut Vec<String>,
    ) {
        for (key, value) in source {
            let len = pointer.len();
            pointer.push('/');
            pointer.push_str(&key.replace('~', "~0").replace('/', "~1"));
            match target.get_mut(&key) {
                Some(existing) => self.merge_at(existing, value, policy, pointer, conflicts),
                None => {
                    if self.watch_lists
                        && len > 0
                        && matches!(key.as_str(), "security" | "servers")
                        && value.is_array()
                    {
                        self.changes.push((pointer.clone(), ListChange::Introduced));
                    }
                    target.insert(key, value);
                }
            }
            pointer.truncate(len);
        }
    }

    /// Appends the items of `source` that the list at `pointer` does not
    /// contain yet (compared by their JSON text), in order; returns whether
    /// any was appended. The texts are kept for the next source.
    fn append_unique(
        &mut self,
        target: &mut Vec<Value>,
        source: Vec<Value>,
        pointer: &str,
    ) -> bool {
        let seen = self
            .lists
            .entry(pointer.to_string())
            .or_insert_with(|| target.iter().map(Value::to_string).collect());
        let before = target.len();
        for item in source {
            if seen.insert(item.to_string()) {
                target.push(item);
            }
        }
        target.len() > before
    }

    /// Forgets the lists at `pointer` and below it, which a replacement
    /// changed.
    fn forget(&mut self, pointer: &str) {
        let below: Vec<String> = self
            .lists
            .range::<str, _>((Bound::Included(pointer), Bound::Unbounded))
            .map(|(key, _)| key)
            .map_while(|key| key.strip_prefix(pointer).map(|rest| (key, rest)))
            .filter(|(_, rest)| rest.is_empty() || rest.starts_with('/'))
            .map(|(key, _)| key.clone())
            .collect();
        for key in below {
            self.lists.remove(&key);
        }
    }

    /// Parameters are identified by `name` + `in`; a source parameter is merged
    /// into the target parameter with the same identity (the source wins
    /// conflicts), others are appended unless the list holds them already
    /// (compared by their JSON text).
    fn merge_parameters(
        &mut self,
        target: &mut Vec<Value>,
        source: Vec<Value>,
        pointer: &mut String,
        conflicts: &mut Vec<String>,
    ) {
        let identity = |p: &Value| match (p.get("name"), p.get("in")) {
            (Some(name), Some(location)) => Some(format!("{name} {location}")),
            _ => None,
        };
        // Indexes instead of a scan per source parameter.
        let mut by_identity = BTreeMap::new();
        let mut texts = HashSet::new();
        for (index, parameter) in target.iter().enumerate() {
            if let Some(id) = identity(parameter) {
                by_identity.entry(id).or_insert(index);
            }
            texts.insert(parameter.to_string());
        }
        for item in source {
            let found = identity(&item).and_then(|id| by_identity.get(&id).copied());
            match found.and_then(|index| Some((index, target.get_mut(index)?))) {
                Some((index, existing)) => {
                    let len = pointer.len();
                    pointer.push_str(&format!("/{index}"));
                    self.merge_at(existing, item, Policy::Operation, pointer, conflicts);
                    pointer.truncate(len);
                }
                None => {
                    if texts.insert(item.to_string()) {
                        if let Some(id) = identity(&item) {
                            by_identity.entry(id).or_insert(target.len());
                        }
                        target.push(item);
                    }
                }
            }
        }
    }
}
