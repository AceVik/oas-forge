//! The text-level stage (ADR 0001): `@insert`, `@extend` markers, `{{…}}`
//! variables and `\$` escapes. Its output is parsed as YAML right away.

use super::registry::Registry;
use crate::diagnostics::{Code, Diagnostic, Diagnostics, Location};
use regex::Regex;
use std::collections::BTreeMap;
use std::sync::OnceLock;

/// Stands for an escaped `$` (`\$`) until the very end of the pipeline, so
/// escaped text is never taken for a smart reference.
pub const ESCAPED_DOLLAR: char = '\u{E024}';

/// Prefix of the keys that carry `@extend` markers through YAML parsing.
pub const EXTEND_KEY: &str = "x-oas-forge-extend-";

const MAX_INSERT_DEPTH: usize = 8;

/// Values of `{{NAME}}` placeholders.
pub type Variables = BTreeMap<String, String>;

fn insert_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"^(\s*)(?:-\s+)?@insert\s+([\w]+)\s*(?:\((.*)\))?\s*$").unwrap())
}

fn extend_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"^(\s*)@extend\s+(.+?)\s*$").unwrap())
}

fn variable_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"\{\{\s*([A-Za-z_][\w:.-]*)\s*\}\}").unwrap())
}

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
    let mut expansion = Expansion {
        registry,
        lines: Vec::new(),
        counter: 0,
        diagnostics,
    };
    for (offset, line) in text.lines().enumerate() {
        expansion.line(line, &location.offset(offset), offset, 0);
    }
    let Expansion { lines, .. } = expansion;
    let mut missing = Vec::new();
    let mut joined = lines
        .iter()
        .map(|(line, offset)| substitute_line(line, variables, *offset, &mut missing))
        .collect::<Vec<_>>()
        .join("\n");
    if text.ends_with('\n') {
        // A final line break is content for block scalars (`|`) at the end.
        joined.push('\n');
    }
    report_missing(missing, location, diagnostics);
    joined.replace("\\$", &ESCAPED_DOLLAR.to_string())
}

/// The names of the `{{env:NAME}}` placeholders in `text`, in order.
pub fn environment_variables(text: &str) -> impl Iterator<Item = &str> {
    variable_re()
        .captures_iter(text)
        .filter_map(|caps| caps.get(1)?.as_str().strip_prefix("env:"))
}

/// Substitutes `{{NAME}}` from `variables` and `{{env:NAME}}` from the
/// environment. A placeholder that is the whole, unquoted value of a mapping
/// entry or list item is inserted as a quoted string so that values like
/// `1.10` stay strings. Unset environment variables are kept and reported at
/// their line (`location` is the position of the first line of `text`).
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
    let mut substituted = text
        .lines()
        .enumerate()
        .map(|(offset, line)| substitute_line(line, variables, offset, &mut missing))
        .collect::<Vec<_>>()
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
    let mut reported = std::collections::BTreeSet::new();
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
    diagnostics: &'a mut Diagnostics,
}

impl Expansion<'_> {
    /// Expands one line; `offset` is its source line within the text.
    fn line(&mut self, line: &str, location: &Location, offset: usize, depth: usize) {
        if let Some(caps) = insert_re().captures(line) {
            let indent = &caps[1];
            let name = &caps[2];
            let args = caps.get(3).map(|m| split_args(m.as_str()));
            match self.registry.fragment(name) {
                Some(fragment) if depth < MAX_INSERT_DEPTH => {
                    let provided = args.unwrap_or_default();
                    if provided.len() < fragment.params.len() {
                        self.diagnostics.push(
                            Diagnostic::new(
                                Code::MissingFragmentArgument,
                                format!(
                                    "fragment `{name}` expects {} argument(s), got {}",
                                    fragment.params.len(),
                                    provided.len()
                                ),
                            )
                            .at(location.clone()),
                        );
                    }
                    let body = substitute_params(&fragment.body, &fragment.params, &provided);
                    for inserted in body.lines() {
                        self.line(&format!("{indent}{inserted}"), location, offset, depth + 1);
                    }
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
        if let Some(caps) = extend_re().captures(line) {
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

/// Replaces `{{param}}` by the corresponding argument.
pub fn substitute_params(body: &str, params: &[String], args: &[String]) -> String {
    let mut result = body.to_string();
    for (param, arg) in params.iter().zip(args) {
        result = result.replace(&format!("{{{{{param}}}}}"), arg);
    }
    result
}

/// Substitutes the placeholders of one line; unset environment variables are
/// added to `missing` with `offset`.
fn substitute_line(
    line: &str,
    variables: &Variables,
    offset: usize,
    missing: &mut Vec<(usize, String)>,
) -> String {
    if !line.contains("{{") {
        return line.to_string();
    }
    let value_start = value_position(line);
    variable_re()
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
            let whole = caps.get(0).unwrap();
            let is_whole_value =
                value_start == Some(whole.start()) && line[whole.end()..].trim().is_empty();
            if is_whole_value {
                format!("'{}'", value.replace('\'', "''"))
            } else {
                value
            }
        })
        .into_owned()
}

/// Byte offset where the unquoted value of `key: value` or `- value` starts.
fn value_position(line: &str) -> Option<usize> {
    let trimmed_start = line.len() - line.trim_start().len();
    let rest = &line[trimmed_start..];
    if let Some(item) = rest.strip_prefix("- ") {
        let item_start = trimmed_start + 2 + (item.len() - item.trim_start().len());
        return Some(item_start);
    }
    let colon = rest.find(": ")?;
    let after = &rest[colon + 2..];
    Some(trimmed_start + colon + 2 + (after.len() - after.trim_start().len()))
}
