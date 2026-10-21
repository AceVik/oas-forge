//! The text-level stage (ADR 0001): `@insert`, `@extend` markers, `{{…}}`
//! variables and `\$` escapes. Its output is parsed as YAML right away, or,
//! for route and channel blocks ([`expand_block`]), read by the DSL, which
//! parses only some of the lines as YAML.

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

/// What follows [`EXTEND_KEY`] in the markers of route defaults
/// (`x-oas-forge-extend-defaults-M-N`: module `M`, marker `N`); they apply
/// after the route's own markers, the innermost module first.
pub const DEFAULTS_MARKER: &str = "defaults-";

/// How deeply `@insert`s may nest; a deeper one is skipped and reported.
const MAX_INSERT_DEPTH: usize = 8;

/// Values of `{{NAME}}` placeholders.
pub type Variables = BTreeMap<String, String>;

/// An `@insert Name` or `@insert Name(args)` line, optionally a list item
/// (`- @insert Name`, the dash is not captured); captures the indentation,
/// the name and the argument text.
static INSERT_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"^(\s*)(?:-\s+)?@insert\s+([\w]+)\s*(?:\((.*)\))?\s*$").expect("constant pattern")
});

/// An `@insert` or `@extend` line whose argument is in Markdown backticks
/// (`` @insert `Paging` ``); captures the line up to the argument and the
/// argument.
static TICKED_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"^(\s*(?:-\s+)?@(?:insert|extend)\s+)`([^`]+)`\s*$").expect("constant pattern")
});

/// An `@extend Spec` line; captures the indentation and the spec
/// (`Name` or `Name(args)`).
static EXTEND_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^(\s*)@extend\s+(.+?)\s*$").expect("constant pattern"));

/// A `{{NAME}}` or `{{env:NAME}}` placeholder (spaces inside the braces
/// allowed); captures the name.
static VARIABLE_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"\{\{\s*([A-Za-z_][\w:.-]*)\s*\}\}").expect("constant pattern"));

/// Expands `@insert`, converts `@extend` lines into marker keys, substitutes
/// variables and protects `\$` escapes. `location` is the position of the
/// first line of `text`.
///
/// An `@insert` of an unknown name without arguments stands for a reusable
/// parameter (`- $ref: "#/components/parameters/Name"`).
///
/// # Examples
///
/// ```
/// use oas_forge::diagnostics::{Diagnostics, Location};
/// use oas_forge::template::lexical::{self, ESCAPED_DOLLAR};
/// use oas_forge::template::{Fragment, Registry, Variables};
///
/// let mut registry = Registry::new();
/// registry.add_fragment(
///     "Limit",
///     Fragment { params: vec!["max".into()], body: "maximum: {{max}}".into() },
/// );
/// let mut variables = Variables::new();
/// variables.insert("version".into(), "1.10".into());
/// let text = "\
/// type: integer
/// @insert Limit(100)
/// description: Costs \\$5 since {{version}}
/// x-since: {{version}}
/// @extend Audited
/// ";
/// let mut diagnostics = Diagnostics::new();
/// let location = Location::new("src/lib.rs", 3);
/// let expanded = lexical::expand(text, &registry, &variables, &location, &mut diagnostics);
///
/// let expected = format!(
///     "type: integer\n\
///      maximum: 100\n\
///      description: Costs {ESCAPED_DOLLAR}5 since 1.10\n\
///      x-since: '1.10'\n\
///      x-oas-forge-extend-1: 'Audited'\n"
/// );
/// assert_eq!(expanded, expected);
/// ```
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
///
/// # Examples
///
/// ```
/// use oas_forge::diagnostics::{Diagnostics, Location};
/// use oas_forge::template::{Fragment, Registry, Variables, lexical};
///
/// let mut registry = Registry::new();
/// registry.add_fragment(
///     "Paging",
///     Fragment { params: Vec::new(), body: "limit: 10\noffset: 0".into() },
/// );
/// let mut diagnostics = Diagnostics::new();
/// let lines = lexical::expand_lines(
///     "type: object\n@insert Paging\nx-paged: true",
///     &registry,
///     &Variables::new(),
///     &Location::new("src/lib.rs", 3),
///     &mut diagnostics,
/// );
/// let expected = [
///     ("type: object", 0),
///     ("limit: 10", 1),
///     ("offset: 0", 1),
///     ("x-paged: true", 2),
/// ];
/// let expected: Vec<(String, usize)> =
///     expected.iter().map(|(line, offset)| (line.to_string(), *offset)).collect();
/// assert_eq!(lines, expected);
/// ```
pub fn expand_lines(
    text: &str,
    registry: &Registry,
    variables: &Variables,
    location: &Location,
    diagnostics: &mut Diagnostics,
) -> Vec<(String, usize)> {
    let (lines, offsets) = expand_text(text, registry, variables, location, diagnostics);
    split_lines(&lines, &offsets)
}

/// [`expand_lines`] for a block whose lines are not all YAML, such as a
/// route or channel block: a placeholder that is the whole value is quoted
/// only on the lines that `yaml` marks as YAML; on the others (summary,
/// description and directive lines) the value is inserted as it is.
///
/// `yaml` gets the lines after `@insert`, `@extend` and substitution, with
/// the plain values (as the DSL reads prose and directives), and returns for
/// each line whether it is YAML; lines it leaves out are YAML. Unset
/// environment variables are reported once, like in [`expand_lines`].
///
/// # Examples
///
/// The route DSL tells which lines of a route block are embedded YAML:
///
/// ```
/// use oas_forge::diagnostics::{Diagnostics, Location};
/// use oas_forge::dsl;
/// use oas_forge::template::{Registry, Variables, lexical};
///
/// let mut variables = Variables::new();
/// variables.insert("VERSION".into(), "1.10".into());
/// let text = "\
/// Since: {{VERSION}}
/// @route GET /status
/// @description Version: {{VERSION}}
/// @return 204:
/// x-since: {{VERSION}}";
/// let mut diagnostics = Diagnostics::new();
/// let lines = lexical::expand_block(
///     text,
///     &Registry::new(),
///     &variables,
///     &Location::new("src/lib.rs", 3),
///     &mut diagnostics,
///     dsl::yaml_lines,
/// );
/// let lines: Vec<&str> = lines.iter().map(|(line, _)| line.as_str()).collect();
/// assert_eq!(
///     lines,
///     [
///         "Since: 1.10",
///         "@route GET /status",
///         "@description Version: 1.10",
///         "@return 204:",
///         "x-since: '1.10'",
///     ]
/// );
/// ```
pub fn expand_block(
    text: &str,
    registry: &Registry,
    variables: &Variables,
    location: &Location,
    diagnostics: &mut Diagnostics,
    yaml: impl FnOnce(&[String]) -> Vec<bool>,
) -> Vec<(String, usize)> {
    let (lines, offsets) = paste(text, registry, location, diagnostics);
    let numbered = || {
        lines
            .iter()
            .map(String::as_str)
            .zip(offsets.iter().copied())
    };
    let mut missing = Vec::new();
    let quoted = substitute_lines(numbered(), variables, &mut missing, true);
    let plain = substitute_lines(numbered(), variables, &mut Vec::new(), false);
    report_missing(missing, location, diagnostics);
    // Quoting adds no line breaks: both have the same lines.
    let quoted = split_lines(&quoted, &offsets);
    let plain: Vec<String> = split_lines(&plain, &offsets)
        .into_iter()
        .map(|(line, _)| line)
        .collect();
    let is_yaml = yaml(&plain);
    quoted
        .into_iter()
        .zip(plain)
        .enumerate()
        .map(|(i, ((quoted, offset), plain))| {
            if is_yaml.get(i).copied().unwrap_or(true) {
                (quoted, offset)
            } else {
                (plain, offset)
            }
        })
        .collect()
}

/// Splits substituted `lines` at the line breaks of their values and
/// protects `\$` escapes; each part keeps the offset of its line.
fn split_lines(lines: &[String], offsets: &[usize]) -> Vec<(String, usize)> {
    let mut out = Vec::with_capacity(lines.len());
    for (line, offset) in lines.iter().zip(offsets) {
        // A substituted value may contain line breaks.
        for part in line.split('\n') {
            let part = part.strip_suffix('\r').unwrap_or(part);
            out.push((part.replace("\\$", ESCAPED_DOLLAR_TEXT), *offset));
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
    let (lines, offsets) = paste(text, registry, location, diagnostics);
    let mut missing = Vec::new();
    let substituted = substitute_lines(
        lines
            .iter()
            .map(String::as_str)
            .zip(offsets.iter().copied()),
        variables,
        &mut missing,
        true,
    );
    report_missing(missing, location, diagnostics);
    (substituted, offsets)
}

/// The lines of `text` after `@insert` and `@extend`, with the offset of the
/// source line each comes from.
fn paste(
    text: &str,
    registry: &Registry,
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
    lines.into_iter().unzip()
}

/// The names of the `{{env:NAME}}` placeholders in `text`, in order.
///
/// # Examples
///
/// ```
/// use oas_forge::template::lexical;
///
/// let text = "url: https://{{env:API_HOST}}/{{ env:API_BASE }} ({{version}})";
/// let names: Vec<&str> = lexical::environment_variables(text).collect();
/// assert_eq!(names, ["API_HOST", "API_BASE"]);
/// ```
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
///
/// # Examples
///
/// ```
/// use oas_forge::diagnostics::{Code, Diagnostics, Location};
/// use oas_forge::template::{Variables, lexical};
///
/// let mut variables = Variables::new();
/// variables.insert("version".into(), "1.10".into());
/// let text = "\
/// version: {{version}}
/// title: Shop {{version}}
/// description: |
///   {{version}}
/// host: {{env:OAS_FORGE_EXAMPLE_UNSET}}
/// ";
/// let mut diagnostics = Diagnostics::new();
/// let location = Location::new("src/lib.rs", 3);
/// let substituted = lexical::substitute(text, &variables, &location, &mut diagnostics);
///
/// assert_eq!(
///     substituted,
///     "\
/// version: '1.10'
/// title: Shop 1.10
/// description: |
///   1.10
/// host: {{env:OAS_FORGE_EXAMPLE_UNSET}}
/// "
/// );
/// let finding = diagnostics.iter().next().ok_or("no finding")?;
/// assert_eq!(finding.code(), Code::UnknownVariable);
/// assert_eq!(finding.location(), Some(&Location::new("src/lib.rs", 7)));
/// # Ok::<(), Box<dyn std::error::Error>>(())
/// ```
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
        true,
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
    /// The fragments `@insert` pastes.
    registry: &'a Registry,
    /// Expanded lines with the offset of the source line they come from.
    lines: Vec<(String, usize)>,
    /// Numbers the `@extend` markers.
    counter: usize,
    /// The fragments being inserted, outermost first.
    stack: Vec<String>,
    /// Fragments whose cycle has been reported.
    cycles: BTreeSet<String>,
    /// Receives cycles, argument count mismatches and unknown or too deeply
    /// nested fragments.
    diagnostics: &'a mut Diagnostics,
}

impl Expansion<'_> {
    /// Expands one line; `offset` is its source line within the text. The
    /// argument of `@insert` and `@extend` may be written in backticks.
    fn line(&mut self, line: &str, location: &Location, offset: usize) {
        let line = TICKED_RE.replace(line, "$1$2");
        let line = line.as_ref();
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
/// Arguments are trimmed; blank text has no arguments.
///
/// # Examples
///
/// ```
/// use oas_forge::template::lexical::split_args;
///
/// assert_eq!(split_args(r#"User, "a, b", 3"#), ["User", "a, b", "3"]);
/// assert!(split_args("  ").is_empty());
/// ```
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
/// argument that contains `{{other}}` is inserted as it is. Placeholders
/// without an argument stay.
///
/// # Examples
///
/// ```
/// use oas_forge::template::lexical::substitute_params;
///
/// let params = ["max".to_string(), "min".to_string()];
/// let body = "maximum: {{max}}\nminimum: {{min}}";
/// let args = ["{{min}}".to_string()];
/// assert_eq!(
///     substitute_params(body, &params, &args),
///     "maximum: {{min}}\nminimum: {{min}}"
/// );
/// ```
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
/// line). With `quote`, whole values are quoted except inside block scalars;
/// without, every value is inserted as it is.
fn substitute_lines<'l>(
    lines: impl Iterator<Item = (&'l str, usize)>,
    variables: &Variables,
    missing: &mut Vec<(usize, String)>,
    quote: bool,
) -> Vec<String> {
    // The indentation of the line that opened the current block scalar.
    let mut block: Option<usize> = None;
    lines
        .map(|(line, offset)| {
            if !quote {
                return substitute_line(line, variables, offset, missing, false);
            }
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
