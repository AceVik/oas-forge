//! Schemas reflected from structs, enums and type aliases.
//!
//! The schemas describe what serde produces (ADR 0002): field and variant
//! names after `rename`/`rename_all`, skipped fields left out, `flatten`,
//! `transparent`, newtype and tuple structs, and all four enum
//! representations. Validation attributes add their keywords.

use super::{BlueprintItem, DocumentItem, Item, Options, SchemaItem};
use crate::attrs::directives::{ItemDocs, ItemKind};
use crate::attrs::rename::RenameRule;
use crate::attrs::serde::SerdeAttrs;
use crate::attrs::validate::Constraints;
use crate::diagnostics::{Code, Diagnostic, Diagnostics, Location};
use crate::docs::{doc_lines, line_of};
use crate::merge::{Policy, merge};
use crate::schema::nullable;
use crate::schema::types::{MappedType, TypeExpr, TypeMapper};
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
    name: String,
    rename_all: Option<RenameRule>,
    docs: ItemDocs,
    serde: SerdeAttrs,
    deprecated: bool,
}

/// The outcome of reflecting one named field.
enum Field {
    /// Left out (`#[serde(skip)]`).
    Skip,
    /// A property of the object.
    Property {
        name: String,
        schema: Value,
        required: bool,
    },
    /// `#[serde(flatten)]` of a struct or enum: its properties are merged in.
    Flatten(Value),
    /// `#[serde(flatten)]` of a map: its entries are additional properties.
    FlattenMap(Value),
}

/// The serialized form of an enum variant's data.
enum Content {
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
    External,
    Internal { tag: &'a str },
    Adjacent { tag: &'a str, content: &'a str },
    Untagged,
}

pub(super) struct Reflector<'a> {
    path: &'a Path,
    types: &'a TypeMapper,
    variables: &'a Variables,
    diagnostics: &'a mut Diagnostics,
}

impl<'a> Reflector<'a> {
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
        }
    }

    fn location(&self, line: usize) -> Location {
        Location::new(self.path, line)
    }

    fn naming(&self, attrs: &[syn::Attribute], default: &str, kind: ItemKind) -> Naming {
        let serde = SerdeAttrs::parse(attrs);
        let docs = ItemDocs::parse(&doc_lines(attrs), kind);
        let name = docs
            .rename
            .clone()
            .or_else(|| serde.rename.clone())
            .unwrap_or_else(|| default.to_string());
        Naming {
            name,
            rename_all: docs.rename_all.or(serde.rename_all),
            deprecated: attrs.iter().any(|a| a.path().is_ident("deprecated")),
            docs,
            serde,
        }
    }

    /// Adds description, `deprecated`, `@example`s and the `@openapi`
    /// override YAML.
    fn finish(&mut self, schema: &mut Value, naming: &Naming, what: &str) {
        if !naming.docs.description.is_empty()
            && let Value::Object(map) = schema
        {
            map.insert("description".into(), json!(naming.docs.description));
        }
        if !naming.docs.examples.is_empty()
            && let Value::Object(map) = schema
        {
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
                    match yaml::parse(&item) {
                        Ok(Value::Array(values)) => values
                            .into_iter()
                            .next()
                            .filter(|value| !value.is_null())
                            .unwrap_or_else(|| json!(text)),
                        _ => json!(text),
                    }
                })
                .collect();
            map.insert("examples".into(), Value::Array(examples));
        }
        if naming.deprecated
            && let Value::Object(map) = schema
        {
            map.insert("deprecated".into(), json!(true));
        }
        let Some(text) = naming.docs.override_text() else {
            return;
        };
        let start = self.location(naming.docs.override_line().unwrap_or(1));
        let text = lexical::substitute(&text, self.variables, &start, self.diagnostics);
        match yaml::parse(&text) {
            Ok(Value::Null) => {}
            Ok(value) => {
                // An override that defines the structure itself (a virtual
                // type on a newtype such as `struct Wrapper<T>(T)`) replaces
                // the reflected reference.
                let structural = value
                    .as_object()
                    .is_some_and(|map| STRUCTURE_KEYS.iter().any(|k| map.contains_key(*k)));
                if structural && let Value::Object(map) = schema {
                    map.remove("$ref");
                }
                merge(schema, value, Policy::Override);
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
    }

    /// The schema of a container serialized as another type (`into`/`from`).
    fn conversion(&self, serde: &SerdeAttrs) -> Option<Value> {
        let target = serde.into.as_ref().or(serde.from.as_ref())?;
        let ty = TypeExpr::parse(target)?;
        Some(self.types.map(&ty).into_schema())
    }

    /// The validation keywords of a field, reporting values that are not literals.
    fn constraints(&mut self, attrs: &[syn::Attribute], line: usize) -> Constraints {
        let (constraints, problems) = Constraints::parse(attrs);
        for problem in problems {
            self.diagnostics.push(
                Diagnostic::new(
                    Code::NonLiteralConstraint,
                    format!("{problem} is not a literal; the constraint is not documented"),
                )
                .at(self.location(line)),
            );
        }
        constraints
    }

    /// The type of a field, honouring known `with` modules.
    fn field_type(&self, ty: &syn::Type, serde: &SerdeAttrs) -> MappedType {
        let mut mapped = self.types.map(&TypeExpr::from_syn(ty));
        if let Some(schema) = serde.with.as_deref().and_then(with_module) {
            mapped.schema = schema;
        }
        mapped
    }

    /// The schema of an unnamed field (newtype and tuple members): type,
    /// validation and nullability. `None` when the field is skipped.
    fn unnamed_field(&mut self, field: &syn::Field) -> Option<Value> {
        let serde = SerdeAttrs::parse(&field.attrs);
        if serde.is_skipped() {
            return None;
        }
        let mapped = self.field_type(&field.ty, &serde);
        let constraints = self.constraints(&field.attrs, line_of(field.span()));
        let mut schema = mapped.schema;
        constraints.apply(&mut schema);
        if mapped.nullable && !constraints.requires_value() {
            schema = nullable(schema);
        }
        Some(schema)
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
        let name = if naming.name == ident {
            rule.map_or_else(|| ident.clone(), |rule| rule.apply_to_field(&ident))
        } else {
            naming.name.clone()
        };

        let mapped = self.field_type(&field.ty, serde);
        if serde.flatten {
            let schema = mapped.schema;
            let map_values = schema.get("additionalProperties").cloned().filter(|_| {
                schema.get("type") == Some(&json!("object")) && schema.get("properties").is_none()
            });
            return match map_values {
                Some(values) => Field::FlattenMap(values),
                // A flattened `Option<T>` may be absent as a whole.
                None if mapped.nullable => Field::Flatten(json!({ "anyOf": [schema, {}] })),
                None => Field::Flatten(schema),
            };
        }

        let constraints = self.constraints(&field.attrs, line_of(field.span()));
        let mut schema = mapped.schema;
        constraints.apply(&mut schema);
        let forced = constraints.requires_value();
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
        self.finish(&mut schema, &naming, &format!("field `{ident}`"));
        let required = (mapped.required || forced)
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
        if !flattened.is_empty() {
            schema.insert("allOf".into(), Value::Array(flattened.clone()));
        }
        match additional {
            Some(values) => {
                schema.insert("additionalProperties".into(), values);
            }
            None if container.deny_unknown_fields && flattened.is_empty() => {
                schema.insert("additionalProperties".into(), json!(false));
            }
            None if container.deny_unknown_fields => {
                schema.insert("unevaluatedProperties".into(), json!(false));
            }
            None => {}
        }
        Value::Object(schema)
    }

    /// Reflects a struct with an `@openapi` directive.
    pub(super) fn structure(&mut self, item: &syn::ItemStruct) -> Vec<Item> {
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
                syn::Fields::Named(fields) if serde.transparent => {
                    let field = fields
                        .named
                        .iter()
                        .find(|f| !SerdeAttrs::parse(&f.attrs).is_skipped());
                    field
                        .and_then(|f| self.unnamed_field(f))
                        .unwrap_or_else(|| json!({}))
                }
                syn::Fields::Named(fields) if !fields.named.is_empty() => {
                    self.object(fields, naming.rename_all, serde, false)
                }
                syn::Fields::Unnamed(fields) => {
                    let members: Vec<Value> = fields
                        .unnamed
                        .iter()
                        .filter_map(|f| self.unnamed_field(f))
                        .collect();
                    match members.as_slice() {
                        // A newtype struct is its single field.
                        [single] => single.clone(),
                        _ => tuple(members),
                    }
                }
                _ => {
                    if let Some(document) = self.root_document(&naming.docs) {
                        return vec![Item::Document(document)];
                    }
                    match (&item.fields, naming.docs.override_text()) {
                        // `struct S {}` is an empty object.
                        (syn::Fields::Named(_), _) => json!({ "type": "object" }),
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

    /// Reflects a type alias (always exported, unless `@openapi skip`).
    pub(super) fn alias(&mut self, item: &syn::ItemType) -> Option<Item> {
        let rust_name = item.ident.unraw().to_string();
        let naming = self.naming(&item.attrs, &rust_name, ItemKind::Member);
        if naming.docs.skip {
            return None;
        }
        let mut schema = self.types.map(&TypeExpr::from_syn(&item.ty)).into_schema();
        self.finish(&mut schema, &naming, &format!("type alias `{rust_name}`"));
        Some(Item::Schema(SchemaItem {
            name: naming.name,
            rust_name,
            schema,
            location: self.location(line_of(item.span())),
        }))
    }

    /// Reflects an enum with an `@openapi` directive.
    pub(super) fn enumeration(&mut self, item: &syn::ItemEnum) -> Vec<Item> {
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

        let serde = naming.serde.clone();
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
            Representation::Internal { tag } => {
                self.tagged(&naming, &variants, tag, None, &rust_name, &mut items)
            }
            Representation::Adjacent { tag, content } => self.tagged(
                &naming,
                &variants,
                tag,
                Some(content),
                &rust_name,
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

    /// The serialized name of a variant.
    fn variant_name(variant: &syn::Variant, naming: &Naming, rule: Option<RenameRule>) -> String {
        let ident = variant.ident.unraw().to_string();
        if naming.name == ident {
            rule.map_or(ident.clone(), |rule| rule.apply_to_variant(&ident))
        } else {
            naming.name.clone()
        }
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
                Content::Struct(self.object(fields, rule, &naming.serde, always_required))
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
        let units: Vec<Value> = tagged
            .iter()
            .filter(|(v, _)| matches!(v.fields, syn::Fields::Unit))
            .map(|(v, n)| json!(Self::variant_name(v, n, rule)))
            .collect();
        if units.len() == variants.len() {
            return if units.is_empty() {
                json!({ "type": "string" })
            } else {
                json!({ "type": "string", "enum": units })
            };
        }
        let mut alternatives = Vec::new();
        if !units.is_empty() {
            alternatives.push(json!({ "type": "string", "enum": units }));
        }
        for (variant, naming) in &tagged {
            let name = Self::variant_name(variant, naming, rule);
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
    /// component schema per variant, combined with a discriminator.
    fn tagged(
        &mut self,
        container: &Naming,
        variants: &[(&syn::Variant, Naming)],
        tag: &str,
        content_key: Option<&str>,
        rust_name: &str,
        items: &mut Vec<Item>,
    ) -> Value {
        let mut refs = Vec::new();
        let mut mapping = Map::new();
        let mut untagged = Vec::new();

        for (variant, naming) in variants {
            if naming.serde.untagged {
                untagged.push(self.untagged_alternative(variant, naming, container));
                continue;
            }
            let tag_value = Self::variant_name(variant, naming, container.rename_all);
            let schema_name = format!("{}{}", container.name, variant.ident.unraw());
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
            let mut all_of = Vec::new();
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
                (None, Content::Struct(inner)) => {
                    if let Some(props) = inner.get("properties").and_then(Value::as_object) {
                        properties.extend(props.clone());
                    }
                    if let Some(req) = inner.get("required").and_then(Value::as_array) {
                        required.extend(req.iter().cloned());
                    }
                    for key in ["allOf", "additionalProperties", "unevaluatedProperties"] {
                        if let Some(value) = inner.get(key) {
                            all_of.push(json!({ key: value }));
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

            let mut schema =
                json!({ "type": "object", "properties": properties, "required": required });
            if !all_of.is_empty()
                && let Value::Object(map) = &mut schema
            {
                map.insert("allOf".into(), Value::Array(all_of));
            }
            self.finish(&mut schema, naming, &format!("variant `{}`", variant.ident));
            items.push(Item::Schema(SchemaItem {
                name: schema_name,
                rust_name: format!("{rust_name}{}", variant.ident.unraw()),
                schema,
                location: self.location(variant_line),
            }));
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

/// The type parameters of a generic item (`T` of `Page<T>`): such items are
/// blueprints, instantiated where they are used (`Page<User>`).
fn type_params(generics: &syn::Generics) -> Option<Vec<String>> {
    let params: Vec<String> = generics
        .type_params()
        .map(|p| p.ident.to_string())
        .collect();
    (!params.is_empty()).then_some(params)
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
