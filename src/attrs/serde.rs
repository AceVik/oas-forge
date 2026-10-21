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
///
/// # Examples
///
/// The attributes are read during extraction; the reflected schema shows
/// their effect:
///
/// ```
/// use oas_forge::diagnostics::Diagnostics;
/// use oas_forge::extract::{self, Item, Options};
/// use serde_json::json;
/// use std::path::Path;
///
/// let source = r#"
/// /// @openapi
/// #[serde(rename_all = "camelCase", deny_unknown_fields)]
/// pub struct Settings {
///     pub dark_mode: bool,
///     #[serde(skip)]
///     pub cache: u8,
///     #[serde(default)]
///     pub page_size: u32,
/// }
/// "#;
/// let items = extract::extract_str(
///     Path::new("src/lib.rs"),
///     source,
///     Options::new(),
///     &mut Diagnostics::new(),
/// )?;
/// let Some(Item::Schema(settings)) = items.first() else {
///     return Err("no schema".into());
/// };
/// assert_eq!(
///     settings.schema,
///     json!({
///         "type": "object",
///         "properties": {
///             "darkMode": { "type": "boolean" },
///             "pageSize": {
///                 "type": "integer",
///                 "format": "int64",
///                 "minimum": 0,
///                 "maximum": u32::MAX,
///             },
///         },
///         "required": ["darkMode"],
///         "additionalProperties": false,
///     })
/// );
/// # Ok::<(), Box<dyn std::error::Error>>(())
/// ```
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
    /// `skip_serializing_if = "Option::is_none"` (also with the `std` path or
    /// a turbofish): `None` is left out instead of written as `null`.
    pub skip_serializing_if_none: bool,
    /// The container derives `Serialize` (also behind `cfg_attr`).
    pub serialize: bool,
    /// The container derives `Deserialize` (also behind `cfg_attr`).
    pub deserialize: bool,
}

impl SerdeAttrs {
    /// Collects every `#[serde(...)]` attribute (also behind `cfg_attr`);
    /// later values win.
    pub(crate) fn parse(attrs: &[Attribute]) -> Self {
        let mut out = Self::default();
        for derive in metas(attrs, "derive") {
            match derive
                .path()
                .segments
                .last()
                .map(|s| s.ident.to_string())
                .as_deref()
            {
                Some("Serialize") => out.serialize = true,
                Some("Deserialize") => out.deserialize = true,
                _ => {}
            }
        }
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

    /// Sets the flag of a bare key such as `untagged` or `skip`.
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

    /// Sets the field of `key = "value"`; an unknown `rename_all` rule
    /// leaves no rule.
    fn value(&mut self, key: &str, value: String) {
        match key {
            "rename" => self.rename = Some(value),
            "rename_all" => self.rename_all = RenameRule::parse(&value),
            "rename_all_fields" => self.rename_all_fields = RenameRule::parse(&value),
            "tag" => self.tag = Some(value),
            "content" => self.content = Some(value),
            "default" => self.default = true,
            "skip_serializing_if" => {
                self.skip_serializing_if = true;
                self.skip_serializing_if_none = is_option_is_none(&value);
            }
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
    ///
    /// # Examples
    ///
    /// ```
    /// use oas_forge::attrs::SerdeAttrs;
    ///
    /// let mut attrs = SerdeAttrs::default();
    /// attrs.skip_serializing = true;
    /// // Still read on input (the schema marks it `writeOnly`).
    /// assert!(!attrs.is_skipped());
    /// attrs.skip_deserializing = true;
    /// assert!(attrs.is_skipped());
    /// ```
    pub fn is_skipped(&self) -> bool {
        self.skip || (self.skip_serializing && self.skip_deserializing)
    }
}

/// Whether `path` is `Option::is_none` (`std::option::Option::is_none`,
/// `Option::<T>::is_none`, spaces allowed).
fn is_option_is_none(path: &str) -> bool {
    let path: String = path.chars().filter(|c| !c.is_whitespace()).collect();
    let Some(option) = path.strip_suffix("::is_none") else {
        return false;
    };
    // `Option::<T>`: the turbofish names no other type.
    let option = option
        .split_once("::<")
        .map_or(option, |(option, _)| option);
    matches!(
        option,
        "Option" | "std::option::Option" | "core::option::Option"
    )
}
