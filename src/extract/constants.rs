//! Regular expression constants referenced by validation attributes
//! (`#[validate(regex(path = *SLUG))]`, `#[garde(pattern(SLUG))]`).
//!
//! Recognized definitions: a `static`/`const` whose initializer calls
//! `Regex::new("...")` (directly, through `LazyLock`/`Lazy`/`OnceLock`, or with
//! `RegexBuilder::new`), and `static ref` entries of `lazy_static!`.

use super::{Item, PatternItem};
use crate::diagnostics::Location;
use crate::docs::line_of;
use proc_macro2::{TokenStream, TokenTree};
use std::path::Path;

/// The pattern of a `static NAME: ... = ...Regex::new("...")...` item.
pub(super) fn from_item(path: &Path, ident: &syn::Ident, expr: &syn::Expr) -> Option<Item> {
    let tokens = quote::ToTokens::to_token_stream(expr);
    let pattern = first_regex_literal(tokens)?;
    Some(Item::Pattern(PatternItem {
        name: ident.to_string(),
        pattern,
        location: Location::new(path, line_of(ident.span())),
    }))
}

/// The patterns of a `lazy_static! { static ref NAME: Regex = Regex::new("...").unwrap(); }`.
pub(super) fn from_macro(path: &Path, mac: &syn::Macro) -> Vec<Item> {
    if !mac
        .path
        .segments
        .last()
        .is_some_and(|s| s.ident == "lazy_static")
    {
        return Vec::new();
    }
    let tokens: Vec<TokenTree> = mac.tokens.clone().into_iter().collect();
    let mut items = Vec::new();
    let mut i = 0;
    while i + 1 < tokens.len() {
        if is_ident(&tokens[i], "ref")
            && let TokenTree::Ident(name) = &tokens[i + 1]
        {
            let end = tokens[i..]
                .iter()
                .position(|t| matches!(t, TokenTree::Punct(p) if p.as_char() == ';'))
                .map_or(tokens.len(), |offset| i + offset);
            let definition: TokenStream = tokens[i..end].iter().cloned().collect();
            if let Some(pattern) = first_regex_literal(definition) {
                items.push(Item::Pattern(PatternItem {
                    name: name.to_string(),
                    pattern,
                    location: Location::new(path, line_of(name.span())),
                }));
            }
            i = end;
        }
        i += 1;
    }
    items
}

fn is_ident(token: &TokenTree, name: &str) -> bool {
    matches!(token, TokenTree::Ident(i) if i == name)
}

/// The string literal passed to the first `Regex::new(..)` / `RegexBuilder::new(..)`.
fn first_regex_literal(tokens: TokenStream) -> Option<String> {
    let tokens: Vec<TokenTree> = tokens.into_iter().collect();
    for (i, token) in tokens.iter().enumerate() {
        if let TokenTree::Group(group) = token
            && let Some(found) = first_regex_literal(group.stream())
        {
            return Some(found);
        }
        // `Regex :: new (` — `::` is two `:` puncts.
        let is_constructor = matches!(token, TokenTree::Ident(t) if t == "Regex" || t == "RegexBuilder")
            && matches!(tokens.get(i + 3), Some(TokenTree::Ident(n)) if n == "new");
        if !is_constructor {
            continue;
        }
        if let Some(TokenTree::Group(args)) = tokens.get(i + 4)
            && let Some(TokenTree::Literal(literal)) = args.stream().into_iter().next()
            && let Ok(literal) = syn::parse2::<syn::LitStr>(TokenTree::Literal(literal).into())
        {
            return Some(literal.value());
        }
    }
    None
}
