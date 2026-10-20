//! The text-level stage (ADR 0001): `@insert`, `@extend` markers, `{{…}}`
//! variables and `\$` escapes. Its output is parsed as YAML right away.

use super::registry::Registry;
use crate::diagnostics::{Code, Diagnostic, Diagnostics, Location};
use regex::Regex;
use std::collections::{BTreeMap, BTreeSet};
use std::sync::LazyLock;

/// Stands for an escaped `$` (`\$`) until the very end of the pipeline, so
/// escaped text is never taken for a smart reference.
pub const ESCAPED_DOLLAR: char = '\u{E024}';

/// [`ESCAPED_DOLLAR`] as text.
const ESCAPED_DOLLAR_TEXT: &str = "\u{E024}";

/// Prefix of the keys that carry `@extend` markers through YAML parsing.
pub const EXTEND_KEY: &str = "x-oas-forge-extend-";

const MAX_INSERT_DEPTH: usize = 8;

/// Values of `{{NAME}}` placeholders.
pub type Variables = BTreeMap<String, String>;

static INSERT_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"^(\s*)(?:-\s+)?@insert\s+([\w]+)\s*(?:\((.*)\))?\s*$").expect("constant pattern")
});

static EXTEND_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^(\s*)@extend\s+(.+?)\s*$").expect("constant pattern"));

static VARIABLE_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"\{\{\s*([A-Za-z_][\w:.-]*)\s*\}\}").expect("constant pattern"));

/// Expands `@insert`, converts `@extend` lines into marker keys, substitutes
/// variables and protects `\$` escapes. `location` is the position of the
/// first line of `text`.
pub fn expand(
    text: &str,
    registry: &Registry,
    variables: &Variables,
    location: &Location,
    diagnostics: &mut Diagnostics,
) -> String {
    let (lines, _) = expand_text(text, registry, variables, location, diagnostics);
    let mut joined = lines.join("\n");
    if text.ends_with('\n') {
        // A final line break is content for block scalars (`|`) at the end.
        joined.push('\n');
    }
    joined.replace("\\$", ESCAPED_DOLLAR_TEXT)
}

/// [`expand`] line by line: the lines of the result (as `str::lines` splits
/// them), each with the 0-based offset of the line of `text` it comes from.
/// Lines pasted by `@insert` come from the `@insert` line, so findings about
/// them can point there.
pub fn expand_lines(
    text: &str,
    registry: &Registry,
    variables: &Variables,
    location: &Location,
    diagnostics: &mut Diagnostics,
) -> Vec<(String, usize)> {
    let (lines, offsets) = expand_text(text, registry, variables, location, diagnostics);
    let mut out = Vec::with_capacity(lines.len());
    for (line, offset) in lines.iter().zip(offsets) {
        // A substituted value may contain line breaks.
        for part in line.split('\n') {
            let part = part.strip_suffix('\r').unwrap_or(part);
            out.push((part.replace("\\$", ESCAPED_DOLLAR_TEXT), offset));
        }
    }
    out
}

/// The expanded and substituted lines of `text` with their source offsets.
fn expand_text(
    text: &str,
    registry: &Registry,
    variables: &Variables,
    location: &Location,
    diagnostics: &mut Diagnostics,
) -> (Vec<String>, Vec<usize>) {
    let mut expansion = Expansion {
        registry,
        lines: Vec::new(),
        counter: 0,
        stack: Vec::new(),
        cycles: BTreeSet::new(),
        diagnostics,
    };
    for (offset, line) in text.lines().enumerate() {
        expansion.line(line, &location.offset(offset), offset);
    }
    let Expansion { lines, .. } = expansion;
    let mut missing = Vec::new();
    let substituted = substitute_lines(
        lines.iter().map(|(line, offset)| (line.as_str(), *offset)),
        variables,
        &mut missing,
    );
    report_missing(missing, location, diagnostics);
    let offsets = lines.into_iter().map(|(_, offset)| offset).collect();
    (substituted, offsets)
}

/// The names of the `{{env:NAME}}` placeholders in `text`, in order.
pub fn environment_variables(text: &str) -> impl Iterator<Item = &str> {
    VARIABLE_RE
        .captures_iter(text)
        .filter_map(|caps| caps.get(1)?.as_str().strip_prefix("env:"))
}

/// Substitutes `{{NAME}}` from `variables` and `{{env:NAME}}` from the
/// environment. A placeholder that is the whole, unquoted value of a mapping
/// entry or list item is inserted as a quoted string so that values like
/// `1.10` stay strings; inside block scalars (`|`, `>`) text is inserted as
/// it is. Unset environment variables are kept and reported at their line
/// (`location` is the position of the first line of `text`).
pub fn substitute(
    text: &str,
    variables: &Variables,
    location: &Location,
    diagnostics: &mut Diagnostics,
) -> String {
    if !text.contains("{{") {
        return text.to_string();
    }
    let mut missing = Vec::new();
    let mut substituted = substitute_lines(
        text.lines()
            .enumerate()
            .map(|(offset, line)| (line, offset)),
        variables,
        &mut missing,
    )
    .join("\n");
    if text.ends_with('\n') {
        substituted.push('\n');
    }
    report_missing(missing, location, diagnostics);
    substituted
}

/// One diagnostic per unset environment variable, at its first use.
fn report_missing(
    mut missing: Vec<(usize, String)>,
    location: &Location,
    diagnostics: &mut Diagnostics,
) {
    missing.sort();
    let mut reported = BTreeSet::new();
    for (offset, variable) in missing {
        if reported.insert(variable.clone()) {
            diagnostics.push(
                Diagnostic::new(
                    Code::UnknownVariable,
                    format!("environment variable `{variable}` is not set; `{{{{env:{variable}}}}}` is kept"),
                )
                .at(location.offset(offset)),
            );
        }
    }
}

/// The state of an [`expand`] run.
struct Expansion<'a> {
    registry: &'a Registry,
    /// Expanded lines with the offset of the source line they come from.
    lines: Vec<(String, usize)>,
    /// Numbers the `@extend` markers.
    counter: usize,
    /// The fragments being inserted, outermost first.
    stack: Vec<String>,
    /// Fragments whose cycle has been reported.
    cycles: BTreeSet<String>,
    diagnostics: &'a mut Diagnostics,
}

impl Expansion<'_> {
    /// Expands one line; `offset` is its source line within the text.
    fn line(&mut self, line: &str, location: &Location, offset: usize) {
        if let Some(caps) = INSERT_RE.captures(line) {
            let indent = &caps[1];
            let name = &caps[2];
            let args = caps.get(3).map(|m| split_args(m.as_str()));
            match self.registry.fragment(name) {
                Some(_) if self.stack.iter().any(|n| n == name) => {
                    if self.cycles.insert(name.to_string()) {
                        self.diagnostics.push(
                            Diagnostic::new(
                                Code::FragmentCycle,
                                format!(
                                    "fragment `{name}` inserts itself ({} → {name}); the insert is skipped",
                                    self.stack.join(" → ")
                                ),
                            )
                            .at(location.clone()),
                        );
                    }
                }
                Some(fragment) if self.stack.len() < MAX_INSERT_DEPTH => {
                    let provided = args.unwrap_or_default();
                    if let Some(finding) =
                        argument_count(name, fragment.params.len(), provided.len())
                    {
                        self.diagnostics.push(finding.at(location.clone()));
                    }
                    let body = substitute_params(&fragment.body, &fragment.params, &provided);
                    self.stack.push(name.to_string());
                    for inserted in body.lines() {
                        self.line(&format!("{indent}{inserted}"), location, offset);
                    }
                    self.stack.pop();
                }
                Some(_) => self.diagnostics.push(
                    Diagnostic::new(
                        Code::UnknownFragment,
                        format!("`@insert {name}` is nested too deeply"),
                    )
                    .at(location.clone()),
                ),
                None if args.is_none() => {
                    // Documented fallback: an unknown name refers to a reusable parameter.
                    self.lines.push((
                        format!("{indent}- $ref: \"#/components/parameters/{name}\""),
                        offset,
                    ));
                }
                None => self.diagnostics.push(
                    Diagnostic::new(Code::UnknownFragment, format!("unknown fragment `{name}`"))
                        .at(location.clone()),
                ),
            }
            return;
        }
        if let Some(caps) = EXTEND_RE.captures(line) {
            self.counter += 1;
            let marker = caps[2].replace('\'', "''");
            self.lines.push((
                format!("{}{EXTEND_KEY}{}: '{marker}'", &caps[1], self.counter),
                offset,
            ));
            return;
        }
        self.lines.push((line.to_string(), offset));
    }
}

/// A finding when a fragment with `expected` parameters gets `got` arguments:
/// missing ones stay `{{param}}`, extra ones are ignored.
pub(crate) fn argument_count(name: &str, expected: usize, got: usize) -> Option<Diagnostic> {
    let code = match got.cmp(&expected) {
        std::cmp::Ordering::Less => Code::MissingFragmentArgument,
        std::cmp::Ordering::Equal => return None,
        std::cmp::Ordering::Greater => Code::ExtraFragmentArgument,
    };
    Some(Diagnostic::new(
        code,
        format!("fragment `{name}` expects {expected} argument(s), got {got}"),
    ))
}

/// Splits `a, "b, c", d` at commas outside quotes and removes the quotes.
pub fn split_args(text: &str) -> Vec<String> {
    if text.trim().is_empty() {
        return Vec::new();
    }
    let mut args = Vec::new();
    let mut current = String::new();
    let mut quoted = false;
    for c in text.chars() {
        match c {
            '"' => quoted = !quoted,
            ',' if !quoted => {
                args.push(current.trim().to_string());
                current.clear();
            }
            _ => current.push(c),
        }
    }
    args.push(current.trim().to_string());
    args
}

/// Replaces `{{param}}` by the corresponding argument, in one pass: an
/// argument that contains `{{other}}` is inserted as it is.
pub fn substitute_params(body: &str, params: &[String], args: &[String]) -> String {
    if params.is_empty() || !body.contains("{{") {
        return body.to_string();
    }
    VARIABLE_RE
        .replace_all(body, |caps: &regex::Captures| {
            let name = &caps[1];
            params
                .iter()
                .position(|param| param == name)
                .and_then(|i| args.get(i))
                .map_or_else(|| caps[0].to_string(), Clone::clone)
        })
        .into_owned()
}

/// Substitutes the placeholders of `lines` (with the offset of their source
/// line); whole values are quoted except inside block scalars.
fn substitute_lines<'l>(
    lines: impl Iterator<Item = (&'l str, usize)>,
    variables: &Variables,
    missing: &mut Vec<(usize, String)>,
) -> Vec<String> {
    // The indentation of the line that opened the current block scalar.
    let mut block: Option<usize> = None;
    lines
        .map(|(line, offset)| {
            let indent = line.len() - line.trim_start().len();
            if block.is_some_and(|parent| line.trim().is_empty() || indent > parent) {
                return substitute_line(line, variables, offset, missing, false);
            }
            block = opens_block_scalar(line).then_some(indent);
            substitute_line(line, variables, offset, missing, true)
        })
        .collect()
}

/// Whether the value of a line is a block scalar indicator (`|`, `>-`, `|2`).
fn opens_block_scalar(line: &str) -> bool {
    let Some(value) = value_position(line).and_then(|start| line.get(start..)) else {
        return false;
    };
    let value = value.split(" #").next().unwrap_or(value).trim_end();
    value.strip_prefix(['|', '>']).is_some_and(|rest| {
        rest.chars()
            .all(|c| c.is_ascii_digit() || c == '+' || c == '-')
    })
}

/// Substitutes the placeholders of one line; unset environment variables are
/// added to `missing` with `offset`. With `quote`, a placeholder that is the
/// whole value is inserted as a quoted string.
fn substitute_line(
    line: &str,
    variables: &Variables,
    offset: usize,
    missing: &mut Vec<(usize, String)>,
    quote: bool,
) -> String {
    if !line.contains("{{") {
        return line.to_string();
    }
    let value_start = value_position(line).filter(|_| quote);
    VARIABLE_RE
        .replace_all(line, |caps: &regex::Captures| {
            let name = &caps[1];
            let value = match name.strip_prefix("env:") {
                Some(variable) => {
                    let value = std::env::var(variable).ok();
                    if value.is_none() {
                        missing.push((offset, variable.to_string()));
                    }
                    value
                }
                None => variables.get(name).cloned(),
            };
            let Some(value) = value else {
                return caps[0].to_string();
            };
            let whole = caps.get_match();
            let is_whole_value = value_start == Some(whole.start())
                && line
                    .get(whole.end()..)
                    .is_some_and(|after| after.trim().is_empty());
            if is_whole_value {
                format!("'{}'", value.replace('\'', "''"))
            } else {
                value
            }
        })
        .into_owned()
}

/// Byte offset where the unquoted value of `key: value`, `- value` or
/// `- key: value` starts.
fn value_position(line: &str) -> Option<usize> {
    let mut rest = line.trim_start();
    let mut item = false;
    while let Some(after) = rest.strip_prefix("- ") {
        rest = after.trim_start();
        item = true;
    }
    let value = match rest.split_once(": ") {
        Some((_, value)) => value,
        None if item => rest,
        None => return None,
    };
    // `value` is a suffix of `line`.
    Some(line.len() - value.trim_start().len())
}
