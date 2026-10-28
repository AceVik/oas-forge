//! The reserved key prefix `x-oas-forge-`: the generator's own markers.
//!
//! The stages hand information to each other in keys of that prefix: the
//! alternative of a bare built-in name (`x-oas-forge-builtin`), a struct
//! expansion, a pattern constant, an `@extend` line, the templates of a
//! fragments output. A source must not write them: a forged marker would
//! be taken for the generator's own and could replace the object around it
//! without a trace. So every place where YAML written in a source enters
//! the pipeline removes them ([`strip`]) and reports them
//! ([`Code::ReservedKey`]). The `@extend` markers are the exception: the
//! lexical stage writes them into the text that is parsed, so they cannot
//! be told from written ones, and a written one does nothing an `@extend`
//! line would not. The templates of an imported fragments output are
//! another: their bodies are the generator's, recorded before the
//! references stage, and keep their markers (ADR 0005). What is left when
//! a document is complete is removed from it ([`sweep`]), so that no
//! marker reaches an output.

use crate::diagnostics::{Code, Diagnostic, Diagnostics, Location};
use crate::template::lexical::EXTEND_KEY;
use serde_json::{Map, Value};
use std::collections::BTreeMap;

/// The prefix of the generator's keys.
pub(crate) const PREFIX: &str = "x-oas-forge-";

/// Which keys of the prefix a value keeps.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Keep {
    /// The `@extend` markers of the lexical stage (prefix [`EXTEND_KEY`]),
    /// in text the lexical stage expanded.
    Markers,
    /// None: the text went through no stage that writes markers.
    Nothing,
}

/// A reserved key removed from a value.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Removed {
    /// The key.
    pub(crate) key: String,
    /// The JSON pointers of the key, in document order.
    pub(crate) pointers: Vec<String>,
}

impl Removed {
    /// The first pointer (there is always one).
    pub(crate) fn pointer(&self) -> &str {
        self.pointers.first().map_or("", String::as_str)
    }

    /// What the finding says: where the key is, that the prefix is the
    /// generator's, and that the key is removed.
    pub(crate) fn message(&self) -> String {
        let more = match self.pointers.len() {
            0 | 1 => String::new(),
            2 => " and at one more place".into(),
            n => format!(" and at {} more places", n - 1),
        };
        format!(
            "`{}` at `{}`{more} is a reserved key: the `{PREFIX}` prefix is the generator's own; it is removed",
            self.key,
            self.pointer()
        )
    }

    /// The finding about the key ([`Code::ReservedKey`]).
    pub(crate) fn finding(&self) -> Diagnostic {
        Diagnostic::new(Code::ReservedKey, self.message())
    }
}

/// Whether `key` is reserved, given what `keep` allows.
fn reserved(key: &str, keep: Keep) -> bool {
    key.starts_with(PREFIX) && !(keep == Keep::Markers && key.starts_with(EXTEND_KEY))
}

/// Removes the reserved keys of `value` at any depth, except those `keep`
/// allows; returns them by name, in the order of their first occurrence.
pub(crate) fn strip(value: &mut Value, keep: Keep) -> Vec<Removed> {
    let mut removed = RemovedKeys::new();
    strip_at(value, keep, &mut String::new(), &mut removed);
    removed.keys
}

/// The keys [`strip`] removed so far, with the entry of each name: looking
/// a name up among all removed before took minutes for 40 000 of them.
struct RemovedKeys {
    /// The keys in the order of their first occurrence.
    keys: Vec<Removed>,
    /// The entry in `keys` of each name.
    index: BTreeMap<String, usize>,
}

impl RemovedKeys {
    /// No removed keys yet.
    fn new() -> Self {
        Self {
            keys: Vec::new(),
            index: BTreeMap::new(),
        }
    }
}

/// [`strip`] at `value`, which is at `pointer`.
fn strip_at(value: &mut Value, keep: Keep, pointer: &mut String, removed: &mut RemovedKeys) {
    match value {
        Value::Object(map) => {
            // One pass that keeps the order: removing each key on its own
            // shifted the keys after it, for minutes with 40 000 keys.
            let mut keys: Vec<String> = Vec::new();
            map.retain(|key, _| {
                let keep_it = !reserved(key, keep);
                if !keep_it {
                    keys.push(key.clone());
                }
                keep_it
            });
            for key in keys {
                let at = format!("{pointer}/{}", escape(&key));
                match removed.index.get(&key) {
                    Some(&entry) => {
                        if let Some(entry) = removed.keys.get_mut(entry) {
                            entry.pointers.push(at);
                        }
                    }
                    None => {
                        removed.index.insert(key.clone(), removed.keys.len());
                        removed.keys.push(Removed {
                            key,
                            pointers: vec![at],
                        });
                    }
                }
            }
            for (key, child) in map.iter_mut() {
                let len = pointer.len();
                pointer.push('/');
                pointer.push_str(&escape(key));
                strip_at(child, keep, pointer, removed);
                pointer.truncate(len);
            }
        }
        Value::Array(items) => {
            for (index, item) in items.iter_mut().enumerate() {
                let len = pointer.len();
                pointer.push('/');
                pointer.push_str(&index.to_string());
                strip_at(item, keep, pointer, removed);
                pointer.truncate(len);
            }
        }
        _ => {}
    }
}

/// The first reserved key in `value`, at any depth; nothing is allowed.
pub(crate) fn first(value: &Value) -> Option<&str> {
    match value {
        Value::Object(map) => map
            .keys()
            .find(|key| reserved(key, Keep::Nothing))
            .map(String::as_str)
            .or_else(|| map.values().find_map(first)),
        Value::Array(items) => items.iter().find_map(first),
        _ => None,
    }
}

/// [`strip`] with every removed key reported at `location`.
pub(crate) fn check(
    value: &mut Value,
    keep: Keep,
    location: &Location,
    diagnostics: &mut Diagnostics,
) {
    for removed in strip(value, keep) {
        diagnostics.push(removed.finding().at(location.clone()));
    }
}

/// [`check`] for an object.
pub(crate) fn check_map(
    map: &mut Map<String, Value>,
    keep: Keep,
    location: &Location,
    diagnostics: &mut Diagnostics,
) {
    let mut value = Value::Object(std::mem::take(map));
    check(&mut value, keep, location, diagnostics);
    if let Value::Object(restored) = value {
        *map = restored;
    }
}

/// Removes every key of the prefix from `document`, which is complete:
/// no stage uses them any more. One that is still there came with an
/// imported template whose body a stage did not resolve, or is a bug;
/// it is reported at the location `origin` gives for its pointer.
pub(crate) fn sweep(
    document: &mut Value,
    origin: impl Fn(&str) -> Option<Location>,
    diagnostics: &mut Diagnostics,
) {
    for removed in strip(document, Keep::Nothing) {
        let mut finding = Diagnostic::new(
            Code::ReservedKey,
            format!(
                "the generator's key `{}` was left at `{}` (a marker of an imported template that no stage resolved, or a bug of the generator); it is removed from the document",
                removed.key,
                removed.pointer()
            ),
        );
        if let Some(location) = origin(removed.pointer()) {
            finding = finding.at(location);
        }
        diagnostics.push(finding);
    }
}

/// The line of `text` (0-based) that first writes each reserved key: a
/// key of the prefix, possibly quoted, followed by `:`, also in a flow
/// mapping, as long as it is on one line. One pass over the text: looking
/// for each removed key in all of it took minutes for 40 000 keys.
pub(crate) fn key_lines(text: &str) -> BTreeMap<&str, usize> {
    let mut lines = BTreeMap::new();
    for (index, line) in text.lines().enumerate() {
        for (at, _) in line.match_indices(PREFIX) {
            let rest = line.get(at..).unwrap_or_default();
            let end = rest
                .find(|c: char| {
                    c == ':' || c == '"' || c == '\'' || c.is_whitespace() || ",{}[]".contains(c)
                })
                .unwrap_or(rest.len());
            let (key, after) = rest.split_at(end);
            if after
                .trim_start_matches(['\'', '"'])
                .trim_start()
                .starts_with(':')
            {
                lines.entry(key).or_insert(index);
            }
        }
    }
    lines
}

/// A JSON pointer segment (`~` and `/` escaped, RFC 6901).
fn escape(key: &str) -> String {
    key.replace('~', "~0").replace('/', "~1")
}
