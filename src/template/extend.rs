//! Structural `@extend`: fragments merged into the mapping that contains the
//! `@extend` line. Local values win; the fragment fills gaps (ADR 0003).
//!
//! A fragment body goes through the same text stage as any other YAML
//! ([`lexical::expand`]): its `@insert`s are pasted, its `@extend`s become
//! markers that are resolved here, and `{{variables}}` are filled in.

use super::lexical::{self, DEFAULTS_MARKER, EXTEND_KEY, Variables, split_args, substitute_params};
use super::registry::Registry;
use crate::diagnostics::{Code, Diagnostic, Diagnostics, Location};
use crate::merge::{Policy, merge};
use crate::reserved::{self, Keep};
use crate::yaml;
use serde_json::Value;
use std::cmp::Reverse;
use std::collections::BTreeSet;

/// How deeply `@extend`s may nest; a deeper one is skipped and reported.
const MAX_DEPTH: usize = 8;

/// Resolves every `@extend` marker in `value`: the marker key is removed and
/// the fragment is merged into the mapping that held it with
/// [`Policy::Extend`] (local scalars win, the fragment fills gaps, arrays are
/// appended). Unknown fragments, cycles, argument count mismatches and
/// bodies that are not valid YAML are reported at `location`.
///
/// # Examples
///
/// ```
/// use oas_forge::diagnostics::{Diagnostics, Location};
/// use oas_forge::template::{Fragment, Registry, Variables, extend, lexical};
/// use serde_json::json;
///
/// let mut registry = Registry::new();
/// registry.add_fragment(
///     "Audited",
///     Fragment {
///         params: Vec::new(),
///         body: "properties:\n  created_at: {type: string}\nrequired: [created_at]".into(),
///     },
/// );
/// // `@extend Audited` after the text stage and YAML parsing:
/// let marker = format!("{}1", lexical::EXTEND_KEY);
/// let mut value = json!({ "type": "object", "properties": { "id": { "type": "integer" } } });
/// value[&marker] = json!("Audited");
///
/// let mut diagnostics = Diagnostics::new();
/// let location = Location::new("src/lib.rs", 3);
/// extend::apply(&mut value, &registry, &Variables::new(), &location, &mut diagnostics);
///
/// assert_eq!(
///     value,
///     json!({
///         "type": "object",
///         "properties": {
///             "id": { "type": "integer" },
///             "created_at": { "type": "string" }
///         },
///         "required": ["created_at"]
///     })
/// );
/// assert!(diagnostics.is_empty());
/// ```
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
        remaining_bytes: lexical::MAX_EXPANSION_BYTES,
        remaining_steps: lexical::MAX_EXPANSION_STEPS,
        exhausted: false,
    };
    extension.apply_at(value);
}

/// The state of an [`apply`] run.
struct Extension<'a> {
    /// The fragments `@extend` merges.
    registry: &'a Registry,
    /// The `{{NAME}}` values for fragment bodies.
    variables: &'a Variables,
    /// The position of the value: this stage reports its findings there,
    /// the text stage of fragment bodies relative to it.
    location: &'a Location,
    /// Receives the findings of this stage and of the text stage of
    /// fragment bodies.
    diagnostics: &'a mut Diagnostics,
    /// The fragments being extended, outermost first.
    stack: Vec<String>,
    /// Fragments whose cycle or depth problem has been reported.
    reported: BTreeSet<String>,
    /// Remaining total text budget for fragment bodies.
    remaining_bytes: usize,
    /// Remaining number of fragment resolutions, including empty bodies.
    remaining_steps: usize,
    /// Whether a budget violation has already been reported.
    exhausted: bool,
}

impl Extension<'_> {
    /// Resolves the markers of `value` and everything below it: nested
    /// mappings first, then the fragments of this mapping, each with its
    /// own markers resolved before it is merged.
    fn apply_at(&mut self, value: &mut Value) {
        match value {
            Value::Object(map) => {
                let mut markers: Vec<String> = map
                    .keys()
                    .filter(|k| k.starts_with(EXTEND_KEY))
                    .cloned()
                    .collect();
                markers.sort_by_key(|key| marker_order(key));
                let mut fragments = Vec::new();
                for key in markers {
                    // The lexical stage writes the fragment as a string; a
                    // marker with another value is not its own (a source
                    // wrote it) and stays, for the sweep of the finished
                    // document.
                    if let Some(spec) = map.get(&key).and_then(Value::as_str).map(str::to_string) {
                        map.remove(&key);
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

    /// Reports a finding at [`Extension::location`].
    fn report(&mut self, code: Code, message: String) {
        self.diagnostics
            .push(Diagnostic::new(code, message).at(self.location.clone()));
    }

    /// `Name` or `Name(args)` → the fragment's name and body as a value.
    fn resolve(&mut self, spec: &str) -> Option<(String, Value)> {
        if self.exhausted {
            return None;
        }
        if self.remaining_steps == 0 {
            self.exhaust();
            return None;
        }
        self.remaining_steps -= 1;
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
        if let Some(finding) = lexical::argument_count(name, fragment.params.len(), args.len()) {
            self.diagnostics.push(finding.at(self.location.clone()));
        }
        // The body is a copy: it counts for the run too, with each line.
        let lines = fragment.body.split('\n').count();
        let Some(size) =
            lexical::substituted_size(&fragment.body, &fragment.params, &args).filter(|size| {
                *size <= self.remaining_bytes
                    && lexical::charge_run(
                        lines
                            .saturating_mul(lexical::PASTED_LINE_COST)
                            .saturating_add(*size),
                    )
            })
        else {
            self.exhaust();
            return None;
        };
        self.remaining_bytes -= size;
        let body = substitute_params(&fragment.body, &fragment.params, &args);
        let body = lexical::expand(
            &body,
            registry,
            self.variables,
            self.location,
            self.diagnostics,
        );
        if body.len() > self.remaining_bytes {
            self.exhaust();
            return None;
        }
        self.remaining_bytes -= body.len();
        match yaml::parse(&body) {
            Ok(mut value) => {
                for removed in reserved::strip(&mut value, Keep::Markers) {
                    self.report(
                        Code::ReservedKey,
                        format!("fragment `{name}`: {}", removed.message()),
                    );
                }
                Some((name.to_string(), value))
            }
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

    /// Stops resolving fragments after the first resource budget violation.
    fn exhaust(&mut self) {
        self.exhausted = true;
        self.report(
            Code::ExpansionLimit,
            "fragment extensions exceed 16 MiB of processed text or 65536 resolutions, or the fragment expansions of the run together exceed 32 MiB".into(),
        );
    }
}

/// The order in which the markers of one mapping apply; the first fragment
/// wins conflicts. The mapping's own markers come in line order, then those
/// of module defaults (`defaults-M-N`), the innermost module first.
fn marker_order(key: &str) -> (bool, Reverse<usize>, usize) {
    let number = |text: &str| text.parse().unwrap_or(usize::MAX);
    let suffix = key.strip_prefix(EXTEND_KEY).unwrap_or(key);
    match suffix
        .strip_prefix(DEFAULTS_MARKER)
        .and_then(|rest| rest.split_once('-'))
    {
        Some((module, line)) => (true, Reverse(number(module)), number(line)),
        None => (false, Reverse(0), number(suffix)),
    }
}
