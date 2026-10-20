//! Validation attributes (`validator` crate) mapped to JSON Schema keywords.

use serde_json::{Map, Value, json};
use syn::punctuated::Punctuated;
use syn::{Attribute, Expr, ExprLit, Lit, Meta};

/// Constraints declared with `#[validate(...)]` on a field.
#[derive(Debug, Clone, Default, PartialEq)]
#[non_exhaustive]
pub struct Constraints {
    /// `email` / `url`
    pub format: Option<String>,
    /// `length(min = ..)`
    pub min_length: Option<u64>,
    /// `length(max = ..)`
    pub max_length: Option<u64>,
    /// `range(min = ..)`
    pub minimum: Option<i64>,
    /// `range(max = ..)`
    pub maximum: Option<i64>,
    /// `pattern = ".."`
    pub pattern: Option<String>,
}

impl Constraints {
    /// Collects every `#[validate(...)]` attribute; later values win.
    pub(crate) fn parse(attrs: &[Attribute]) -> Self {
        let mut out = Self::default();
        for meta in validate_metas(attrs) {
            match meta {
                Meta::Path(p) if p.is_ident("email") => out.format = Some("email".into()),
                Meta::Path(p) if p.is_ident("url") => out.format = Some("uri".into()),
                Meta::List(list) if list.path.is_ident("length") => {
                    for (key, value) in int_args(&list) {
                        let Ok(value) = value.base10_parse::<u64>() else {
                            continue;
                        };
                        match key.as_str() {
                            "min" => out.min_length = Some(value),
                            "max" => out.max_length = Some(value),
                            _ => {}
                        }
                    }
                }
                Meta::List(list) if list.path.is_ident("range") => {
                    for (key, value) in int_args(&list) {
                        let Ok(value) = value.base10_parse::<i64>() else {
                            continue;
                        };
                        match key.as_str() {
                            "min" => out.minimum = Some(value),
                            "max" => out.maximum = Some(value),
                            _ => {}
                        }
                    }
                }
                Meta::NameValue(nv) if nv.path.is_ident("pattern") => {
                    if let Expr::Lit(ExprLit {
                        lit: Lit::Str(s), ..
                    }) = nv.value
                    {
                        out.pattern = Some(s.value());
                    }
                }
                _ => {}
            }
        }
        out
    }

    /// Whether no constraint is set.
    pub fn is_empty(&self) -> bool {
        *self == Self::default()
    }

    /// The constraints as JSON Schema keywords.
    pub fn to_keywords(&self) -> Map<String, Value> {
        let mut map = Map::new();
        if let Some(format) = &self.format {
            map.insert("format".into(), json!(format));
        }
        if let Some(v) = self.min_length {
            map.insert("minLength".into(), json!(v));
        }
        if let Some(v) = self.max_length {
            map.insert("maxLength".into(), json!(v));
        }
        if let Some(v) = self.minimum {
            map.insert("minimum".into(), json!(v));
        }
        if let Some(v) = self.maximum {
            map.insert("maximum".into(), json!(v));
        }
        if let Some(pattern) = &self.pattern {
            map.insert("pattern".into(), json!(pattern));
        }
        map
    }
}

fn validate_metas(attrs: &[Attribute]) -> Vec<Meta> {
    attrs
        .iter()
        .filter(|a| a.path().is_ident("validate"))
        .filter_map(|a| match &a.meta {
            Meta::List(list) => list
                .parse_args_with(Punctuated::<Meta, syn::Token![,]>::parse_terminated)
                .ok(),
            _ => None,
        })
        .flatten()
        .collect()
}

/// `key = <integer literal>` arguments of a nested list such as `length(min = 1)`.
fn int_args(list: &syn::MetaList) -> Vec<(String, syn::LitInt)> {
    let Ok(args) = list.parse_args_with(Punctuated::<Meta, syn::Token![,]>::parse_terminated)
    else {
        return Vec::new();
    };
    args.into_iter()
        .filter_map(|arg| match arg {
            Meta::NameValue(nv) => match nv.value {
                Expr::Lit(ExprLit {
                    lit: Lit::Int(i), ..
                }) => Some((nv.path.get_ident()?.to_string(), i)),
                _ => None,
            },
            _ => None,
        })
        .collect()
}
