//! Doc comments as lines with source positions.

use proc_macro2::Span;
use syn::spanned::Spanned;
use syn::{Attribute, Expr, ExprLit, Lit, Meta};

/// One line of documentation and the source line it came from.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct DocLine {
    /// The text after `///` / `//!` (leading space included).
    pub text: String,
    /// 1-based source line.
    pub line: usize,
}

impl DocLine {
    /// A doc line with `text` from the 1-based source `line`.
    pub(crate) fn new(text: impl Into<String>, line: usize) -> Self {
        Self {
            text: text.into(),
            line,
        }
    }

    /// The text without leading and trailing whitespace.
    pub(crate) fn trimmed(&self) -> &str {
        self.text.trim()
    }
}

/// The value of `#[doc = "..."]` if the attribute is a doc string.
pub(crate) fn doc_text(attr: &Attribute) -> Option<String> {
    if !attr.path().is_ident("doc") {
        return None;
    }
    match &attr.meta {
        Meta::NameValue(nv) => match &nv.value {
            Expr::Lit(ExprLit {
                lit: Lit::Str(s), ..
            }) => Some(s.value()),
            _ => None,
        },
        _ => None,
    }
}

/// Line of a span (1-based).
pub(crate) fn line_of(span: Span) -> usize {
    span.start().line.max(1)
}

/// The doc lines of an item, split at embedded newlines (`/** */` comments).
pub(crate) fn doc_lines(attrs: &[Attribute]) -> Vec<DocLine> {
    attrs
        .iter()
        .filter_map(|attr| Some((doc_text(attr)?, line_of(attr.span()))))
        .flat_map(|(text, line)| {
            doc_parts(&text)
                .into_iter()
                .enumerate()
                .map(|(i, part)| DocLine::new(part, line + i))
                .collect::<Vec<_>>()
        })
        .collect()
}

/// The lines of one doc attribute. A `/** */` comment is split at its
/// newlines; when its lines after the first all start with `*` (Javadoc
/// style), that column is removed, like rustdoc does.
pub(crate) fn doc_parts(text: &str) -> Vec<&str> {
    let parts: Vec<&str> = text.split('\n').map(|p| p.trim_end_matches('\r')).collect();
    let mut rest = parts
        .iter()
        .skip(1)
        .filter(|p| !p.trim().is_empty())
        .peekable();
    let decorated = rest.peek().is_some() && rest.all(|p| p.trim_start().starts_with('*'));
    if !decorated {
        return parts;
    }
    parts
        .into_iter()
        .map(|p| p.trim_start().strip_prefix('*').unwrap_or(p))
        .collect()
}

/// Removes the indentation (spaces) shared by all non-blank lines.
///
/// Works on characters, so wide whitespace such as U+3000 cannot split a
/// UTF-8 sequence.
pub(crate) fn unindent(lines: &[DocLine]) -> Vec<DocLine> {
    let indent = lines
        .iter()
        .filter(|l| !l.text.trim().is_empty())
        .map(|l| l.text.chars().take_while(|c| *c == ' ').count())
        .min()
        .unwrap_or(0);
    lines
        .iter()
        .map(|l| {
            let strip = l.text.chars().take_while(|c| *c == ' ').count().min(indent);
            DocLine::new(l.text.chars().skip(strip).collect::<String>(), l.line)
        })
        .collect()
}

/// Whether `trimmed`, a line at the top level of a YAML block, can start
/// YAML: a key (`name:`, a word before the colon), a list item, a comment, a
/// flow collection or its end, a quoted key, a document marker or
/// `@insert`/`@extend`. Prose such as `Some prose: with a colon.` or a
/// Markdown bullet `* item` cannot.
pub(crate) fn starts_yaml(trimmed: &str) -> bool {
    if trimmed.starts_with([
        '-', '#', '{', '}', '[', ']', '"', '\'', '?', '&', '!', '%', '@',
    ]) {
        return true;
    }
    trimmed.split_once(':').is_some_and(|(key, rest)| {
        !key.is_empty()
            && !key.contains(char::is_whitespace)
            && (rest.is_empty() || rest.starts_with(char::is_whitespace))
    })
}

/// Joins lines with `\n`.
pub(crate) fn join(lines: &[DocLine]) -> String {
    lines
        .iter()
        .map(|l| l.text.as_str())
        .collect::<Vec<_>>()
        .join("\n")
}

/// Turns documentation lines into a description, with rustdoc's paragraph
/// semantics: lines of a paragraph are joined with spaces, paragraphs with a
/// blank line. Markdown that depends on line breaks keeps them: fenced code
/// blocks (verbatim), list items and the line after a heading.
pub(crate) fn description(lines: &[&str]) -> String {
    // The indentation (spaces) shared by all non-blank lines.
    let indent = lines
        .iter()
        .filter(|l| !l.trim().is_empty())
        .map(|l| l.len() - l.trim_start_matches(' ').len())
        .min()
        .unwrap_or(0);
    let mut out = String::new();
    let mut fence: Option<&str> = None;
    let mut blank = false;
    let mut in_list = false;
    let mut after_heading = false;
    for line in lines {
        let text = line.get(indent..).unwrap_or_default().trim_end();
        let trimmed = text.trim();
        if let Some(marker) = fence {
            out.push('\n');
            out.push_str(text);
            if trimmed.starts_with(marker) {
                fence = None;
                after_heading = true;
            }
            continue;
        }
        if trimmed.is_empty() {
            blank = !out.is_empty();
            in_list = false;
            after_heading = false;
            continue;
        }
        let opens = fence_marker(trimmed);
        let heading = is_heading(trimmed);
        let item = is_list_item(trimmed);
        let separator = if out.is_empty() {
            ""
        } else if blank {
            "\n\n"
        } else if opens.is_some() || heading || item || in_list || after_heading {
            "\n"
        } else {
            " "
        };
        out.push_str(separator);
        blank = false;
        after_heading = heading;
        in_list |= item;
        if opens.is_some() {
            fence = opens;
            out.push_str(text);
        } else if in_list {
            // Nested items and continuation lines keep their indentation.
            out.push_str(text);
        } else {
            out.push_str(trimmed);
        }
    }
    out
}

/// The marker of a line that opens a fenced code block (`` ``` ``, `~~~`).
fn fence_marker(trimmed: &str) -> Option<&'static str> {
    ["```", "~~~"]
        .into_iter()
        .find(|marker| trimmed.starts_with(marker))
}

/// `# Title` … `###### Title`.
fn is_heading(trimmed: &str) -> bool {
    let rest = trimmed.trim_start_matches('#');
    let level = trimmed.len() - rest.len();
    (1..=6).contains(&level) && (rest.is_empty() || rest.starts_with(' '))
}

/// `- item`, `* item`, `+ item`, `1. item`, `1) item`.
fn is_list_item(trimmed: &str) -> bool {
    if ["- ", "* ", "+ "].iter().any(|m| trimmed.starts_with(m)) {
        return true;
    }
    let rest = trimmed.trim_start_matches(|c: char| c.is_ascii_digit());
    rest.len() < trimmed.len() && (rest.starts_with(". ") || rest.starts_with(") "))
}
