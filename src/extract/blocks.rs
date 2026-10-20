//! Free annotation blocks: `@openapi…` sections in item docs and file-level
//! (`//!`) blocks.
//!
//! A file-level block starts at a header line (`@openapi…`, `@route`) and ends
//! at the next header, at a non-doc attribute, at a separator line such as
//! `//! -----`, or where the doc comment is interrupted by a non-doc line.

use super::{DocumentItem, FragmentItem, Item, NamedSchemaItem, RouteItem};
use crate::diagnostics::{Code, Diagnostic, Diagnostics, Location};
use crate::docs::{DocLine, doc_text, join, line_of, unindent};
use std::path::Path;
use syn::spanned::Spanned;

/// The `@openapi…` sections of an item's doc comment (functions, methods, modules).
///
/// Documentation before the first header describes the Rust item and is ignored.
pub(super) fn item_blocks(
    path: &Path,
    lines: &[DocLine],
    tags: &[String],
    diagnostics: &mut Diagnostics,
) -> Vec<Item> {
    if !lines.iter().any(|l| l.trimmed().starts_with("@openapi")) {
        return Vec::new();
    }
    let lines = unindent(lines);
    let mut items = Vec::new();
    let mut header: Option<DocLine> = None;
    let mut body: Vec<DocLine> = Vec::new();

    for line in lines {
        let trimmed = line.trimmed();
        if trimmed.starts_with("@openapi") {
            if let Some(h) = header.take() {
                section(path, &h, &body, tags, diagnostics, &mut items);
            }
            header = Some(DocLine::new(trimmed, line.line));
            body.clear();
        } else if trimmed.starts_with('{') && header.is_none() {
            header = Some(DocLine::new("@json", line.line));
            body.clear();
            body.push(line);
        } else if header.is_some() {
            body.push(line);
        }
    }
    if let Some(h) = header {
        section(path, &h, &body, tags, diagnostics, &mut items);
    }
    items
}

/// Turns one `@openapi…` section into an item.
fn section(
    path: &Path,
    header: &DocLine,
    body: &[DocLine],
    tags: &[String],
    diagnostics: &mut Diagnostics,
    items: &mut Vec<Item>,
) {
    let text = join(body).trim().to_string();
    let first_body_line = body
        .iter()
        .find(|l| !l.text.trim().is_empty())
        .map_or(header.line, |l| l.line);
    let location = Location::new(path, first_body_line);
    let head = header.text.as_str();

    if let Some(rest) = head.strip_prefix("@openapi-fragment") {
        let (name, params) = signature(rest.trim());
        items.push(Item::Fragment(FragmentItem {
            name,
            params,
            body: text,
            location: Location::new(path, header.line),
        }));
    } else if let Some(name) = head.strip_prefix("@openapi-type") {
        if !text.is_empty() {
            items.push(Item::NamedSchema(NamedSchemaItem {
                name: name.trim().to_string(),
                text,
                location,
            }));
        }
    } else if head.starts_with("@openapi<") || (head.starts_with("@openapi") && head.contains('<'))
    {
        diagnostics.push(
            Diagnostic::new(
                Code::UnnamedBlueprint,
                "`@openapi<T>` needs a struct or enum to name the template; the block is ignored",
            )
            .at(Location::new(path, header.line)),
        );
    } else if !text.is_empty() {
        items.push(Item::Document(DocumentItem {
            text,
            location,
            tags: tags.to_vec(),
        }));
    }
}

/// `Name(a, b)` → (`Name`, [`a`, `b`]); `Name` → (`Name`, []).
fn signature(text: &str) -> (String, Vec<String>) {
    match text.find('(') {
        Some(open) => {
            let params = text[open + 1..]
                .trim_end_matches(')')
                .split(',')
                .map(|p| p.trim().to_string())
                .filter(|p| !p.is_empty())
                .collect();
            (text[..open].trim().to_string(), params)
        }
        None => (text.to_string(), Vec::new()),
    }
}

/// Tags declared in a module's doc comment (`tags: [A, B]`).
pub(super) fn module_tags(lines: &[DocLine]) -> Vec<String> {
    let mut tags = Vec::new();
    for line in lines {
        if !line.text.contains("tags:") {
            continue;
        }
        let Some(open) = line.text.find('[') else {
            continue;
        };
        let list = &line.text[open + 1..];
        if let Some(close) = list.find(']') {
            tags.extend(
                list[..close]
                    .split(',')
                    .map(|t| t.trim().to_string())
                    .filter(|t| !t.is_empty()),
            );
        }
    }
    tags
}

/// The file-level (`//!`) blocks of a file.
pub(super) fn file_blocks(
    path: &Path,
    attrs: &[syn::Attribute],
    diagnostics: &mut Diagnostics,
) -> Vec<Item> {
    let mut items = Vec::new();
    let mut block: Vec<DocLine> = Vec::new();
    let mut last_line: Option<usize> = None;

    for attr in attrs {
        if !attr.path().is_ident("doc") {
            close(path, &mut block, &mut items, diagnostics);
            last_line = None;
            continue;
        }
        let start = line_of(attr.span());
        let Some(text) = doc_text(attr) else {
            // Non-literal doc attributes keep the comment contiguous.
            last_line = Some(attr.span().end().line.max(start));
            continue;
        };
        for (offset, part) in text.split('\n').enumerate() {
            let line = DocLine::new(part.trim_end_matches('\r'), start + offset);
            if last_line.is_some_and(|last| line.line > last + 1) {
                close(path, &mut block, &mut items, diagnostics);
            }
            last_line = Some(line.line);

            let trimmed = line.trimmed();
            if is_header(trimmed) {
                close(path, &mut block, &mut items, diagnostics);
                block.push(line);
            } else if is_separator(trimmed) {
                if !block.is_empty() {
                    diagnostics.push(
                        Diagnostic::new(Code::BlockSeparator, "separator line ends the block")
                            .at(Location::new(path, line.line)),
                    );
                }
                close(path, &mut block, &mut items, diagnostics);
            } else if !block.is_empty() {
                block.push(line);
            }
        }
    }
    close(path, &mut block, &mut items, diagnostics);
    items
}

fn is_header(trimmed: &str) -> bool {
    trimmed.starts_with("@openapi") || trimmed == "@route" || trimmed.starts_with("@route ")
}

fn is_separator(trimmed: &str) -> bool {
    let mut chars = trimmed.chars();
    match chars.next() {
        Some(first) if "-=*_~#".contains(first) => {
            trimmed.chars().count() >= 3 && chars.all(|c| c == first)
        }
        _ => false,
    }
}

fn close(path: &Path, block: &mut Vec<DocLine>, items: &mut Vec<Item>, diags: &mut Diagnostics) {
    let Some(header) = block.first() else {
        return;
    };
    let trimmed = header.trimmed();
    if trimmed.starts_with("@route") {
        items.push(Item::Route(RouteItem {
            lines: block.iter().map(|l| l.text.clone()).collect(),
            line_numbers: block.iter().map(|l| l.line).collect(),
            operation_id: None,
            tags: Vec::new(),
            location: Location::new(path, header.line),
        }));
    } else if let Some(name) = trimmed.strip_prefix("@openapi-type") {
        let body = unindent(&block[1..]);
        let text = join(&body).trim().to_string();
        if !text.is_empty() {
            let line = body
                .iter()
                .find(|l| !l.text.trim().is_empty())
                .map_or(header.line, |l| l.line);
            items.push(Item::NamedSchema(NamedSchemaItem {
                name: name.trim().to_string(),
                text,
                location: Location::new(path, line),
            }));
        }
    } else {
        items.extend(item_blocks(path, block, &[], diags));
    }
    block.clear();
}
