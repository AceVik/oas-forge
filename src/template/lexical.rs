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
    let mut counter = 0usize;
    let mut out = Vec::new();
    for (offset, line) in text.lines().enumerate() {
        let here = location.offset(offset);
        insert_line(
            line,
            registry,
            &here,
            0,
            &mut counter,
            &mut out,
            diagnostics,
        );
    }
    let mut joined = out.join("\n");
    if text.ends_with('\n') {
        // A final line break is content for block scalars (`|`) at the end.
        joined.push('\n');
    }
    let substituted = substitute_variables(&joined, variables);
    substituted.replace("\\$", &ESCAPED_DOLLAR.to_string())
}

fn insert_line(
    line: &str,
    registry: &Registry,
    location: &Location,
    depth: usize,
    counter: &mut usize,
    out: &mut Vec<String>,
    diagnostics: &mut Diagnostics,
) {
    if let Some(caps) = insert_re().captures(line) {
        let indent = &caps[1];
        let name = &caps[2];
        let args = caps.get(3).map(|m| split_args(m.as_str()));
        match registry.fragment(name) {
            Some(fragment) if depth < MAX_INSERT_DEPTH => {
                let provided = args.unwrap_or_default();
                if provided.len() < fragment.params.len() {
                    diagnostics.push(
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
                    let indented = format!("{indent}{inserted}");
                    insert_line(
                        &indented,
                        registry,
                        location,
                        depth + 1,
                        counter,
                        out,
                        diagnostics,
                    );
                }
            }
            Some(_) => diagnostics.push(
                Diagnostic::new(
                    Code::UnknownFragment,
                    format!("`@insert {name}` is nested too deeply"),
                )
                .at(location.clone()),
            ),
            None if args.is_none() => {
                // Documented fallback: an unknown name refers to a reusable parameter.
                out.push(format!(
                    "{indent}- $ref: \"#/components/parameters/{name}\""
                ));
            }
            None => diagnostics.push(
                Diagnostic::new(Code::UnknownFragment, format!("unknown fragment `{name}`"))
                    .at(location.clone()),
            ),
        }
        return;
    }
    if let Some(caps) = extend_re().captures(line) {
        *counter += 1;
        let marker = caps[2].replace('\'', "''");
        out.push(format!("{}{EXTEND_KEY}{}: '{marker}'", &caps[1], *counter));
        return;
    }
    out.push(line.to_string());
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

/// Substitutes known `{{NAME}}` variables. A placeholder that is the whole,
/// unquoted value of a mapping entry or list item is inserted as a quoted
/// string so that values like `1.10` stay strings.
fn substitute_variables(text: &str, variables: &Variables) -> String {
    if variables.is_empty() || !text.contains("{{") {
        return text.to_string();
    }
    text.lines()
        .map(|line| {
            let value_start = value_position(line);
            variable_re()
                .replace_all(line, |caps: &regex::Captures| {
                    let Some(value) = variables.get(&caps[1]) else {
                        return caps[0].to_string();
                    };
                    let whole = caps.get(0).unwrap();
                    let is_whole_value =
                        value_start == Some(whole.start()) && line[whole.end()..].trim().is_empty();
                    if is_whole_value {
                        format!("'{}'", value.replace('\'', "''"))
                    } else {
                        value.clone()
                    }
                })
                .into_owned()
        })
        .collect::<Vec<_>>()
        .join("\n")
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
