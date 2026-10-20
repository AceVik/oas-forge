//! Extraction of annotations from Rust source code.
//!
//! [`extract`] parses one Rust file and returns every annotation as a typed
//! [`Item`]: schemas reflected from structs, enums and type aliases, free YAML
//! documents, named YAML schemas, fragments, blueprints and DSL routes.

mod blocks;
mod constants;
mod reflect;
mod signature;

use super::diagnostics::{Diagnostics, Location};
use super::docs::{DocLine, doc_lines, line_of};
use super::dsl::{self, Signature};
use super::error::{Error, Result};
use super::schema::TypeMapper;
use super::source::SourceFile;
use super::template::Variables;
use serde_json::Value;
use std::path::Path;
use syn::spanned::Spanned;
use syn::visit::{self, Visit};

/// An annotation found in Rust source code.
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub enum Item {
    /// A schema reflected from a struct, enum or type alias.
    Schema(SchemaItem),
    /// A free OpenAPI YAML block (`@openapi` on functions, modules, file docs).
    Document(DocumentItem),
    /// A named schema written as YAML (`@openapi-type Name`).
    NamedSchema(NamedSchemaItem),
    /// A reusable text fragment (`@openapi-fragment Name(params)`).
    Fragment(FragmentItem),
    /// A generic schema template (`@openapi<T>` on a struct or enum).
    Blueprint(BlueprintItem),
    /// An operation written in the route DSL (`@route`).
    Route(RouteItem),
    /// A regular expression constant (`static RE: LazyLock<Regex> = ...`),
    /// referenced by validation attributes.
    Pattern(PatternItem),
    /// A free AsyncAPI YAML block (`@asyncapi`).
    AsyncApi(DocumentItem),
    /// An AsyncAPI channel in the channel DSL (`@channel`).
    Channel(ChannelItem),
}

impl Item {
    /// Where the annotation starts.
    pub fn location(&self) -> &Location {
        match self {
            Self::Schema(i) => &i.location,
            Self::Document(i) => &i.location,
            Self::NamedSchema(i) => &i.location,
            Self::Fragment(i) => &i.location,
            Self::Blueprint(i) => &i.location,
            Self::Route(i) => &i.location,
            Self::Pattern(i) => &i.location,
            Self::AsyncApi(i) => &i.location,
            Self::Channel(i) => &i.location,
        }
    }
}

/// A schema reflected from a Rust item.
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub struct SchemaItem {
    /// The schema name (after `rename`).
    pub name: String,
    /// The Rust identifier.
    pub rust_name: String,
    /// The JSON Schema; references to other types are `{"$ref": "$Name"}`.
    pub schema: Value,
    /// Where the item is declared.
    pub location: Location,
}

/// A free YAML block.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct DocumentItem {
    /// The YAML text (unindented).
    pub text: String,
    /// Where the YAML text starts.
    pub location: Location,
    /// Tags of the enclosing modules, applied to operations without tags.
    pub tags: Vec<String>,
}

/// A named schema written as YAML.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct NamedSchemaItem {
    /// The schema name.
    pub name: String,
    /// The schema YAML.
    pub text: String,
    /// Where the YAML text starts.
    pub location: Location,
}

/// A reusable fragment.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct FragmentItem {
    /// The fragment name.
    pub name: String,
    /// Parameter names, substituted as `{{param}}`.
    pub params: Vec<String>,
    /// The body text.
    pub body: String,
    /// Where the fragment is declared.
    pub location: Location,
}

/// A generic schema template.
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub struct BlueprintItem {
    /// The template name (after `rename`).
    pub name: String,
    /// The Rust identifier.
    pub rust_name: String,
    /// Type parameter names, referenced as `$T` in the schema.
    pub params: Vec<String>,
    /// The schema with `$T` placeholders.
    pub schema: Value,
    /// Where the item is declared.
    pub location: Location,
}

/// An operation in the route DSL.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct RouteItem {
    /// The doc lines of the route block.
    pub lines: Vec<String>,
    /// Source line of each entry of `lines`.
    pub line_numbers: Vec<usize>,
    /// The default operationId (function name, or derived from method and path).
    pub operation_id: Option<String>,
    /// Tags of the enclosing modules, applied when the route has no `@tag`.
    pub tags: Vec<String>,
    /// `@route-defaults` blocks of the enclosing modules, outermost first;
    /// each starts with its `@route-defaults` header.
    pub defaults: Vec<String>,
    /// What the handler's signature says (signature inference).
    pub signature: Option<Signature>,
    /// Where the route block starts.
    pub location: Location,
}

/// An AsyncAPI channel block.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct ChannelItem {
    /// The doc lines of the block.
    pub lines: Vec<String>,
    /// Source line of each entry of `lines`.
    pub line_numbers: Vec<usize>,
    /// The operationId (the function name), if any.
    pub operation_id: Option<String>,
    /// Where the block starts.
    pub location: Location,
}

/// A regular expression constant.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct PatternItem {
    /// The name of the constant.
    pub name: String,
    /// The regular expression.
    pub pattern: String,
    /// Where the constant is defined.
    pub location: Location,
}

/// How [`extract`] reads annotations.
#[derive(Debug, Clone, Copy)]
#[non_exhaustive]
pub struct Options<'a> {
    /// How Rust types map to schemas.
    pub types: &'a TypeMapper,
    /// Values of `{{NAME}}` placeholders in type overrides and `@example`s.
    /// Free blocks and routes are expanded later, together with `@insert`.
    pub variables: &'a Variables,
}

impl Default for Options<'_> {
    fn default() -> Self {
        static NONE: Variables = Variables::new();
        Self {
            types: TypeMapper::builtin(),
            variables: &NONE,
        }
    }
}

impl<'a> Options<'a> {
    /// The built-in type table and no variables.
    pub fn new() -> Self {
        Self::default()
    }

    /// Sets the type mapping.
    pub fn types(mut self, types: &'a TypeMapper) -> Self {
        self.types = types;
        self
    }

    /// Sets the values of `{{NAME}}` placeholders.
    pub fn variables(mut self, variables: &'a Variables) -> Self {
        self.variables = variables;
        self
    }
}

/// Parses Rust `source` and returns its annotations in source order.
///
/// # Errors
/// [`Error::Parse`] when the file is not valid Rust.
pub fn extract(
    source: &SourceFile,
    options: Options<'_>,
    diagnostics: &mut Diagnostics,
) -> Result<Vec<Item>> {
    extract_str(source.path(), source.text(), options, diagnostics)
}

/// Parses Rust source text and returns its annotations in source order.
///
/// # Errors
/// [`Error::Parse`] when the text is not valid Rust.
pub fn extract_str(
    path: &Path,
    text: &str,
    options: Options<'_>,
    diagnostics: &mut Diagnostics,
) -> Result<Vec<Item>> {
    let file = syn::parse_file(text).map_err(|e| Error::Parse {
        location: Location::new(path, line_of(e.span())).with_column(e.span().start().column + 1),
        message: e.to_string(),
    })?;
    let mut visitor = Extractor {
        path,
        options,
        items: Vec::new(),
        tags: Vec::new(),
        defaults: Vec::new(),
        diagnostics,
    };
    visitor.visit_file(&file);
    Ok(visitor.items)
}

struct Extractor<'a> {
    path: &'a Path,
    options: Options<'a>,
    items: Vec<Item>,
    tags: Vec<String>,
    defaults: Vec<String>,
    diagnostics: &'a mut Diagnostics,
}

impl Extractor<'_> {
    fn location(&self, line: usize) -> Location {
        Location::new(self.path, line)
    }

    /// Handles the doc comment of a function or method: a route or free blocks.
    fn function(&mut self, attrs: &[syn::Attribute], sig: &syn::Signature, line: usize) {
        let lines = doc_lines(attrs);
        let texts: Vec<String> = lines.iter().map(|l| l.text.clone()).collect();
        if dsl::is_channel(&texts) {
            self.items.push(Item::Channel(ChannelItem {
                line_numbers: lines.iter().map(|l| l.line).collect(),
                lines: texts,
                operation_id: Some(sig.ident.to_string()),
                location: self.location(lines.first().map_or(line, |l| l.line)),
            }));
        } else if dsl::is_route(&texts) {
            let mut texts = texts;
            // `#[deprecated]` on a handler deprecates its operation.
            if attrs.iter().any(|a| a.path().is_ident("deprecated")) {
                texts.push("@deprecated".into());
            }
            self.items.push(Item::Route(RouteItem {
                lines: texts,
                line_numbers: lines.iter().map(|l| l.line).collect(),
                operation_id: Some(sig.ident.to_string()),
                tags: self.tags.clone(),
                defaults: self.defaults.clone(),
                signature: signature::infer(sig),
                location: self.location(lines.first().map_or(line, |l| l.line)),
            }));
        } else {
            self.blocks(&lines);
        }
    }

    /// Reflects a struct, enum or type alias; fragments in its doc comments
    /// come first, in source order.
    fn reflect(&mut self, reflect: impl FnOnce(&mut reflect::Reflector<'_>) -> Vec<Item>) {
        let mut reflector = reflect::Reflector::new(self.path, self.options, self.diagnostics);
        let items = reflect(&mut reflector);
        let fragments = reflector.take_fragments();
        self.items.extend(fragments);
        self.items.extend(items);
    }

    fn blocks(&mut self, lines: &[DocLine]) {
        let items = blocks::item_blocks(self.path, lines, &self.tags, self.diagnostics);
        self.items.extend(items);
    }
}

impl<'ast> Visit<'ast> for Extractor<'_> {
    fn visit_file(&mut self, file: &'ast syn::File) {
        let items = blocks::file_blocks(self.path, &file.attrs, self.diagnostics);
        self.items.extend(items);
        visit::visit_file(self, file);
    }

    fn visit_item_fn(&mut self, item: &'ast syn::ItemFn) {
        self.function(&item.attrs, &item.sig, line_of(item.span()));
        visit::visit_item_fn(self, item);
    }

    fn visit_impl_item_fn(&mut self, item: &'ast syn::ImplItemFn) {
        self.function(&item.attrs, &item.sig, line_of(item.span()));
        visit::visit_impl_item_fn(self, item);
    }

    fn visit_item_mod(&mut self, item: &'ast syn::ItemMod) {
        let lines = doc_lines(&item.attrs);
        let saved = (self.tags.len(), self.defaults.len());
        self.tags.extend(blocks::module_tags(&lines));
        let defaults = blocks::route_defaults(&lines);
        if !defaults.is_empty() {
            // The header separates the modules: inner defaults replace the
            // tags and security of outer ones.
            self.defaults.push("@route-defaults".to_string());
            self.defaults.extend(defaults);
        }
        self.blocks(&lines);
        visit::visit_item_mod(self, item);
        self.tags.truncate(saved.0);
        self.defaults.truncate(saved.1);
    }

    fn visit_item_struct(&mut self, item: &'ast syn::ItemStruct) {
        self.reflect(|reflector| reflector.structure(item));
        visit::visit_item_struct(self, item);
    }

    fn visit_item_enum(&mut self, item: &'ast syn::ItemEnum) {
        self.reflect(|reflector| reflector.enumeration(item));
        visit::visit_item_enum(self, item);
    }

    fn visit_item_type(&mut self, item: &'ast syn::ItemType) {
        self.reflect(|reflector| reflector.alias(item).into_iter().collect());
        visit::visit_item_type(self, item);
    }

    fn visit_item_static(&mut self, item: &'ast syn::ItemStatic) {
        self.items
            .extend(constants::from_item(self.path, &item.ident, &item.expr));
        visit::visit_item_static(self, item);
    }

    fn visit_item_const(&mut self, item: &'ast syn::ItemConst) {
        self.items
            .extend(constants::from_item(self.path, &item.ident, &item.expr));
        visit::visit_item_const(self, item);
    }

    fn visit_item_macro(&mut self, item: &'ast syn::ItemMacro) {
        self.items
            .extend(constants::from_macro(self.path, &item.mac));
        visit::visit_item_macro(self, item);
    }
}
