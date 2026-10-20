//! `#[serde(...)]` attributes of containers, fields and variants.
//!
//! The schema describes the serialized form: where serde allows different
//! names for serialization and deserialization (`rename(serialize = ..,
//! deserialize = ..)`), the serialize name is used. Deserialize-only
//! attributes such as `alias` do not change the schema, with one exception:
//! a field with a custom deserializer (`with`, `deserialize_with`) must be
//! present even if it is an `Option`.

use super::rename::RenameRule;
use super::{metas, string_literal};
use syn::punctuated::Punctuated;
use syn::{Attribute, Meta};

/// The serde attributes of a container, field or variant.
///
/// Unknown keys and malformed values are ignored, as serde would reject them
/// at compile time anyway.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
#[non_exhaustive]
pub struct SerdeAttrs {
    /// `rename = "..."` (the serialize name of `rename(serialize = ..)`).
    pub rename: Option<String>,
    /// `rename_all = "..."` (structs: fields; enums: variants).
    pub rename_all: Option<RenameRule>,
    /// `rename_all_fields = "..."` (enums: fields of struct variants).
    pub rename_all_fields: Option<RenameRule>,
    /// `tag = "..."` (internally or adjacently tagged enums).
    pub tag: Option<String>,
    /// `content = "..."` (adjacently tagged enums).
    pub content: Option<String>,
    /// `untagged` (enums and, since serde 1.0.181, single variants).
    pub untagged: bool,
    /// `transparent` (structs with one field).
    pub transparent: bool,
    /// `deny_unknown_fields`
    pub deny_unknown_fields: bool,
    /// `default` or `default = "path"`.
    pub default: bool,
    /// `skip`
    pub skip: bool,
    /// `skip_serializing`
    pub skip_serializing: bool,
    /// `skip_deserializing`
    pub skip_deserializing: bool,
    /// `skip_serializing_if = "path"`
    pub skip_serializing_if: bool,
    /// `flatten`
    pub flatten: bool,
    /// `other` (the catch-all variant).
    pub other: bool,
    /// `with = "module"` or `serialize_with = "function"`.
    pub with: Option<String>,
    /// `with` or `deserialize_with`: a custom deserializer, so a missing
    /// field is an error even for `Option` (unless `default` is set).
    pub custom_deserializer: bool,
    /// `into = "Type"` (containers serialized as another type).
    pub into: Option<String>,
    /// `from = "Type"` / `try_from = "Type"` (containers deserialized from another type).
    pub from: Option<String>,
}

impl SerdeAttrs {
    /// Collects every `#[serde(...)]` attribute (also behind `cfg_attr`);
    /// later values win.
    pub(crate) fn parse(attrs: &[Attribute]) -> Self {
        let mut out = Self::default();
        for meta in metas(attrs, "serde") {
            let Some(key) = meta.path().get_ident().map(ToString::to_string) else {
                continue;
            };
            match &meta {
                Meta::Path(_) => out.flag(&key),
                Meta::NameValue(nv) => {
                    if let Some(value) = string_literal(&nv.value) {
                        out.value(&key, value);
                    }
                }
                Meta::List(list) => {
                    // `rename(serialize = "a", deserialize = "b")`
                    let Ok(args) =
                        list.parse_args_with(Punctuated::<Meta, syn::Token![,]>::parse_terminated)
                    else {
                        continue;
                    };
                    // Only the serialize half describes what serde writes;
                    // `deserialize = ..` alone keeps the serialized name.
                    let serialize = args.into_iter().find_map(|arg| match arg {
                        Meta::NameValue(nv) if nv.path.is_ident("serialize") => {
                            string_literal(&nv.value)
                        }
                        _ => None,
                    });
                    if let Some(value) = serialize {
                        out.value(&key, value);
                    }
                }
            }
        }
        out
    }

    fn flag(&mut self, key: &str) {
        match key {
            "untagged" => self.untagged = true,
            "transparent" => self.transparent = true,
            "deny_unknown_fields" => self.deny_unknown_fields = true,
            "default" => self.default = true,
            "skip" => self.skip = true,
            "skip_serializing" => self.skip_serializing = true,
            "skip_deserializing" => self.skip_deserializing = true,
            "flatten" => self.flatten = true,
            "other" => self.other = true,
            _ => {}
        }
    }

    fn value(&mut self, key: &str, value: String) {
        match key {
            "rename" => self.rename = Some(value),
            "rename_all" => self.rename_all = RenameRule::parse(&value),
            "rename_all_fields" => self.rename_all_fields = RenameRule::parse(&value),
            "tag" => self.tag = Some(value),
            "content" => self.content = Some(value),
            "default" => self.default = true,
            "skip_serializing_if" => self.skip_serializing_if = true,
            "with" => {
                self.with = Some(value);
                self.custom_deserializer = true;
            }
            "serialize_with" => self.with = Some(value),
            "deserialize_with" => self.custom_deserializer = true,
            "into" => self.into = Some(value),
            "from" | "try_from" => self.from = Some(value),
            _ => {}
        }
    }

    /// Whether the field or variant is left out of the serialized form and
    /// of the deserialized form.
    pub fn is_skipped(&self) -> bool {
        self.skip || (self.skip_serializing && self.skip_deserializing)
    }
}
