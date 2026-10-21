//! Schemas reflected from structs, enums and type aliases.
//!
//! The schemas describe what serde produces (ADR 0002): field and variant
//! names after `rename`/`rename_all`, skipped fields left out, `flatten`,
//! `transparent`, newtype and tuple structs, and all four enum
//! representations. Validation attributes add their keywords.

use super::{AliasItem, BlueprintItem, DocumentItem, Item, Options, SchemaItem};
use crate::attrs::directives::{ItemDocs, ItemKind};
use crate::attrs::metas;
use crate::attrs::rename::RenameRule;
use crate::attrs::serde::SerdeAttrs;
use crate::attrs::validate::Constraints;
use crate::diagnostics::{Code, Diagnostic, Diagnostics, Location};
use crate::docs::{doc_lines, line_of};
use crate::merge::{Policy, merge};
use crate::schema::types::{MappedType, TypeExpr, TypeMapper, undocumentable};
use crate::schema::{nullable, shadow};
use crate::template::{Variables, lexical};
use crate::yaml;
use serde_json::{Map, Value, json};
use std::path::Path;
use syn::ext::IdentExt;
use syn::spanned::Spanned;

/// Keys that mark an `@openapi` block on a field-less struct as a document
/// (e.g. the root) instead of a schema.
const DOCUMENT_KEYS: &[&str] = &[
    "openapi",
    "info",
    "paths",
    "webhooks",
    "servers",
    "components",
];

/// Override keys that define a schema's structure.
const STRUCTURE_KEYS: &[&str] = &[
    "type",
    "properties",
    "items",
    "prefixItems",
    "allOf",
    "anyOf",
    "oneOf",
    "enum",
    "const",
    "$ref",
];

/// What the attributes of an item, field or variant say about it.
struct Naming {
    /// The `rename`d name, or the Rust name.
    name: String,
    /// Whether `name` comes from `#[serde(rename)]` or `@openapi rename`
    /// (then no `rename_all` rule applies to it).
    explicit: bool,
    /// How the fields or variants inside the item are renamed:
    /// `@openapi rename-all`, else `#[serde(rename_all)]`.
    rename_all: Option<RenameRule>,
    /// The description, `@openapi` directives and `@example`s of the doc
    /// comment.
    docs: ItemDocs,
    /// The `#[serde(...)]` attributes.
    serde: SerdeAttrs,
    /// Whether the item has a `#[deprecated]` attribute.
    deprecated: bool,
}

/// The outcome of reflecting one named field.
enum Field {
    /// Left out (`#[serde(skip)]`).
    Skip,
    /// A property of the object.
    Property {
        /// The serialized name.
        name: String,
        /// The schema, with description, examples and override.
        schema: Value,
        /// Whether the property belongs in `required`.
        required: bool,
    },
    /// `#[serde(flatten)]` of a struct or enum: its properties are merged in.
    Flatten(Value),
    /// `#[serde(flatten)]` of a map: its entries are additional properties.
    FlattenMap(Value),
}

/// The serialized form of an enum variant's data.
enum Content {
    /// A unit variant: no data.
    Unit,
    /// A newtype variant: the schema of its single field.
    Newtype(Value),
    /// A tuple variant: a fixed-length array.
    Tuple(Value),
    /// A struct variant: an object.
    Struct(Value),
}

/// How an enum is represented (serde's four modes).
enum Representation<'a> {
    /// Serde's default: unit variants are strings, the others
    /// `{"Variant": data}`.
    External,
    /// `#[serde(tag = "...")]`: the tag next to the variant's fields.
    Internal {
        /// The tag property.
        tag: &'a str,
    },
    /// `#[serde(tag = "...", content = "...")]`: the tag and the data in two
    /// properties.
    Adjacent {
        /// The tag property.
        tag: &'a str,
        /// The property that holds the data.
        content: &'a str,
    },
    /// `#[serde(untagged)]`: the data alone.
    Untagged,
}

/// Reflects the structs, enums and type aliases of one file into schemas.
pub(super) struct Reflector<'a> {
    /// The file, for locations.
    path: &'a Path,
    /// How Rust types map to schemas.
    types: &'a TypeMapper,
    /// Values of `{{NAME}}` placeholders in overrides and `@example`s.
    variables: &'a Variables,
    /// Where findings go.
    diagnostics: &'a mut Diagnostics,
    /// `@openapi-fragment` blocks found in the doc comments of the items
    /// reflected so far, their fields and their variants.
    fragments: Vec<Item>,
}

impl<'a> Reflector<'a> {
    /// A reflector for the file at `path`.
    pub(super) fn new(
        path: &'a Path,
        options: Options<'a>,
        diagnostics: &'a mut Diagnostics,
    ) -> Self {
        Self {
            path,
            types: options.types,
            variables: options.variables,
            diagnostics,
            fragments: Vec::new(),
        }
    }

    /// The fragments defined in the doc comments of the items reflected so
    /// far, their fields and their variants (also of items that are not
    /// exported).
    pub(super) fn take_fragments(&mut self) -> Vec<Item> {
        std::mem::take(&mut self.fragments)
    }

    /// Keeps the `@openapi-fragment` blocks of an item's doc comment
    /// (`attrs`) and of its `members` (fields, variants and the fields of
    /// variants) for [`Self::take_fragments`], in source order. Every member
    /// is read, also of items that are not exported or not reflected field
    /// by field (`into`, `transparent`, tuple structs); nothing else of their
    /// docs is checked here.
    fn fragments<'m>(
        &mut self,
        attrs: &'m [syn::Attribute],
        members: impl IntoIterator<Item = &'m [syn::Attribute]>,
    ) {
        for attrs in std::iter::once(attrs).chain(members) {
            // The kind only matters for `@openapi<T>`, not for fragments.
            let docs = ItemDocs::parse(&doc_lines(attrs), ItemKind::Member);
            for (header, body) in &docs.fragments {
                self.fragments
                    .push(super::blocks::item_fragment(self.path, header, body));
            }
        }
    }

    /// The location of the 1-based `line` of the file.
    fn location(&self, line: usize) -> Location {
        Location::new(self.path, line)
    }

    /// Reads the attributes and doc comment of an item, field or variant
    /// (`kind`) named `default` in Rust. Invalid directives are reported;
    /// fragments are read by [`Self::fragments`].
    fn naming(&mut self, attrs: &[syn::Attribute], default: &str, kind: ItemKind) -> Naming {
        let serde = SerdeAttrs::parse(attrs);
        let docs = ItemDocs::parse(&doc_lines(attrs), kind);
        for problem in &docs.problems {
            self.diagnostics.push(
                Diagnostic::new(Code::InvalidSchemaDirective, problem.text.clone())
                    .at(self.location(problem.line)),
            );
        }
        let renamed = docs.rename.clone().or_else(|| serde.rename.clone());
        Naming {
            explicit: renamed.is_some(),
            name: renamed.unwrap_or_else(|| default.to_string()),
            rename_all: docs.rename_all.or(serde.rename_all),
            deprecated: attrs.iter().any(|a| a.path().is_ident("deprecated")),
            docs,
            serde,
        }
    }

    /// The serialized name of a field or variant: an explicit `rename` as
    /// written, otherwise the Rust name converted by `rule`.
    fn member_name(
        naming: &Naming,
        rule: Option<RenameRule>,
        apply: fn(RenameRule, &str) -> String,
    ) -> String {
        match rule {
            Some(rule) if !naming.explicit => apply(rule, &naming.name),
            _ => naming.name.clone(),
        }
    }

    /// Adds description, `deprecated`, `@example`s and the `@openapi`
    /// override YAML (also to the alternatives of built-in names).
    fn finish(&mut self, schema: &mut Value, naming: &Naming, what: &str) {
        let annotations = self.annotations(naming, what);
        shadow::for_each(schema, |schema| annotations.apply(schema));
    }

    /// What the docs of an item, field or variant add to its schema; problems
    /// are reported here, once.
    fn annotations(&mut self, naming: &Naming, what: &str) -> Annotations {
        let mut annotations = Annotations {
            description: (!naming.docs.description.is_empty())
                .then(|| naming.docs.description.clone()),
            examples: None,
            deprecated: naming.deprecated,
            overrides: None,
        };
        if !naming.docs.examples.is_empty() {
            let examples: Vec<Value> = naming
                .docs
                .examples
                .iter()
                .map(|line| {
                    // Substituted as a list item, so that a value that is a
                    // single placeholder stays a string (like `key: {{V}}`).
                    let item = lexical::substitute(
                        &format!("- {}", line.text),
                        self.variables,
                        &Location::new(self.path, line.line),
                        self.diagnostics,
                    );
                    let text = item.strip_prefix("- ").unwrap_or(&item);
                    // Text YAML reads as nothing (`@example # fff`) stays text;
                    // an explicit `null` is the value `null`.
                    match yaml::parse(&item) {
                        Ok(Value::Array(values)) => match values.into_iter().next() {
                            Some(Value::Null)
                                if matches!(text.trim(), "null" | "Null" | "NULL" | "~") =>
                            {
                                Value::Null
                            }
                            Some(value) if !value.is_null() => value,
                            _ => json!(text),
                        },
                        _ => json!(text),
                    }
                })
                .collect();
            annotations.examples = Some(examples);
        }
        let Some(text) = naming.docs.override_text() else {
            return annotations;
        };
        let start = self.location(naming.docs.override_line().unwrap_or(1));
        let text = lexical::substitute(&text, self.variables, &start, self.diagnostics);
        match yaml::parse(&text) {
            Ok(Value::Null) => {}
            Ok(Value::Object(value)) => annotations.overrides = Some(value),
            Ok(_) => {
                self.diagnostics.push(
                    Diagnostic::new(
                        Code::InvalidOverride,
                        format!(
                            "`@openapi` override of {what} is not a YAML mapping (`{}`); it is ignored",
                            text.trim()
                        ),
                    )
                    .at(start),
                );
            }
            Err(e) => {
                let line = naming.docs.override_line().unwrap_or(1) + e.line - 1;
                self.diagnostics.push(
                    Diagnostic::new(
                        Code::InvalidOverride,
                        format!(
                            "`@openapi` override of {what} is not valid YAML: {}",
                            e.message
                        ),
                    )
                    .at(self.location(line)),
                );
            }
        }
        annotations
    }

    /// The schema of a container serialized as another type (`into`/`from`).
    fn conversion(&self, serde: &SerdeAttrs) -> Option<Value> {
        let target = serde.into.as_ref().or(serde.from.as_ref())?;
        let ty = TypeExpr::parse(target)?;
        Some(self.typed(&ty, MappedType::into_schema))
    }

    /// The schema `build` makes from the mapping of `ty`. When `ty` names
    /// built-in types bare (`Duration`), it carries the alternatives for
    /// documented schemas of these names (see [`shadow`]).
    fn typed(&self, ty: &TypeExpr, build: impl Fn(MappedType) -> Value) -> Value {
        let eager = build(self.types.map(ty));
        let names = self.types.shadowable_names(ty);
        shadow::with_alternatives(eager, &names, |subset| {
            build(self.types.shadowing(subset).map(ty))
        })
    }

    /// Reports the parts of `ty`, the type of a documented field at `line`,
    /// that cannot be documented (their schema is `{}`).
    fn report_undocumentable(&mut self, ty: &TypeExpr, line: usize) {
        for part in ty.undocumentable() {
            self.diagnostics
                .push(undocumentable(&part).at(self.location(line)));
        }
    }

    /// The validation keywords of a field, reporting values that are not literals.
    fn constraints(&mut self, attrs: &[syn::Attribute], line: usize) -> Constraints {
        let (constraints, problems) = Constraints::parse(attrs);
        for problem in problems {
            self.diagnostics.push(
                Diagnostic::new(
                    Code::NonLiteralConstraint,
                    format!("{problem}; the constraint is not documented"),
                )
                .at(self.location(line)),
            );
        }
        constraints
    }

    /// The schema of an unnamed field (newtype and tuple members): type,
    /// validation and nullability. `None` when the field is skipped.
    fn unnamed_field(&mut self, field: &syn::Field) -> Option<Value> {
        let serde = SerdeAttrs::parse(&field.attrs);
        if serde.is_skipped() {
            return None;
        }
        let line = line_of(field.span());
        let constraints = self.constraints(&field.attrs, line);
        let with = serde.with.as_deref().and_then(with_module);
        let ty = TypeExpr::from_syn(&field.ty);
        self.report_undocumentable(&ty, line);
        Some(self.typed(&ty, |mapped| {
            let mut schema = with.clone().unwrap_or(mapped.schema);
            constraints.apply(&mut schema);
            if mapped.nullable && !constraints.requires_value() {
                schema = nullable(schema);
            }
            schema
        }))
    }

    /// Reflects one named field.
    fn field(
        &mut self,
        field: &syn::Field,
        rule: Option<RenameRule>,
        container_default: bool,
    ) -> Field {
        let ident = field
            .ident
            .as_ref()
            .map(|i| i.unraw().to_string())
            .unwrap_or_default();
        let naming = self.naming(&field.attrs, &ident, ItemKind::Member);
        let serde = &naming.serde;
        if serde.is_skipped() || naming.docs.skip {
            return Field::Skip;
        }
        let name = Self::member_name(&naming, rule, RenameRule::apply_to_field);

        let ty = TypeExpr::from_syn(&field.ty);
        self.report_undocumentable(&ty, line_of(field.span()));
        // Known `with` modules replace the type's schema.
        let with = serde.with.as_deref().and_then(with_module);
        let mapped = self.types.map(&ty);
        if serde.flatten {
            let is_map = with.is_none()
                && mapped.schema.get("type") == Some(&json!("object"))
                && mapped.schema.get("properties").is_none()
                && mapped.schema.get("additionalProperties").is_some();
            let mut schema = self.typed(&ty, |m| with.clone().unwrap_or(m.schema));
            return if is_map {
                shadow::for_each(&mut schema, |schema| {
                    *schema = schema
                        .get("additionalProperties")
                        .cloned()
                        .unwrap_or_else(|| json!({}));
                });
                Field::FlattenMap(schema)
            } else if mapped.nullable {
                // A flattened `Option<T>` may be absent as a whole.
                Field::Flatten(json!({ "anyOf": [schema, {}] }))
            } else {
                Field::Flatten(schema)
            };
        }

        let constraints = self.constraints(&field.attrs, line_of(field.span()));
        let forced = constraints.requires_value();
        let mut schema = self.typed(&ty, |mapped| {
            let mut schema = with.clone().unwrap_or(mapped.schema);
            constraints.apply(&mut schema);
            if mapped.nullable && !forced {
                schema = nullable(schema);
            }
            if let Value::Object(map) = &mut schema {
                if serde.skip_serializing {
                    map.insert("writeOnly".into(), json!(true));
                }
                if serde.skip_deserializing {
                    map.insert("readOnly".into(), json!(true));
                }
            }
            schema
        });
        self.finish(&mut schema, &naming, &format!("field `{ident}`"));
        // A custom deserializer makes even an `Option` field mandatory: serde
        // only fills in `None` for a missing field it deserializes itself.
        let required = (mapped.required || forced || serde.custom_deserializer)
            && !serde.default
            && !container_default
            && !serde.skip_serializing_if;
        Field::Property {
            name,
            schema,
            required,
        }
    }

    /// An object schema from named fields.
    fn object(
        &mut self,
        fields: &syn::FieldsNamed,
        rule: Option<RenameRule>,
        container: &SerdeAttrs,
        always_required: bool,
    ) -> Value {
        let mut properties = Map::new();
        let mut required = Vec::new();
        let mut flattened = Vec::new();
        let mut additional = None;
        for f in &fields.named {
            match self.field(f, rule, container.default) {
                Field::Skip => {}
                Field::Property {
                    name,
                    schema,
                    required: is_required,
                } => {
                    if is_required {
                        required.push(json!(name));
                    }
                    properties.insert(name, schema);
                }
                Field::Flatten(schema) => flattened.push(schema),
                Field::FlattenMap(values) => additional = Some(values),
            }
        }
        let mut schema = Map::new();
        schema.insert("type".into(), json!("object"));
        if !properties.is_empty() || flattened.is_empty() {
            schema.insert("properties".into(), Value::Object(properties));
        }
        if always_required || !required.is_empty() {
            schema.insert("required".into(), Value::Array(required));
        }
        // `additionalProperties` does not see the properties of flattened
        // schemas (`allOf`); `unevaluatedProperties` does.
        let others = if flattened.is_empty() {
            "additionalProperties"
        } else {
            schema.insert("allOf".into(), Value::Array(flattened));
            "unevaluatedProperties"
        };
        match additional {
            Some(values) => {
                schema.insert(others.into(), values);
            }
            None if container.deny_unknown_fields => {
                schema.insert(others.into(), json!(false));
            }
            None => {}
        }
        Value::Object(schema)
    }

    /// Reflects a struct with an `@openapi` directive.
    pub(super) fn structure(&mut self, item: &syn::ItemStruct) -> Vec<Item> {
        self.fragments(&item.attrs, item.fields.iter().map(|f| &f.attrs[..]));
        let rust_name = item.ident.unraw().to_string();
        let naming = self.naming(&item.attrs, &rust_name, ItemKind::Container);
        if !naming.docs.exported || naming.docs.skip {
            return Vec::new();
        }
        let location = self.location(line_of(item.span()));
        let serde = &naming.serde;

        let mut schema = if let Some(schema) = self.conversion(serde) {
            schema
        } else {
            match &item.fields {
                // The one field that is not skipped.
                syn::Fields::Named(fields) if serde.transparent => fields
                    .named
                    .iter()
                    .find_map(|f| self.unnamed_field(f))
                    .unwrap_or_else(|| json!({})),
                syn::Fields::Named(fields) if !fields.named.is_empty() => {
                    let mut schema = self.object(fields, naming.rename_all, serde, false);
                    if let Some(tag) = &serde.tag {
                        let value = serde.rename.as_deref().unwrap_or(&rust_name);
                        struct_tag(&mut schema, tag, value);
                    }
                    schema
                }
                syn::Fields::Unnamed(fields) => {
                    let mut members: Vec<Value> = fields
                        .unnamed
                        .iter()
                        .filter_map(|f| self.unnamed_field(f))
                        .collect();
                    // A newtype struct (one field, or `transparent`) is its
                    // single field; serde writes other tuple structs as
                    // arrays of their members, even when all but one are
                    // skipped.
                    let newtype =
                        members.len() == 1 && (fields.unnamed.len() == 1 || serde.transparent);
                    match members.pop() {
                        Some(single) if newtype => single,
                        last => {
                            members.extend(last);
                            tuple(members)
                        }
                    }
                }
                _ => {
                    if let Some(document) = self.root_document(&naming.docs) {
                        return vec![Item::Document(document)];
                    }
                    match (&item.fields, naming.docs.override_text()) {
                        // `struct S {}` is an empty object.
                        (syn::Fields::Named(_), _) => {
                            let mut schema = json!({ "type": "object" });
                            if let Some(tag) = &serde.tag {
                                let value = serde.rename.as_deref().unwrap_or(&rust_name);
                                struct_tag(&mut schema, tag, value);
                            }
                            schema
                        }
                        // A unit struct is `null`, unless the block defines a
                        // virtual type (0.1 compatible).
                        (_, Some(_)) => json!({ "type": "object" }),
                        (_, None) => json!({ "type": "null" }),
                    }
                }
            }
        };
        self.finish(&mut schema, &naming, &format!("struct `{rust_name}`"));
        let blueprint = naming
            .docs
            .blueprint
            .clone()
            .or_else(|| type_params(&item.generics));
        vec![wrap_item(
            naming.name,
            rust_name,
            schema,
            blueprint,
            location,
        )]
    }

    /// An `@openapi` block on a field-less struct that describes a document
    /// (e.g. the root with `openapi` and `info`).
    fn root_document(&self, docs: &ItemDocs) -> Option<DocumentItem> {
        let text = docs.override_text()?;
        let value = yaml::parse(&text).ok()?;
        let map = value.as_object()?;
        if !DOCUMENT_KEYS.iter().any(|k| map.contains_key(*k)) {
            return None;
        }
        Some(DocumentItem {
            text,
            location: self.location(docs.override_line().unwrap_or(1)),
            tags: Vec::new(),
        })
    }

    /// Reflects a type alias. With `@openapi` it is exported (a generic
    /// alias is a blueprint); without it, or with `@openapi skip`, it is a
    /// transparent [`Item::Alias`] that stands for its target.
    pub(super) fn alias(&mut self, item: &syn::ItemType) -> Option<Item> {
        self.fragments(&item.attrs, []);
        let rust_name = item.ident.unraw().to_string();
        let naming = self.naming(&item.attrs, &rust_name, ItemKind::Container);
        let ty = TypeExpr::from_syn(&item.ty);
        let mut schema = self.typed(&ty, MappedType::into_schema);
        if !naming.docs.exported || naming.docs.skip {
            return Some(Item::Alias(AliasItem {
                rust_name,
                params: type_params(&item.generics).unwrap_or_default(),
                schema,
                undocumentable: ty.undocumentable(),
                location: self.location(line_of(item.span())),
            }));
        }
        self.report_undocumentable(&ty, line_of(item.span()));
        self.finish(&mut schema, &naming, &format!("type alias `{rust_name}`"));
        // `type Paged<T> = Page<T>` is a template like a generic struct, and
        // `@openapi<T>` declares one like on a struct.
        let blueprint = naming
            .docs
            .blueprint
            .clone()
            .or_else(|| type_params(&item.generics));
        Some(wrap_item(
            naming.name,
            rust_name,
            schema,
            blueprint,
            self.location(line_of(item.span())),
        ))
    }

    /// Reflects an enum with an `@openapi` directive.
    pub(super) fn enumeration(&mut self, item: &syn::ItemEnum) -> Vec<Item> {
        let members = item.variants.iter().flat_map(|variant| {
            std::iter::once(&variant.attrs[..]).chain(variant.fields.iter().map(|f| &f.attrs[..]))
        });
        self.fragments(&item.attrs, members);
        let rust_name = item.ident.unraw().to_string();
        let naming = self.naming(&item.attrs, &rust_name, ItemKind::Container);
        if !naming.docs.exported || naming.docs.skip {
            return Vec::new();
        }
        let location = self.location(line_of(item.span()));
        let blueprint = naming
            .docs
            .blueprint
            .clone()
            .or_else(|| type_params(&item.generics));
        if let Some(mut schema) = self.conversion(&naming.serde) {
            self.finish(&mut schema, &naming, &format!("enum `{rust_name}`"));
            return vec![wrap_item(
                naming.name,
                rust_name,
                schema,
                blueprint,
                location,
            )];
        }

        if let Some(mut schema) = self.repr_enum(item) {
            self.finish(&mut schema, &naming, &format!("enum `{rust_name}`"));
            return vec![wrap_item(
                naming.name,
                rust_name,
                schema,
                blueprint,
                location,
            )];
        }

        let serde = &naming.serde;
        let representation = match (&serde.tag, &serde.content) {
            _ if serde.untagged => Representation::Untagged,
            (Some(tag), Some(content)) => Representation::Adjacent { tag, content },
            (Some(tag), None) => Representation::Internal { tag },
            (None, _) => Representation::External,
        };
        let variants: Vec<(&syn::Variant, Naming)> = item
            .variants
            .iter()
            .map(|v| {
                (
                    v,
                    self.naming(&v.attrs, &v.ident.unraw().to_string(), ItemKind::Member),
                )
            })
            .filter(|(_, n)| !n.serde.is_skipped() && !n.docs.skip)
            .collect();

        let mut items = Vec::new();
        let mut schema = match representation {
            Representation::External => self.external(&naming, &variants),
            Representation::Untagged => {
                let alternatives = variants
                    .iter()
                    .map(|(v, n)| self.untagged_alternative(v, n, &naming))
                    .collect::<Vec<_>>();
                json!({ "anyOf": alternatives })
            }
            Representation::Internal { tag } => self.tagged(
                &naming,
                &variants,
                &Tagging {
                    tag,
                    content: None,
                    rust_name: &rust_name,
                    params: blueprint.as_deref(),
                },
                &mut items,
            ),
            Representation::Adjacent { tag, content } => self.tagged(
                &naming,
                &variants,
                &Tagging {
                    tag,
                    content: Some(content),
                    rust_name: &rust_name,
                    params: blueprint.as_deref(),
                },
                &mut items,
            ),
        };
        if variants.is_empty() && naming.docs.override_text().is_none() {
            return Vec::new();
        }
        self.finish(&mut schema, &naming, &format!("enum `{rust_name}`"));
        items.push(wrap_item(
            naming.name,
            rust_name,
            schema,
            blueprint,
            location,
        ));
        items
    }

    /// The schema of an enum serialized by `serde_repr` (`Serialize_repr`):
    /// the integer discriminants of its variants.
    fn repr_enum(&mut self, item: &syn::ItemEnum) -> Option<Value> {
        let derived = metas(&item.attrs, "derive").iter().any(|meta| {
            meta.path()
                .segments
                .last()
                .is_some_and(|segment| segment.ident == "Serialize_repr")
        });
        if !derived {
            return None;
        }
        let mut values = Vec::new();
        let mut next = Some(0_i128);
        for variant in &item.variants {
            let value = match &variant.discriminant {
                Some((_, expr)) => integer_literal(expr),
                None => next,
            };
            match value {
                Some(value) => {
                    values.push(integer_value(value));
                    next = value.checked_add(1);
                }
                None => {
                    self.diagnostics.push(
                        Diagnostic::new(
                            Code::UnknownDiscriminant,
                            format!(
                                "the discriminant of variant `{}` of `{}` is not an integer \
                                 literal; the values of the enum are not documented",
                                variant.ident, item.ident
                            ),
                        )
                        .at(self.location(line_of(variant.span()))),
                    );
                    return Some(json!({ "type": "integer" }));
                }
            }
        }
        Some(json!({ "type": "integer", "enum": values }))
    }

    /// The serialized name of a variant.
    fn variant_name(naming: &Naming, rule: Option<RenameRule>) -> String {
        Self::member_name(naming, rule, RenameRule::apply_to_variant)
    }

    /// The data of a variant. Fields of struct variants follow the variant's
    /// own `rename_all`, else the enum's `rename_all_fields`.
    fn content(
        &mut self,
        variant: &syn::Variant,
        naming: &Naming,
        container: &Naming,
        always_required: bool,
    ) -> Content {
        match &variant.fields {
            syn::Fields::Unit => Content::Unit,
            syn::Fields::Named(fields) => {
                let rule = naming.rename_all.or(container.serde.rename_all_fields);
                // The enum's `deny_unknown_fields` applies to its struct variants.
                let mut attrs = naming.serde.clone();
                attrs.deny_unknown_fields |= container.serde.deny_unknown_fields;
                Content::Struct(self.object(fields, rule, &attrs, always_required))
            }
            syn::Fields::Unnamed(fields) => {
                let members: Vec<Value> = fields
                    .unnamed
                    .iter()
                    .filter_map(|f| self.unnamed_field(f))
                    .collect();
                match members.as_slice() {
                    [single] if fields.unnamed.len() == 1 => {
                        let mut schema = single.clone();
                        if let Some(with) = naming.serde.with.as_deref().and_then(with_module) {
                            schema = with;
                        }
                        Content::Newtype(schema)
                    }
                    _ => Content::Tuple(tuple(members)),
                }
            }
        }
    }

    /// A variant of an untagged enum (or with `#[serde(untagged)]`).
    fn untagged_alternative(
        &mut self,
        variant: &syn::Variant,
        naming: &Naming,
        container: &Naming,
    ) -> Value {
        let mut schema = match self.content(variant, naming, container, false) {
            Content::Unit => json!({ "type": "null" }),
            Content::Newtype(schema) | Content::Tuple(schema) | Content::Struct(schema) => schema,
        };
        self.finish(&mut schema, naming, &format!("variant `{}`", variant.ident));
        schema
    }

    /// Externally tagged enums (serde's default): unit variants are strings,
    /// data variants objects with a single key.
    fn external(&mut self, container: &Naming, variants: &[(&syn::Variant, Naming)]) -> Value {
        let rule = container.rename_all;
        let tagged: Vec<_> = variants.iter().filter(|(_, n)| !n.serde.untagged).collect();
        let units: Vec<_> = tagged
            .iter()
            .filter(|(v, _)| matches!(v.fields, syn::Fields::Unit))
            .collect();
        let names: Vec<Value> = units
            .iter()
            .map(|(_, n)| json!(Self::variant_name(n, rule)))
            .collect();
        let mut strings = if names.is_empty() {
            json!({ "type": "string" })
        } else {
            json!({ "type": "string", "enum": names })
        };
        // A string `enum` says nothing about single values. When a unit
        // variant has an override, `@example` or `#[deprecated]`, a `oneOf`
        // of `const`s next to the `enum` carries it (and the variants' docs).
        let annotated = units.iter().any(|(_, n)| {
            n.deprecated || !n.docs.examples.is_empty() || n.docs.override_text().is_some()
        });
        if annotated && let Value::Object(map) = &mut strings {
            let mut values = Vec::new();
            for (variant, naming) in &units {
                let mut value = json!({ "const": Self::variant_name(naming, rule) });
                self.finish(&mut value, naming, &format!("variant `{}`", variant.ident));
                values.push(value);
            }
            map.insert("oneOf".into(), Value::Array(values));
        }
        if units.len() == variants.len() {
            return strings;
        }
        let mut alternatives = Vec::new();
        if !units.is_empty() {
            alternatives.push(strings);
        }
        for (variant, naming) in &tagged {
            let name = Self::variant_name(naming, rule);
            let content = match self.content(variant, naming, container, false) {
                Content::Unit => continue,
                Content::Newtype(schema) | Content::Tuple(schema) | Content::Struct(schema) => {
                    schema
                }
            };
            let mut properties = Map::new();
            properties.insert(name.clone(), content);
            let mut alternative = json!({
                "type": "object",
                "properties": properties,
                "required": [name],
                "additionalProperties": false,
            });
            self.finish(
                &mut alternative,
                naming,
                &format!("variant `{}`", variant.ident),
            );
            alternatives.push(alternative);
        }
        let mut untagged: Vec<Value> = variants
            .iter()
            .filter(|(_, n)| n.serde.untagged)
            .map(|(v, n)| self.untagged_alternative(v, n, container))
            .collect();
        if untagged.is_empty() {
            json!({ "oneOf": alternatives })
        } else {
            alternatives.append(&mut untagged);
            json!({ "anyOf": alternatives })
        }
    }

    /// Internally (`tag`) and adjacently (`tag` + `content`) tagged enums: one
    /// component schema per variant, combined with a discriminator. Variants
    /// of a generic enum that use its type parameters are blueprints
    /// themselves (`$ShapeCircle<$T>`), instantiated with the enum.
    fn tagged(
        &mut self,
        container: &Naming,
        variants: &[(&syn::Variant, Naming)],
        tagging: &Tagging<'_>,
        items: &mut Vec<Item>,
    ) -> Value {
        let Tagging {
            tag,
            content: content_key,
            rust_name,
            params,
        } = *tagging;
        let mut refs = Vec::new();
        let mut mapping = Map::new();
        let mut untagged = Vec::new();

        for (variant, naming) in variants {
            if naming.serde.untagged {
                untagged.push(self.untagged_alternative(variant, naming, container));
                continue;
            }
            let tag_value = Self::variant_name(naming, container.rename_all);
            let schema_name = format!("{}{}", container.name, variant.ident.unraw());

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
            let mut all_of = Vec::new();
            // Keywords about the properties not listed in `properties`: they
            // must stay next to the merged properties (and the tag).
            let mut closing = Vec::new();
            let variant_line = line_of(variant.span());

            match (content_key, self.content(variant, naming, container, true)) {
                (_, Content::Unit) => {}
                (
                    Some(key),
                    Content::Newtype(inner) | Content::Tuple(inner) | Content::Struct(inner),
                ) => {
                    properties.insert(key.to_string(), inner);
                    required.push(json!(key));
                }
                (None, Content::Struct(mut inner)) => {
                    let mut inner: Map<String, Value> = inner
                        .as_object_mut()
                        .map(std::mem::take)
                        .unwrap_or_default();
                    if let Some(Value::Object(props)) = inner.remove("properties") {
                        properties.extend(props);
                    }
                    if let Some(Value::Array(req)) = inner.remove("required") {
                        required.extend(req);
                    }
                    if let Some(value) = inner.remove("allOf") {
                        all_of.push(json!({ "allOf": value }));
                    }
                    for key in ["additionalProperties", "unevaluatedProperties"] {
                        if let Some(value) = inner.remove(key) {
                            closing.push((key, value));
                        }
                    }
                }
                // Internally tagged newtype variants carry the fields of a
                // struct or map next to the tag.
                (None, Content::Newtype(inner)) if is_object_like(&inner) => all_of.push(inner),
                (None, Content::Newtype(_) | Content::Tuple(_)) => {
                    self.diagnostics.push(
                        Diagnostic::new(
                            Code::UnsupportedVariant,
                            format!(
                                "variant `{}` of internally tagged enum `{rust_name}` holds a value \
                                 serde cannot put next to the tag; only the tag is documented",
                                variant.ident
                            ),
                        )
                        .at(self.location(variant_line)),
                    );
                }
            }

            let mut map = Map::new();
            map.insert("type".into(), json!("object"));
            map.insert("properties".into(), Value::Object(properties));
            map.insert("required".into(), Value::Array(required));
            if !all_of.is_empty() {
                map.insert("allOf".into(), Value::Array(all_of));
            }
            for (key, value) in closing {
                map.insert(key.into(), value);
            }
            let mut schema = Value::Object(map);
            self.finish(&mut schema, naming, &format!("variant `{}`", variant.ident));

            let used: Vec<String> = params
                .unwrap_or_default()
                .iter()
                .filter(|param| mentions_param(&schema, param))
                .cloned()
                .collect();
            let location = self.location(variant_line);
            let variant_rust_name = format!("{rust_name}{}", variant.ident.unraw());
            let reference = if used.is_empty() {
                items.push(Item::Schema(SchemaItem {
                    name: schema_name.clone(),
                    rust_name: variant_rust_name,
                    schema,
                    location,
                }));
                format!("#/components/schemas/{schema_name}")
            } else {
                // Raw until the generics stage (`$ShapeCircle<$T>`).
                let args: Vec<String> = used.iter().map(|p| format!("${p}")).collect();
                let reference = format!("${schema_name}<{}>", args.join(", "));
                items.push(Item::Blueprint(BlueprintItem {
                    name: schema_name,
                    rust_name: variant_rust_name,
                    params: used,
                    schema,
                    location,
                }));
                reference
            };
            refs.push(json!({ "$ref": reference }));
            mapping.insert(tag_value, json!(reference));
        }

        if untagged.is_empty() {
            json!({
                "type": "object",
                "oneOf": refs,
                "discriminator": { "propertyName": tag, "mapping": mapping },
            })
        } else {
            refs.append(&mut untagged);
            json!({ "anyOf": refs })
        }
    }
}

/// How a tagged enum is written.
#[derive(Clone, Copy)]
struct Tagging<'a> {
    /// The tag property.
    tag: &'a str,
    /// The content property (adjacently tagged enums).
    content: Option<&'a str>,
    /// The Rust name of the enum.
    rust_name: &'a str,
    /// The type parameters of a generic enum.
    params: Option<&'a [String]>,
}

/// Whether a string or key of `schema` mentions the type parameter `$param`
/// (`$T`, also inside `$Page<$T>`).
fn mentions_param(schema: &Value, param: &str) -> bool {
    let needle = format!("${param}");
    let hit = |text: &str| {
        text.match_indices(needle.as_str()).any(|(at, _)| {
            !text
                .get(at + needle.len()..)
                .and_then(|rest| rest.chars().next())
                .is_some_and(|c| c.is_alphanumeric() || c == '_')
        })
    };
    fn walk(value: &Value, hit: &dyn Fn(&str) -> bool) -> bool {
        match value {
            Value::String(text) => hit(text),
            Value::Array(items) => items.iter().any(|item| walk(item, hit)),
            Value::Object(map) => map.iter().any(|(key, value)| hit(key) || walk(value, hit)),
            _ => false,
        }
    }
    walk(schema, &hit)
}

/// What the docs of an item, field or variant add to its schema.
struct Annotations {
    /// The prose of the doc comment; `None` without any.
    description: Option<String>,
    /// The `@example` values, parsed as YAML (text that is no YAML or reads
    /// as nothing stays text); `None` without `@example`.
    examples: Option<Vec<Value>>,
    /// Whether the item has a `#[deprecated]` attribute.
    deprecated: bool,
    /// The `@openapi` override YAML.
    overrides: Option<Map<String, Value>>,
}

impl Annotations {
    /// Sets `description`, `examples` and `deprecated` on an object
    /// `schema`, then merges the override ([`apply_override`]).
    fn apply(&self, schema: &mut Value) {
        if let Value::Object(map) = schema {
            if let Some(description) = &self.description {
                map.insert("description".into(), json!(description));
            }
            if let Some(examples) = &self.examples {
                map.insert("examples".into(), Value::Array(examples.clone()));
            }
            if self.deprecated {
                map.insert("deprecated".into(), json!(true));
            }
        }
        if let Some(overrides) = &self.overrides {
            apply_override(schema, overrides.clone());
        }
    }
}

/// Merges an `@openapi` override into a reflected schema. An override that
/// defines the structure itself (a virtual type on a newtype such as
/// `struct Wrapper<T>(T)`) replaces the reflected reference, also the one of
/// an `Option<Newtype>`, which stays nullable.
fn apply_override(schema: &mut Value, value: Map<String, Value>) {
    let structural = STRUCTURE_KEYS.iter().any(|k| value.contains_key(*k));
    let mut renullable = false;
    if structural
        && let Value::Object(map) = schema
        && map.remove("$ref").is_none()
        && is_nullable_reference(map)
    {
        map.remove("anyOf");
        renullable = true;
    }
    merge(schema, Value::Object(value), Policy::Override);
    if renullable {
        *schema = nullable(std::mem::take(schema));
    }
}

/// `{anyOf: [{$ref: …}, {type: null}]}`: a nullable reference.
fn is_nullable_reference(map: &Map<String, Value>) -> bool {
    match map
        .get("anyOf")
        .and_then(Value::as_array)
        .map(Vec::as_slice)
    {
        Some([first, second]) => {
            let null = json!({ "type": "null" });
            let reference = |v: &Value| v.get("$ref").is_some();
            (reference(first) && *second == null) || (*first == null && reference(second))
        }
        _ => false,
    }
}

/// The type parameters of a generic item (`T` of `Page<T>`): such items are
/// blueprints, instantiated where they are used (`Page<User>`).
fn type_params(generics: &syn::Generics) -> Option<Vec<String>> {
    let params: Vec<String> = generics
        .type_params()
        .map(|p| p.ident.to_string())
        .collect();
    (!params.is_empty()).then_some(params)
}

/// The value of an integer literal discriminant (`3`, `-1`, `0x10`).
fn integer_literal(expr: &syn::Expr) -> Option<i128> {
    match expr {
        syn::Expr::Lit(syn::ExprLit {
            lit: syn::Lit::Int(int),
            ..
        }) => int.base10_parse().ok(),
        syn::Expr::Unary(syn::ExprUnary {
            op: syn::UnOp::Neg(_),
            expr,
            ..
        }) => integer_literal(expr)?.checked_neg(),
        syn::Expr::Paren(syn::ExprParen { expr, .. })
        | syn::Expr::Group(syn::ExprGroup { expr, .. }) => integer_literal(expr),
        _ => None,
    }
}

/// An integer as a JSON number (beyond 64 bits: its decimal text).
fn integer_value(value: i128) -> Value {
    i64::try_from(value)
        .map(Value::from)
        .or_else(|_| u64::try_from(value).map(Value::from))
        .unwrap_or_else(|_| json!(value.to_string()))
}

/// `#[serde(tag = "t")]` on a struct: serde writes the tag (the struct's
/// serde name) before the fields.
fn struct_tag(schema: &mut Value, tag: &str, value: &str) {
    let Value::Object(map) = schema else { return };
    let mut properties = Map::new();
    properties.insert(
        tag.to_string(),
        json!({ "type": "string", "enum": [value] }),
    );
    if let Some(Value::Object(fields)) = map.remove("properties") {
        properties.extend(fields);
    }
    let mut required = vec![json!(tag)];
    if let Some(Value::Array(fields)) = map.remove("required") {
        required.extend(fields);
    }
    // Keep the keyword order of reflected objects: type, properties, required.
    let rest = std::mem::take(map);
    for (key, value) in rest {
        map.insert(key.clone(), value);
        if key == "type" {
            map.insert(
                "properties".into(),
                Value::Object(std::mem::take(&mut properties)),
            );
            map.insert(
                "required".into(),
                Value::Array(std::mem::take(&mut required)),
            );
        }
    }
}

/// `[A, B]`: a fixed-length array of the member schemas.
fn tuple(members: Vec<Value>) -> Value {
    let len = members.len();
    json!({
        "type": "array",
        "prefixItems": members,
        "items": false,
        "minItems": len,
        "maxItems": len,
    })
}

/// Whether serde can put the value next to a tag: references (structs) and maps.
fn is_object_like(schema: &Value) -> bool {
    schema.get("$ref").is_some()
        || schema.get("type") == Some(&json!("object"))
        || schema.get("allOf").is_some()
}

/// The schema of a field serialized with a known `with` module.
fn with_module(path: &str) -> Option<Value> {
    let segments: Vec<&str> = path.split("::").map(str::trim).collect();
    let has = |name: &str| segments.contains(&name);
    let timestamp = [
        "ts_seconds",
        "ts_milliseconds",
        "ts_microseconds",
        "ts_nanoseconds",
    ];
    Some(
        if timestamp
            .iter()
            .any(|s| segments.iter().any(|seg| seg.starts_with(s)))
            || has("timestamp")
        {
            json!({ "type": "integer", "format": "int64" })
        } else if has("rfc3339") || has("iso8601") {
            json!({ "type": "string", "format": "date-time" })
        } else if has("rfc2822") || has("humantime_serde") || has("display_fromstr") || has("hex") {
            json!({ "type": "string" })
        } else if segments.iter().any(|s| s.starts_with("base64")) {
            json!({ "type": "string", "format": "byte" })
        } else {
            return None;
        },
    )
}

/// A [`Item::Blueprint`] when the item has type parameters (`blueprint`),
/// otherwise a [`Item::Schema`].
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
