//! The only place that talks to the YAML library (see ADR 0009).
//!
//! Documents are represented as `serde_json::Value` everywhere else; YAML is an
//! input and output format only. The parser options are pinned here:
//!
//! - YAML 1.2 booleans only: `yes`, `no`, `on` and `off` stay strings.
//! - Duplicate keys are errors.
//! - Merge keys (`<<`) are expanded.
//! - `.nan` and `.inf` become the strings `.nan` and `.inf` (JSON has no such numbers).
//! - The default resource budget rejects excessive nesting, and a document
//!   holds at most [`NODES_PER_BYTE`] nodes and [`SCALAR_BYTES_PER_BYTE`]
//!   bytes of scalars per byte of its text (plus [`MIN_NODES`] and
//!   [`MIN_SCALAR_BYTES`]), so aliases cannot expand a small text into a
//!   large document.

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

/// How many nodes (mappings, sequences, scalars) a document may hold per
/// byte of its text. Plain YAML has about one node for every two bytes at
/// most; only aliases reach more. The library's 250 000 nodes are a limit
/// for each document, and a hundred blocks of 690 bytes, each expanding
/// five levels of aliases to 111 111 nodes, took 5.5 GB.
const NODES_PER_BYTE: usize = 4;

/// The nodes a document may hold besides [`NODES_PER_BYTE`].
const MIN_NODES: usize = 64;

/// How many bytes of scalars a document may hold per byte of its text.
/// Without aliases they are at most its text; the library allows 64 MiB
/// for each document, and a 16 KB scalar aliased 4000 times in a block
/// that a route copied for 16 `@route` lines (41 KB) took 6 GB.
const SCALAR_BYTES_PER_BYTE: usize = 16;

/// The bytes of scalars a document may hold besides
/// [`SCALAR_BYTES_PER_BYTE`].
const MIN_SCALAR_BYTES: usize = 1024;

/// The pinned parser options listed in the module documentation, for a
/// text of `len` bytes. Snippets are off: callers report
/// [`YamlError::line`] themselves.
fn options(len: usize) -> Options {
    let mut budget = serde_saphyr::budget::Budget::default();
    budget.max_nodes = budget
        .max_nodes
        .min(len.saturating_mul(NODES_PER_BYTE).saturating_add(MIN_NODES));
    budget.max_total_scalar_bytes = budget.max_total_scalar_bytes.min(
        len.saturating_mul(SCALAR_BYTES_PER_BYTE)
            .saturating_add(MIN_SCALAR_BYTES),
    );
    let mut options = Options::default();
    options.budget = Some(budget);
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
    serde_saphyr::from_str_with_options(text, options(text.len())).map_err(|error| {
        let error = error.without_snippet();
        let mut message: String = DefaultMessageFormatter
            .format_message(error)
            .replace(LIBRARY_HINT, "")
            .chars()
            .map(|c| if c.is_control() { ' ' } else { c })
            .collect();
        // The limits on nodes and scalars are the ones that depend on the
        // text; nesting and the others are the library's.
        if message.contains("budget breached: Nodes")
            || message.contains("budget breached: ScalarBytes")
        {
            message.push_str(&format!(
                "; a document may expand to at most {NODES_PER_BYTE} nodes and {SCALAR_BYTES_PER_BYTE} bytes of scalars per byte of its text, counting the values of YAML anchors each time an alias or merge key repeats them"
            ));
        }
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
                let before = lines
                    .iter()
                    .take(i)
                    .rev()
                    .find(content)
                    .map_or(0, |l| indent(l));
                let after = lines
                    .iter()
                    .skip(i + 1)
                    .find(content)
                    .map_or(0, |l| indent(l));
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
