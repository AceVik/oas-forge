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
use std::path::{Component, Path, PathBuf};
use syn::ext::IdentExt;
use syn::spanned::Spanned;
use syn::visit::{self, Visit};

/// An annotation found in Rust source code.
///
/// # Examples
///
/// ```
/// use oas_forge::diagnostics::Diagnostics;
/// use oas_forge::extract::{self, Item, Options};
/// use std::path::Path;
///
/// let source = r#"
///     /// @openapi
///     pub struct User {
///         pub id: u64,
///     }
///
///     /// Get a user
///     /// @route GET /users/{id: u64}
///     /// @return 200: $User
///     pub async fn get_user() {}
/// "#;
/// let items = extract::extract_str(
///     Path::new("src/lib.rs"),
///     source,
///     Options::new(),
///     &mut Diagnostics::new(),
/// )?;
/// let kinds: Vec<&str> = items
///     .iter()
///     .map(|item| match item {
///         Item::Schema(_) => "schema",
///         Item::Route(_) => "route",
///         _ => "other",
///     })
///     .collect();
/// assert_eq!(kinds, ["schema", "route"]);
/// # Ok::<(), oas_forge::Error>(())
/// ```
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
// Items are made once per annotation and moved into the pipeline; boxing
// `Route` would change the public API to save a few bytes.
#[allow(clippy::large_enum_variant)]
pub enum Item {
    /// A schema reflected from a struct, enum or type alias.
    Schema(SchemaItem),
    /// A free OpenAPI YAML block (`@openapi` on functions, modules, file
    /// docs, or on a field-less struct whose YAML has document keys such as
    /// `openapi` or `info`).
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
    /// A module declared with `mod name;`, whose items are in another file.
    Module(ModuleItem),
    /// A type alias without `@openapi`: no schema of its own, it stands for
    /// its target wherever its name is used.
    Alias(AliasItem),
}

impl Item {
    /// Where the annotation starts.
    ///
    /// # Examples
    ///
    /// ```
    /// use oas_forge::diagnostics::Diagnostics;
    /// use oas_forge::extract::{self, Options};
    /// use std::path::Path;
    ///
    /// let source = r#"/// @openapi
    /// pub struct User {
    ///     pub id: u64,
    /// }
    ///
    /// /// @openapi
    /// pub struct Group {}
    /// "#;
    /// let items = extract::extract_str(
    ///     Path::new("src/lib.rs"),
    ///     source,
    ///     Options::new(),
    ///     &mut Diagnostics::new(),
    /// )?;
    /// let lines: Vec<usize> = items.iter().map(|item| item.location().line()).collect();
    /// assert_eq!(lines, [1, 6]);
    /// assert_eq!(items[0].location().file(), Path::new("src/lib.rs"));
    /// # Ok::<(), oas_forge::Error>(())
    /// ```
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
            Self::Module(i) => &i.location,
            Self::Alias(i) => &i.location,
        }
    }
}

/// A schema reflected from a Rust item.
///
/// # Examples
///
/// ```
/// use oas_forge::diagnostics::Diagnostics;
/// use oas_forge::extract::{self, Item, Options};
/// use serde_json::json;
/// use std::path::Path;
///
/// let source = r#"
/// /// A user.
/// /// @openapi
/// #[derive(serde::Serialize)]
/// #[serde(rename = "UserDto", rename_all = "camelCase")]
/// pub struct User {
///     pub user_id: u64,
///     pub nick: Option<String>,
/// }
/// "#;
/// let items = extract::extract_str(
///     Path::new("src/lib.rs"),
///     source,
///     Options::new(),
///     &mut Diagnostics::new(),
/// )?;
/// let Some(Item::Schema(user)) = items.first() else {
///     return Err("no schema".into());
/// };
/// assert_eq!((user.name.as_str(), user.rust_name.as_str()), ("UserDto", "User"));
/// assert_eq!(
///     user.schema,
///     json!({
///         "type": "object",
///         "properties": {
///             "userId": { "type": "integer", "format": "int64", "minimum": 0 },
///             "nick": { "type": ["string", "null"] },
///         },
///         // Only serialized: serde always writes `nick`, maybe as `null`.
///         "required": ["userId", "nick"],
///         "description": "A user.",
///     })
/// );
/// assert_eq!(user.location.line(), 2);
/// # Ok::<(), Box<dyn std::error::Error>>(())
/// ```
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
///
/// # Examples
///
/// ```
/// use oas_forge::diagnostics::Diagnostics;
/// use oas_forge::extract::{self, Item, Options};
/// use std::path::Path;
///
/// let source = r#"
/// /// @openapi
/// /// tags: [Users]
/// /// paths:
/// ///   /users/count:
/// ///     get:
/// ///       responses:
/// ///         '200': { description: OK }
/// mod users {}
/// "#;
/// let items = extract::extract_str(
///     Path::new("src/lib.rs"),
///     source,
///     Options::new(),
///     &mut Diagnostics::new(),
/// )?;
/// let Some(Item::Document(block)) = items.first() else {
///     return Err("no document".into());
/// };
/// assert!(block.text.starts_with("tags: [Users]\npaths:\n  /users/count:"));
/// assert_eq!(block.location.line(), 3);
/// // The module's tags apply to the operations of its blocks.
/// assert_eq!(block.tags, ["Users"]);
/// # Ok::<(), Box<dyn std::error::Error>>(())
/// ```
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
///
/// # Examples
///
/// ```
/// use oas_forge::diagnostics::Diagnostics;
/// use oas_forge::extract::{self, Item, Options};
/// use std::path::Path;
///
/// let source = r#"
/// /// @openapi-type Money
/// /// type: string
/// /// format: decimal
/// fn _schemas() {}
/// "#;
/// let items = extract::extract_str(
///     Path::new("src/lib.rs"),
///     source,
///     Options::new(),
///     &mut Diagnostics::new(),
/// )?;
/// let Some(Item::NamedSchema(money)) = items.first() else {
///     return Err("no named schema".into());
/// };
/// assert_eq!(money.name, "Money");
/// assert_eq!(money.text, "type: string\nformat: decimal");
/// assert_eq!(money.location.line(), 3);
/// # Ok::<(), Box<dyn std::error::Error>>(())
/// ```
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
///
/// # Examples
///
/// ```
/// use oas_forge::diagnostics::Diagnostics;
/// use oas_forge::extract::{self, Item, Options};
/// use std::path::Path;
///
/// let source = r#"
/// /// @openapi-fragment Paged(limit)
/// /// - name: limit
/// ///   in: query
/// ///   schema: { type: integer, maximum: {{limit}} }
/// fn _fragments() {}
/// "#;
/// let items = extract::extract_str(
///     Path::new("src/lib.rs"),
///     source,
///     Options::new(),
///     &mut Diagnostics::new(),
/// )?;
/// let Some(Item::Fragment(paged)) = items.first() else {
///     return Err("no fragment".into());
/// };
/// assert_eq!(paged.name, "Paged");
/// assert_eq!(paged.params, ["limit"]);
/// assert_eq!(
///     paged.body,
///     "- name: limit\n  in: query\n  schema: { type: integer, maximum: {{limit}} }"
/// );
/// assert_eq!(paged.location.line(), 2);
/// # Ok::<(), Box<dyn std::error::Error>>(())
/// ```
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
///
/// # Examples
///
/// ```
/// use oas_forge::diagnostics::Diagnostics;
/// use oas_forge::extract::{self, Item, Options};
/// use serde_json::json;
/// use std::path::Path;
///
/// let source = r#"
/// /// @openapi
/// pub struct Page<T> {
///     pub items: Vec<T>,
///     pub total: u64,
/// }
/// "#;
/// let items = extract::extract_str(
///     Path::new("src/lib.rs"),
///     source,
///     Options::new(),
///     &mut Diagnostics::new(),
/// )?;
/// let Some(Item::Blueprint(page)) = items.first() else {
///     return Err("no blueprint".into());
/// };
/// assert_eq!(page.name, "Page");
/// assert_eq!(page.params, ["T"]);
/// assert_eq!(
///     page.schema["properties"]["items"],
///     json!({ "type": "array", "items": { "$ref": "$T" } })
/// );
/// # Ok::<(), Box<dyn std::error::Error>>(())
/// ```
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
///
/// # Examples
///
/// ```
/// use oas_forge::diagnostics::Diagnostics;
/// use oas_forge::extract::{self, Item, Options};
/// use std::path::Path;
///
/// let source = r#"
/// /// Get a user
/// /// @route GET /users/{id}
/// pub async fn get_user(Path(id): Path<u64>) -> Json<User> {}
/// "#;
/// let items = extract::extract_str(
///     Path::new("src/lib.rs"),
///     source,
///     Options::new(),
///     &mut Diagnostics::new(),
/// )?;
/// let Some(Item::Route(route)) = items.first() else {
///     return Err("no route".into());
/// };
/// assert_eq!(route.lines, [" Get a user", " @route GET /users/{id}"]);
/// assert_eq!(route.line_numbers, [2, 3]);
/// assert_eq!(route.operation_id.as_deref(), Some("get_user"));
/// let signature = route.signature.as_ref().ok_or("no signature")?;
/// assert_eq!(signature.path.len(), 1);
/// assert_eq!(signature.path[0].to_string(), "u64");
/// assert_eq!(signature.response.as_ref().map(ToString::to_string).as_deref(), Some("User"));
/// # Ok::<(), Box<dyn std::error::Error>>(())
/// ```
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct RouteItem {
    /// The doc lines of the route block.
    pub lines: Vec<String>,
    /// Source line of each entry of `lines`.
    pub line_numbers: Vec<usize>,
    /// The default operationId: the function name; `None` for a block in
    /// file docs (`//!`), whose operationId is derived from method and path.
    pub operation_id: Option<String>,
    /// Tags of the enclosing modules, applied when the route has no `@tag`.
    pub tags: Vec<String>,
    /// `@route-defaults` blocks of the enclosing modules, outermost first;
    /// each starts with its `@route-defaults` header.
    pub defaults: Vec<String>,
    /// Source line of each entry of `defaults` (in the file of the route).
    pub default_line_numbers: Vec<usize>,
    /// What the handler's signature says (signature inference).
    pub signature: Option<Signature>,
    /// Where the route block starts.
    pub location: Location,
}

/// An AsyncAPI channel block.
///
/// # Examples
///
/// ```
/// use oas_forge::diagnostics::Diagnostics;
/// use oas_forge::extract::{self, Item, Options};
/// use std::path::Path;
///
/// let source = r#"
/// /// A user signed up.
/// /// @channel user/{id}/signedup
/// /// @send UserSignedUp: $User
/// fn publish_signup() {}
/// "#;
/// let items = extract::extract_str(
///     Path::new("src/lib.rs"),
///     source,
///     Options::new(),
///     &mut Diagnostics::new(),
/// )?;
/// let Some(Item::Channel(channel)) = items.first() else {
///     return Err("no channel".into());
/// };
/// assert_eq!(channel.lines[1], " @channel user/{id}/signedup");
/// assert_eq!(channel.line_numbers, [2, 3, 4]);
/// assert_eq!(channel.operation_id.as_deref(), Some("publish_signup"));
/// # Ok::<(), Box<dyn std::error::Error>>(())
/// ```
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

/// A module declared with `mod name;`: its items are in another file. The
/// generator applies the module's `@route-defaults` to the routes of that
/// file, and through its declarations in turn to the modules below it.
///
/// # Examples
///
/// ```
/// use oas_forge::diagnostics::Diagnostics;
/// use oas_forge::extract::{self, Item, Options};
/// use std::path::{Path, PathBuf};
///
/// let source = r#"
/// /// @route-defaults
/// /// @tag Decks
/// pub mod decks;
/// "#;
/// let items = extract::extract_str(
///     Path::new("src/routes/mod.rs"),
///     source,
///     Options::new(),
///     &mut Diagnostics::new(),
/// )?;
/// let Some(Item::Module(decks)) = items.first() else {
///     return Err("no module".into());
/// };
/// assert_eq!(decks.name, "decks");
/// assert_eq!(
///     decks.files,
///     [PathBuf::from("src/routes/decks.rs"), PathBuf::from("src/routes/decks/mod.rs")]
/// );
/// assert_eq!(decks.defaults, ["@route-defaults", " @tag Decks"]);
/// assert_eq!(decks.default_line_numbers, [2, 3]);
/// assert_eq!(decks.location.line(), 4);
/// # Ok::<(), Box<dyn std::error::Error>>(())
/// ```
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct ModuleItem {
    /// The module's name.
    pub name: String,
    /// The files that can hold the module, as Rust looks for them:
    /// `name.rs` and `name/mod.rs` in the directory of the declaring module,
    /// or the file of a `#[path]` attribute. They extend the path of the
    /// declaring file as given, without `.` and with `..` resolved where the
    /// path allows.
    pub files: Vec<PathBuf>,
    /// The `@route-defaults` blocks that apply to the module, outermost
    /// first: those of the declaring file (`//!`) and of the inline modules
    /// around the declaration, then the declaration's own. Each starts with
    /// its `@route-defaults` header.
    pub defaults: Vec<String>,
    /// Source line of each entry of `defaults` (in the declaring file).
    pub default_line_numbers: Vec<usize>,
    /// Where the declaration is (its `mod` keyword).
    pub location: Location,
}

/// A regular expression constant.
///
/// # Examples
///
/// ```
/// use oas_forge::diagnostics::Diagnostics;
/// use oas_forge::extract::{self, Item, Options};
/// use std::path::Path;
///
/// let source = r#"
/// static SLUG: LazyLock<Regex> =
///     LazyLock::new(|| Regex::new("^[a-z0-9-]+$").expect("valid pattern"));
/// "#;
/// let items = extract::extract_str(
///     Path::new("src/lib.rs"),
///     source,
///     Options::new(),
///     &mut Diagnostics::new(),
/// )?;
/// let Some(Item::Pattern(slug)) = items.first() else {
///     return Err("no pattern".into());
/// };
/// assert_eq!((slug.name.as_str(), slug.pattern.as_str()), ("SLUG", "^[a-z0-9-]+$"));
/// # Ok::<(), Box<dyn std::error::Error>>(())
/// ```
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

/// A type alias without `@openapi` (or with `@openapi skip`). It is not
/// exported; references to its name are replaced by its schema when the
/// references are resolved.
///
/// # Examples
///
/// ```
/// use oas_forge::diagnostics::Diagnostics;
/// use oas_forge::extract::{self, Item, Options};
/// use serde_json::json;
/// use std::path::Path;
///
/// let source = r#"
/// type Owner = User;
/// type Paged<T> = Vec<T>;
/// type Hook = Box<dyn Fn(u8)>;
/// "#;
/// let items = extract::extract_str(
///     Path::new("src/lib.rs"),
///     source,
///     Options::new(),
///     &mut Diagnostics::new(),
/// )?;
/// let [Item::Alias(owner), Item::Alias(paged), Item::Alias(hook)] = &items[..] else {
///     return Err("no aliases".into());
/// };
/// assert_eq!(owner.rust_name, "Owner");
/// assert_eq!(owner.schema, json!({ "$ref": "$User" }));
/// assert_eq!(paged.params, ["T"]);
/// assert_eq!(paged.schema, json!({ "type": "array", "items": { "$ref": "$T" } }));
/// assert_eq!(paged.location.line(), 3);
/// // Reported only where `Hook` is used.
/// assert_eq!(hook.undocumentable, ["dyn Fn(u8)"]);
/// # Ok::<(), Box<dyn std::error::Error>>(())
/// ```
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub struct AliasItem {
    /// The Rust identifier.
    pub rust_name: String,
    /// Type parameter names, referenced as `$T` in the schema.
    pub params: Vec<String>,
    /// The schema of the target type; references to other types are
    /// `{"$ref": "$Name"}`.
    pub schema: Value,
    /// The parts of the target that cannot be documented (function
    /// pointers, trait objects, …); they are reported where the alias is
    /// used.
    pub undocumentable: Vec<String>,
    /// Where the alias is declared.
    pub location: Location,
}

/// How [`extract`] reads annotations.
///
/// # Examples
///
/// ```
/// use oas_forge::diagnostics::Diagnostics;
/// use oas_forge::extract::{self, Item, Options};
/// use oas_forge::schema::TypeMapper;
/// use oas_forge::template::Variables;
/// use serde_json::json;
/// use std::path::Path;
///
/// let types = TypeMapper::new().with_type("UserId", json!({ "type": "string", "format": "uuid" }));
/// let variables = Variables::from([("EXAMPLE_NAME".to_string(), "Ada".to_string())]);
/// let options = Options::new().types(&types).variables(&variables);
///
/// let source = r#"
/// /// @openapi
/// pub struct User {
///     pub id: UserId,
///     /// @example {{EXAMPLE_NAME}}
///     pub name: String,
/// }
/// "#;
/// let items =
///     extract::extract_str(Path::new("src/lib.rs"), source, options, &mut Diagnostics::new())?;
/// let Some(Item::Schema(user)) = items.first() else {
///     return Err("no schema".into());
/// };
/// assert_eq!(
///     user.schema["properties"],
///     json!({
///         "id": { "type": "string", "format": "uuid" },
///         "name": { "type": "string", "examples": ["Ada"] },
///     })
/// );
/// # Ok::<(), Box<dyn std::error::Error>>(())
/// ```
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
        /// No variables, shared by every default.
        static NONE: Variables = Variables::new();
        Self {
            types: TypeMapper::builtin(),
            variables: &NONE,
        }
    }
}

impl<'a> Options<'a> {
    /// The built-in type table and no variables.
    ///
    /// # Examples
    ///
    /// Types outside the built-in table become smart references, resolved
    /// later against the documented schemas:
    ///
    /// ```
    /// use oas_forge::diagnostics::Diagnostics;
    /// use oas_forge::extract::{self, Item, Options};
    /// use serde_json::json;
    /// use std::path::Path;
    ///
    /// let source = "/// @openapi\npub struct User {\n    pub id: UserId,\n    pub age: u8,\n}\n";
    /// let items = extract::extract_str(
    ///     Path::new("src/lib.rs"),
    ///     source,
    ///     Options::new(),
    ///     &mut Diagnostics::new(),
    /// )?;
    /// let Some(Item::Schema(user)) = items.first() else {
    ///     return Err("no schema".into());
    /// };
    /// assert_eq!(
    ///     user.schema["properties"],
    ///     json!({
    ///         "id": { "$ref": "$UserId" },
    ///         "age": { "type": "integer", "format": "int32", "minimum": 0, "maximum": 255 },
    ///     })
    /// );
    /// # Ok::<(), Box<dyn std::error::Error>>(())
    /// ```
    pub fn new() -> Self {
        Self::default()
    }

    /// Sets the type mapping.
    ///
    /// # Examples
    ///
    /// ```
    /// use oas_forge::diagnostics::Diagnostics;
    /// use oas_forge::extract::{self, Item, Options};
    /// use oas_forge::schema::TypeMapper;
    /// use serde_json::json;
    /// use std::path::Path;
    ///
    /// let types = TypeMapper::new().with_type("UserId", json!({ "type": "string", "format": "uuid" }));
    /// let source = "/// @openapi\npub struct User {\n    pub id: UserId,\n}\n";
    /// let items = extract::extract_str(
    ///     Path::new("src/lib.rs"),
    ///     source,
    ///     Options::new().types(&types),
    ///     &mut Diagnostics::new(),
    /// )?;
    /// let Some(Item::Schema(user)) = items.first() else {
    ///     return Err("no schema".into());
    /// };
    /// assert_eq!(user.schema["properties"]["id"], json!({ "type": "string", "format": "uuid" }));
    /// # Ok::<(), Box<dyn std::error::Error>>(())
    /// ```
    pub fn types(mut self, types: &'a TypeMapper) -> Self {
        self.types = types;
        self
    }

    /// Sets the values of `{{NAME}}` placeholders.
    ///
    /// # Examples
    ///
    /// A value that is a single placeholder stays a string:
    ///
    /// ```
    /// use oas_forge::diagnostics::Diagnostics;
    /// use oas_forge::extract::{self, Item, Options};
    /// use oas_forge::template::Variables;
    /// use serde_json::json;
    /// use std::path::Path;
    ///
    /// let variables = Variables::from([("EXAMPLE_ID".to_string(), "42".to_string())]);
    /// let source = r#"
    /// /// @openapi
    /// pub struct User {
    ///     /// @example {{EXAMPLE_ID}}
    ///     pub id: u64,
    /// }
    /// "#;
    /// let items = extract::extract_str(
    ///     Path::new("src/lib.rs"),
    ///     source,
    ///     Options::new().variables(&variables),
    ///     &mut Diagnostics::new(),
    /// )?;
    /// let Some(Item::Schema(user)) = items.first() else {
    ///     return Err("no schema".into());
    /// };
    /// assert_eq!(user.schema["properties"]["id"]["examples"], json!(["42"]));
    /// # Ok::<(), Box<dyn std::error::Error>>(())
    /// ```
    pub fn variables(mut self, variables: &'a Variables) -> Self {
        self.variables = variables;
        self
    }
}

/// Parses Rust `source` and returns its annotations in source order.
///
/// # Errors
/// [`Error::Parse`] when the file is not valid Rust.
///
/// # Examples
///
/// ```
/// use oas_forge::Error;
/// use oas_forge::diagnostics::Diagnostics;
/// use oas_forge::extract::{self, Item, Options};
/// use oas_forge::source::{SourceFile, SourceKind};
///
/// let file = SourceFile::new("src/lib.rs", SourceKind::Rust, "/// @openapi\npub struct Ping;\n");
/// let items = extract::extract(&file, Options::new(), &mut Diagnostics::new())?;
/// assert!(matches!(&items[..], [Item::Schema(ping)] if ping.name == "Ping"));
///
/// let broken = SourceFile::new("src/lib.rs", SourceKind::Rust, "pub struct {");
/// let error = extract::extract(&broken, Options::new(), &mut Diagnostics::new());
/// assert!(matches!(error, Err(Error::Parse { .. })));
/// # Ok::<(), oas_forge::Error>(())
/// ```
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
///
/// # Examples
///
/// Problems with single annotations are findings, not errors:
///
/// ```
/// use oas_forge::diagnostics::{Code, Diagnostics};
/// use oas_forge::extract::{self, Item, Options};
/// use std::path::Path;
///
/// let source = r#"
/// static SLUG: LazyLock<Regex> = LazyLock::new(|| Regex::new("^[a-z0-9-]+$").expect("valid"));
///
/// /// @openapi
/// /// @openapi rename
/// pub struct Post {
///     #[validate(regex(path = *SLUG))]
///     pub slug: String,
/// }
/// "#;
/// let mut diagnostics = Diagnostics::new();
/// let items = extract::extract_str(
///     Path::new("src/posts.rs"),
///     source,
///     Options::new(),
///     &mut diagnostics,
/// )?;
/// assert!(matches!(&items[..], [Item::Pattern(_), Item::Schema(_)]));
/// let codes: Vec<Code> = diagnostics.iter().map(|d| d.code()).collect();
/// assert_eq!(codes, [Code::InvalidSchemaDirective]);
/// # Ok::<(), oas_forge::Error>(())
/// ```
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
        default_line_numbers: Vec::new(),
        inline: Vec::new(),
        diagnostics,
    };
    visitor.visit_file(&file);
    Ok(visitor.items)
}

/// Walks a parsed file and collects its annotations in source order.
struct Extractor<'a> {
    /// The file, for locations.
    path: &'a Path,
    /// How annotations are read.
    options: Options<'a>,
    /// The annotations found so far.
    items: Vec<Item>,
    /// Tags of the enclosing modules (`tags: [A, B]` in their doc comments),
    /// outermost first.
    tags: Vec<String>,
    /// `@route-defaults` blocks of the enclosing modules, outermost first;
    /// each starts with its `@route-defaults` header.
    defaults: Vec<String>,
    /// The source line of each entry of `defaults` (1-based).
    default_line_numbers: Vec<usize>,
    /// The directories of the inline modules around the current item: their
    /// names, or their `#[path]` attributes.
    inline: Vec<String>,
    /// Where findings go.
    diagnostics: &'a mut Diagnostics,
}

impl Extractor<'_> {
    /// The location of the 1-based `line` of the file.
    fn location(&self, line: usize) -> Location {
        Location::new(self.path, line)
    }

    /// Handles the doc comment of a function or method: a route or free blocks.
    fn function(&mut self, attrs: &[syn::Attribute], sig: &syn::Signature, line: usize) {
        let lines = doc_lines(attrs);
        // Most functions carry no directive: skip them before copying lines.
        if !lines.iter().any(|l| l.trimmed().starts_with('@')) {
            return;
        }
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
                default_line_numbers: self.default_line_numbers.clone(),
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

    /// Adds the free blocks of an item's doc comment (`@openapi…`,
    /// `@asyncapi`), with the tags of the enclosing modules.
    fn blocks(&mut self, lines: &[DocLine]) {
        let items = blocks::item_blocks(self.path, lines, &self.tags, self.diagnostics);
        self.items.extend(items);
    }

    /// Adds a `@route-defaults` block (`header` and the lines after it) to
    /// the defaults of the items that follow; a block without lines adds
    /// nothing.
    fn push_defaults(&mut self, block: &[DocLine]) {
        if let [header, rest @ ..] = block
            && !rest.is_empty()
        {
            // The header separates the modules: inner defaults replace the
            // tags and security of outer ones.
            self.defaults.push("@route-defaults".to_string());
            self.defaults.extend(rest.iter().map(|l| l.text.clone()));
            self.default_line_numbers.push(header.line);
            self.default_line_numbers
                .extend(rest.iter().map(|l| l.line));
        }
    }

    /// The files Rust looks for the module `name` in, declared here with the
    /// `#[path]` attribute `path`, if any.
    fn module_files(&self, name: &str, path: Option<&str>) -> Vec<PathBuf> {
        let parent = self.path.parent().unwrap_or(Path::new(""));
        let files = match path {
            // Outside inline modules relative to the declaring file.
            Some(path) if self.inline.is_empty() => vec![parent.join(path)],
            Some(path) => vec![self.module_directory().join(path)],
            None => {
                let directory = self.module_directory();
                vec![
                    directory.join(format!("{name}.rs")),
                    directory.join(name).join("mod.rs"),
                ]
            }
        };
        files.iter().map(|file| normalize_path(file)).collect()
    }

    /// The directory of the modules declared here: next to `mod.rs`,
    /// `lib.rs` and `main.rs`, in a directory of the file's name for other
    /// files, and below it one directory per inline module.
    fn module_directory(&self) -> PathBuf {
        let parent = self.path.parent().unwrap_or(Path::new(""));
        let mod_rs = self
            .path
            .file_name()
            .and_then(|name| name.to_str())
            .is_some_and(|name| matches!(name, "mod.rs" | "lib.rs" | "main.rs"));
        let mut directory = match self.path.file_stem() {
            Some(stem) if !mod_rs => parent.join(stem),
            _ => parent.to_path_buf(),
        };
        directory.extend(&self.inline);
        directory
    }
}

/// The value of a `#[path = "file"]` attribute.
fn path_attribute(attrs: &[syn::Attribute]) -> Option<String> {
    attrs.iter().find_map(|attr| match &attr.meta {
        syn::Meta::NameValue(meta) if meta.path.is_ident("path") => match &meta.value {
            syn::Expr::Lit(syn::ExprLit {
                lit: syn::Lit::Str(path),
                ..
            }) => Some(path.value()),
            _ => None,
        },
        _ => None,
    })
}

/// `path` without `.` components and with `..` resolved against the
/// component before it, where there is one; the file system is not read.
pub(crate) fn normalize_path(path: &Path) -> PathBuf {
    let mut normalized = PathBuf::new();
    for component in path.components() {
        match component {
            Component::CurDir => {}
            Component::ParentDir
                if matches!(
                    normalized.components().next_back(),
                    Some(Component::Normal(_))
                ) =>
            {
                normalized.pop();
            }
            other => normalized.push(other),
        }
    }
    normalized
}

impl<'ast> Visit<'ast> for Extractor<'_> {
    fn visit_file(&mut self, file: &'ast syn::File) {
        let mut items = blocks::file_blocks(self.path, &file.attrs, self.diagnostics);
        // `//! @route-defaults` applies to every route of the file,
        // including the file-level ones.
        self.push_defaults(blocks::file_route_defaults(&doc_lines(&file.attrs)));
        for item in &mut items {
            if let Item::Route(route) = item {
                route.defaults.clone_from(&self.defaults);
                route
                    .default_line_numbers
                    .clone_from(&self.default_line_numbers);
            }
        }
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
        self.push_defaults(blocks::route_defaults(&lines));
        self.blocks(&lines);
        let name = item.ident.unraw().to_string();
        let path = path_attribute(&item.attrs);
        if item.content.is_none() {
            self.items.push(Item::Module(ModuleItem {
                files: self.module_files(&name, path.as_deref()),
                name: name.clone(),
                defaults: self.defaults.clone(),
                default_line_numbers: self.default_line_numbers.clone(),
                location: self.location(line_of(item.mod_token.span)),
            }));
        }
        self.inline.push(path.unwrap_or(name));
        visit::visit_item_mod(self, item);
        self.inline.pop();
        self.tags.truncate(saved.0);
        self.defaults.truncate(saved.1);
        self.default_line_numbers.truncate(saved.1);
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
