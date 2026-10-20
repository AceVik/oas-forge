//! Typed models of the attributes and doc directives that shape schemas.

pub(crate) mod directives;
pub mod rename;
pub mod serde;
pub mod validate;

pub use rename::RenameRule;
pub use serde::SerdeAttrs;
pub use validate::{Constraints, LengthUnit, Rule};

use syn::punctuated::Punctuated;
use syn::{Attribute, Expr, ExprLit, Lit, Meta};

/// Every nested item of the `#[name(...)]` attributes, including those behind
/// `#[cfg_attr(predicate, name(...))]` (any feature combination may be active,
/// so conditional attributes are treated as present).
pub(crate) fn metas(attrs: &[Attribute], name: &str) -> Vec<Meta> {
    let mut out = Vec::new();
    for attr in attrs {
        collect(&attr.meta, name, &mut out);
    }
    out
}

fn collect(meta: &Meta, name: &str, out: &mut Vec<Meta>) {
    let Meta::List(list) = meta else {
        return;
    };
    let Ok(nested) = list.parse_args_with(Punctuated::<Meta, syn::Token![,]>::parse_terminated)
    else {
        return;
    };
    if list.path.is_ident(name) {
        out.extend(nested);
    } else if list.path.is_ident("cfg_attr") {
        // The first item is the predicate; the rest are attributes.
        for attribute in nested.iter().skip(1) {
            collect(attribute, name, out);
        }
    }
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
