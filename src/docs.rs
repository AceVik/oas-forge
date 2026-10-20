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
    pub(crate) fn new(text: impl Into<String>, line: usize) -> Self {
        Self {
            text: text.into(),
            line,
        }
    }

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
            text.split('\n')
                .enumerate()
                .map(|(i, part)| DocLine::new(part.trim_end_matches('\r'), line + i))
                .collect::<Vec<_>>()
        })
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

/// Joins lines with `\n`.
pub(crate) fn join(lines: &[DocLine]) -> String {
    lines
        .iter()
        .map(|l| l.text.as_str())
        .collect::<Vec<_>>()
        .join("\n")
}

/// Turns documentation lines into a description: lines of a paragraph are
/// joined with spaces, paragraphs with a blank line.
pub(crate) fn description(lines: &[&str]) -> String {
    let mut paragraphs: Vec<String> = Vec::new();
    let mut current: Vec<&str> = Vec::new();
    for line in lines {
        let line = line.trim();
        if line.is_empty() {
            if !current.is_empty() {
                paragraphs.push(current.join(" "));
                current.clear();
            }
        } else {
            current.push(line);
        }
    }
    if !current.is_empty() {
        paragraphs.push(current.join(" "));
    }
    paragraphs.join("\n\n")
}
