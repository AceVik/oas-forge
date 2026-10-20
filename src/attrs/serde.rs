//! `#[serde(...)]` attributes that change the serialized shape.

use super::rename::RenameRule;
use syn::punctuated::Punctuated;
use syn::{Attribute, Expr, ExprLit, Lit, Meta};

/// The serde attributes of a container, field or variant.
///
/// Unknown keys and values that are not string literals are ignored.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
#[non_exhaustive]
pub struct SerdeAttrs {
    /// `rename = "..."`
    pub rename: Option<String>,
    /// `rename_all = "..."` (on enums: variants only)
    pub rename_all: Option<RenameRule>,
    /// `rename_all_fields = "..."` (enums: fields of struct variants)
    pub rename_all_fields: Option<RenameRule>,
    /// `tag = "..."` (internally or adjacently tagged enums)
    pub tag: Option<String>,
    /// `content = "..."` (adjacently tagged enums)
    pub content: Option<String>,
}

impl SerdeAttrs {
    /// Collects every `#[serde(...)]` attribute; later values win.
    pub(crate) fn parse(attrs: &[Attribute]) -> Self {
        let mut out = Self::default();
        for meta in serde_metas(attrs) {
            let Meta::NameValue(nv) = meta else { continue };
            let Some(value) = string_literal(&nv.value) else {
                continue;
            };
            let Some(key) = nv.path.get_ident().map(ToString::to_string) else {
                continue;
            };
            match key.as_str() {
                "rename" => out.rename = Some(value),
                "rename_all" => out.rename_all = RenameRule::parse(&value),
                "rename_all_fields" => out.rename_all_fields = RenameRule::parse(&value),
                "tag" => out.tag = Some(value),
                "content" => out.content = Some(value),
                _ => {}
            }
        }
        out
    }
}

/// Every nested meta item of every `#[serde(...)]` attribute.
pub(crate) fn serde_metas(attrs: &[Attribute]) -> Vec<Meta> {
    attrs
        .iter()
        .filter(|a| a.path().is_ident("serde"))
        .filter_map(|a| match &a.meta {
            Meta::List(list) => list
                .parse_args_with(Punctuated::<Meta, syn::Token![,]>::parse_terminated)
                .ok(),
            _ => None,
        })
        .flatten()
        .collect()
}

/// The value of a string literal expression.
pub(crate) fn string_literal(expr: &Expr) -> Option<String> {
    match expr {
        Expr::Lit(ExprLit {
            lit: Lit::Str(s), ..
        }) => Some(s.value()),
        _ => None,
    }
}
