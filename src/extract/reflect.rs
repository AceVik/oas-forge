//! Schemas reflected from structs, enums and type aliases.

use super::{BlueprintItem, DocumentItem, Item, SchemaItem};
use crate::attrs::directives::{ItemDocs, ItemKind};
use crate::attrs::rename::RenameRule;
use crate::attrs::serde::SerdeAttrs;
use crate::attrs::validate::Constraints;
use crate::diagnostics::{Code, Diagnostic, Diagnostics, Location};
use crate::docs::{doc_lines, line_of};
use crate::merge::{Policy, merge};
use crate::schema::types::{TypeExpr, TypeMapper};
use crate::yaml;
use serde_json::{Map, Value, json};
use std::path::Path;
use syn::spanned::Spanned;

/// Keys that mark an `@openapi` block on a unit struct as a document (root)
/// instead of a schema.
const DOCUMENT_KEYS: &[&str] = &[
    "openapi",
    "info",
    "paths",
    "webhooks",
    "servers",
    "components",
];

struct Naming {
    name: String,
    rename_all: Option<RenameRule>,
    docs: ItemDocs,
}

fn naming(attrs: &[syn::Attribute], default: &str, kind: ItemKind) -> (Naming, SerdeAttrs) {
    let serde = SerdeAttrs::parse(attrs);
    let docs = ItemDocs::parse(&doc_lines(attrs), kind);
    let name = docs
        .rename
        .clone()
        .or_else(|| serde.rename.clone())
        .unwrap_or_else(|| default.to_string());
    let rename_all = docs.rename_all.or(serde.rename_all);
    (
        Naming {
            name,
            rename_all,
            docs,
        },
        serde,
    )
}

/// Applies the `@openapi` override YAML of `docs` to `schema`.
fn apply_overrides(
    schema: &mut Value,
    docs: &ItemDocs,
    what: &str,
    path: &Path,
    diagnostics: &mut Diagnostics,
) {
    let Some(text) = docs.override_text() else {
        return;
    };
    match yaml::parse(&text) {
        Ok(Value::Null) => {}
        Ok(value) => {
            merge(schema, value, Policy::Override);
        }
        Err(e) => diagnostics.push(
            Diagnostic::new(
                Code::InvalidOverride,
                format!(
                    "`@openapi` override of {what} is not valid YAML: {}",
                    e.message
                ),
            )
            .at(Location::new(
                path,
                docs.override_line().unwrap_or(1) + e.line - 1,
            )),
        ),
    }
}

fn with_description(schema: &mut Value, description: &str) {
    if description.is_empty() {
        return;
    }
    if let Value::Object(map) = schema {
        map.insert("description".into(), json!(description));
    }
}

/// Reflects one named field: (serialized name, schema, required).
fn field(
    field: &syn::Field,
    rename_all: Option<RenameRule>,
    path: &Path,
    diagnostics: &mut Diagnostics,
) -> (String, Value, bool) {
    let ident = field
        .ident
        .as_ref()
        .map(ToString::to_string)
        .unwrap_or_default();
    let (naming, _) = naming(&field.attrs, &ident, ItemKind::Member);
    let name = if naming.name == ident {
        rename_all.map_or(ident.clone(), |rule| rule.apply(&ident))
    } else {
        naming.name
    };

    let mapped = TypeMapper::new().map(&TypeExpr::from_syn(&field.ty));
    let mut schema = mapped.schema;
    with_description(&mut schema, &naming.docs.description);

    let constraints = Constraints::parse(&field.attrs);
    if !constraints.is_empty() {
        merge(
            &mut schema,
            Value::Object(constraints.to_keywords()),
            Policy::Override,
        );
    }
    apply_overrides(
        &mut schema,
        &naming.docs,
        &format!("field `{ident}`"),
        path,
        diagnostics,
    );
    (name, schema, mapped.required)
}

/// An object schema from named fields.
fn object(
    fields: &syn::FieldsNamed,
    rename_all: Option<RenameRule>,
    always_required: bool,
    path: &Path,
    diagnostics: &mut Diagnostics,
) -> Value {
    let mut properties = Map::new();
    let mut required = Vec::new();
    for f in &fields.named {
        let (name, schema, is_required) = field(f, rename_all, path, diagnostics);
        if is_required {
            required.push(json!(name));
        }
        properties.insert(name, schema);
    }
    let mut schema = json!({ "type": "object", "properties": properties });
    if always_required || !required.is_empty() {
        schema["required"] = Value::Array(required);
    }
    schema
}

fn wrap_item(
    name: String,
    rust_name: String,
    schema: Value,
    blueprint: Option<Vec<String>>,
    location: Location,
) -> Item {
    match blueprint {
        Some(params) => Item::Blueprint(BlueprintItem {
            name,
            rust_name,
            params,
            schema,
            location,
        }),
        None => Item::Schema(SchemaItem {
            name,
            rust_name,
            schema,
            location,
        }),
    }
}

/// Reflects a struct with an `@openapi` directive.
pub(super) fn structure(
    path: &Path,
    item: &syn::ItemStruct,
    diagnostics: &mut Diagnostics,
) -> Vec<Item> {
    let rust_name = item.ident.to_string();
    let (naming, _) = naming(&item.attrs, &rust_name, ItemKind::Container);
    if !naming.docs.exported {
        return Vec::new();
    }
    let location = Location::new(path, line_of(item.span()));

    let mut schema = match &item.fields {
        syn::Fields::Named(fields) if !fields.named.is_empty() => {
            object(fields, naming.rename_all, false, path, diagnostics)
        }
        _ => {
            if let Some(document) = root_document(&naming.docs, path) {
                return vec![Item::Document(document)];
            }
            json!({ "type": "object" })
        }
    };
    with_description(&mut schema, &naming.docs.description);
    apply_overrides(
        &mut schema,
        &naming.docs,
        &format!("struct `{rust_name}`"),
        path,
        diagnostics,
    );
    vec![wrap_item(
        naming.name,
        rust_name,
        schema,
        naming.docs.blueprint.clone(),
        location,
    )]
}

/// An `@openapi` block on a field-less struct that describes a document
/// (e.g. the root with `openapi` and `info`).
fn root_document(docs: &ItemDocs, path: &Path) -> Option<DocumentItem> {
    let text = docs.override_text()?;
    let value = yaml::parse(&text).ok()?;
    let map = value.as_object()?;
    if !DOCUMENT_KEYS.iter().any(|k| map.contains_key(*k)) {
        return None;
    }
    Some(DocumentItem {
        text,
        location: Location::new(path, docs.override_line().unwrap_or(1)),
        tags: Vec::new(),
    })
}

/// Reflects an enum with an `@openapi` directive.
pub(super) fn enumeration(
    path: &Path,
    item: &syn::ItemEnum,
    diagnostics: &mut Diagnostics,
) -> Vec<Item> {
    let rust_name = item.ident.to_string();
    let (naming, serde) = naming(&item.attrs, &rust_name, ItemKind::Container);
    if !naming.docs.exported {
        return Vec::new();
    }
    let location = Location::new(path, line_of(item.span()));

    if let Some(tag) = &serde.tag {
        return tagged(path, item, &naming, &serde, tag, location, diagnostics);
    }

    let variants: Vec<Value> = item
        .variants
        .iter()
        .filter(|v| matches!(v.fields, syn::Fields::Unit))
        .map(|v| json!(variant_name(v, naming.rename_all)))
        .collect();
    let mut schema = if variants.is_empty() {
        json!({ "type": "string" })
    } else {
        json!({ "type": "string", "enum": variants })
    };
    with_description(&mut schema, &naming.docs.description);
    apply_overrides(
        &mut schema,
        &naming.docs,
        &format!("enum `{rust_name}`"),
        path,
        diagnostics,
    );
    if variants.is_empty() && naming.docs.override_text().is_none() {
        return Vec::new();
    }
    vec![wrap_item(
        naming.name,
        rust_name,
        schema,
        naming.docs.blueprint.clone(),
        location,
    )]
}

/// The serialized name of a variant (tag value or string enum value).
fn variant_name(variant: &syn::Variant, rename_all: Option<RenameRule>) -> String {
    let ident = variant.ident.to_string();
    let (naming, _) = naming(&variant.attrs, &ident, ItemKind::Member);
    if naming.name == ident {
        rename_all.map_or(ident, |rule| rule.apply(&variant.ident.to_string()))
    } else {
        naming.name
    }
}

/// Internally (`tag`) and adjacently (`tag` + `content`) tagged enums.
fn tagged(
    path: &Path,
    item: &syn::ItemEnum,
    naming: &Naming,
    serde: &SerdeAttrs,
    tag: &str,
    location: Location,
    diagnostics: &mut Diagnostics,
) -> Vec<Item> {
    let mut items = Vec::new();
    let mut refs = Vec::new();
    let mut mapping = Map::new();

    for variant in &item.variants {
        let tag_value = variant_name(variant, naming.rename_all);
        let (variant_naming, variant_serde) =
            self::naming(&variant.attrs, &variant.ident.to_string(), ItemKind::Member);
        let schema_name = format!("{}{}", naming.name, variant.ident);
        let reference = format!("#/components/schemas/{schema_name}");
        refs.push(json!({ "$ref": reference }));
        mapping.insert(tag_value.clone(), json!(reference));

        let mut properties = Map::new();
        properties.insert(
            tag.to_string(),
            json!({
                "type": "string",
                "enum": [tag_value],
                "description": format!("Discriminator: {tag_value}"),
            }),
        );
        let mut required = vec![json!(tag)];

        // serde: the enum's `rename_all` renames variants only; fields follow
        // the variant's own `rename_all`, else the enum's `rename_all_fields`.
        let field_rule = variant_naming
            .rename_all
            .or(variant_serde.rename_all)
            .or(serde.rename_all_fields);
        let content = match &variant.fields {
            syn::Fields::Named(fields) => Some(object(fields, field_rule, true, path, diagnostics)),
            syn::Fields::Unnamed(fields) if fields.unnamed.len() == 1 => {
                let f = &fields.unnamed[0];
                let mut schema = TypeMapper::new().map(&TypeExpr::from_syn(&f.ty)).schema;
                let constraints = Constraints::parse(&f.attrs);
                if !constraints.is_empty() {
                    merge(
                        &mut schema,
                        Value::Object(constraints.to_keywords()),
                        Policy::Override,
                    );
                }
                Some(schema)
            }
            syn::Fields::Unnamed(_) => {
                diagnostics.push(
                    Diagnostic::new(
                        Code::UnsupportedVariant,
                        format!(
                            "tuple variant `{}` has more than one field; only the tag is documented",
                            variant.ident
                        ),
                    )
                    .at(Location::new(path, line_of(variant.span()))),
                );
                None
            }
            syn::Fields::Unit => None,
        };

        match (&serde.content, content) {
            (Some(content_key), Some(inner)) => {
                properties.insert(content_key.clone(), inner);
                required.push(json!(content_key));
            }
            (None, Some(inner)) => {
                if let Some(props) = inner.get("properties").and_then(Value::as_object) {
                    for (k, v) in props {
                        properties.insert(k.clone(), v.clone());
                    }
                }
                if let Some(req) = inner.get("required").and_then(Value::as_array) {
                    required.extend(req.iter().cloned());
                }
            }
            (_, None) => {}
        }

        let mut schema =
            json!({ "type": "object", "properties": properties, "required": required });
        with_description(&mut schema, &variant_naming.docs.description);
        items.push(Item::Schema(SchemaItem {
            name: schema_name,
            rust_name: format!("{}{}", item.ident, variant.ident),
            schema,
            location: Location::new(path, line_of(variant.span())),
        }));
    }

    let mut main = json!({
        "type": "object",
        "oneOf": refs,
        "discriminator": { "propertyName": tag, "mapping": mapping },
    });
    with_description(&mut main, &naming.docs.description);
    items.push(Item::Schema(SchemaItem {
        name: naming.name.clone(),
        rust_name: item.ident.to_string(),
        schema: main,
        location,
    }));
    items
}

/// Reflects a type alias (always exported).
pub(super) fn alias(path: &Path, item: &syn::ItemType, diagnostics: &mut Diagnostics) -> Item {
    let rust_name = item.ident.to_string();
    let (naming, _) = naming(&item.attrs, &rust_name, ItemKind::Member);
    let mut schema = TypeMapper::new().map(&TypeExpr::from_syn(&item.ty)).schema;
    with_description(&mut schema, &naming.docs.description);
    apply_overrides(
        &mut schema,
        &naming.docs,
        &format!("type alias `{rust_name}`"),
        path,
        diagnostics,
    );
    Item::Schema(SchemaItem {
        name: naming.name,
        rust_name,
        schema,
        location: Location::new(path, line_of(item.span())),
    })
}
