//! The only place that talks to the YAML library (see ADR 0009).
//!
//! Documents are represented as `serde_json::Value` everywhere else; YAML is an
//! input and output format only. The parser options are pinned here:
//!
//! - YAML 1.2 booleans only: `yes`, `no`, `on` and `off` stay strings.
//! - Duplicate keys are errors.
//! - Merge keys (`<<`) are expanded.
//! - `.nan` and `.inf` become the strings `.nan` and `.inf` (JSON has no such numbers).
//! - The default resource budget rejects alias bombs and excessive nesting.

use serde_json::Value;
use serde_saphyr::{DefaultMessageFormatter, MessageFormatter, NonFiniteFloatPolicy, Options};

/// A YAML syntax error.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct YamlError {
    /// The parser message, without position.
    pub message: String,
    /// 1-based line of the problem inside the parsed text.
    pub line: usize,
}

/// Advice in parser messages that refers to the library's options, which
/// users of this crate cannot set.
const LIBRARY_HINT: &str = ", set DuplicateKeyPolicy in Options if acceptable";

fn options() -> Options {
    let mut options = Options::default();
    options.strict_booleans = true;
    options.duplicate_keys = serde_saphyr::DuplicateKeyPolicy::Error;
    options.merge_keys = serde_saphyr::MergeKeyPolicy::Merge;
    options.non_finite_float_policy = NonFiniteFloatPolicy::AsString;
    options.with_snippet = false;
    options
}

/// Parses one YAML (or JSON) document; empty documents are `null`.
///
/// Mapping keys that are numbers or booleans (`200:`) become their text, so
/// every document is representable as JSON.
pub(crate) fn parse(text: &str) -> Result<Value, YamlError> {
    serde_saphyr::from_str_with_options(text, options()).map_err(|error| {
        let error = error.without_snippet();
        let message: String = DefaultMessageFormatter
            .format_message(error)
            .replace(LIBRARY_HINT, "")
            .chars()
            .map(|c| if c.is_control() { ' ' } else { c })
            .collect();
        YamlError {
            message,
            line: error
                .location()
                .and_then(|l| usize::try_from(l.line()).ok())
                .filter(|line| *line > 0)
                .unwrap_or(1),
        }
    })
}

/// Serializes a value as a YAML document.
///
/// Blank lines inside block scalars are written without indentation, so
/// generated files have no trailing whitespace; this is only done when the
/// result still reads back as `value`.
pub(crate) fn to_string(value: &Value) -> String {
    // Serializing a `serde_json::Value` cannot fail: keys are strings and
    // numbers are finite.
    let text = serde_saphyr::to_string(value).unwrap_or_default();
    if !text
        .lines()
        .any(|line| !line.is_empty() && line.trim().is_empty())
    {
        return text;
    }
    let lines: Vec<&str> = text.split_inclusive('\n').collect();
    let indent = |line: &str| line.len() - line.trim_start_matches(' ').len();
    let content = |line: &&&str| !line.trim().is_empty();
    let tidy: String = lines
        .iter()
        .enumerate()
        .map(|(i, line)| {
            if line.trim().is_empty() && line.len() > 1 {
                // Spaces up to the block's indentation are layout, more are content.
                let before = lines[..i]
                    .iter()
                    .rev()
                    .find(content)
                    .map_or(0, |l| indent(l));
                let after = lines[i + 1..].iter().find(content).map_or(0, |l| indent(l));
                if line.trim_end_matches(['\r', '\n']).len() <= before.min(after) {
                    return line.trim_start_matches(' ');
                }
            }
            line
        })
        .collect();
    if parse(&tidy).is_ok_and(|parsed| parsed == *value) {
        tidy
    } else {
        text
    }
}
