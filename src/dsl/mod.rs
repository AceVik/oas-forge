//! The route DSL: `@route` and `@webhook` blocks compiled to OpenAPI
//! operations.
//!
//! ```text
//! /// List users
//! /// @route GET /users/{id: u64 "User id"}
//! /// @param query limit: u32 default=20 max=100 "Page size"
//! /// @return 200: $Page<User> "Users"
//! /// @problem 404
//! ```
//!
//! Every field of the OpenAPI Operation object can also be written as YAML
//! inside the block; `@openapi path` and `@openapi document` blocks target the
//! path item and the document. Explicit YAML wins over the DSL (with a note).

mod channel;
pub(crate) mod expand;
mod operation;
mod params;
mod prose;
mod responses;
mod tokens;

pub(crate) use channel::channel_messages;
pub use channel::{ChannelSource, CompiledChannel, compile_channel, is_channel};
pub(crate) use params::EXPAND_KEY;

use crate::diagnostics::{Code, Diagnostic, Diagnostics, Location, Severity};
use crate::docs::{closes_fence, yaml_fence};
use crate::merge::{Policy, merge};
use crate::reserved::{self, Keep};
use crate::schema::{MappedType, TypeExpr, TypeMapper, shadow};
use crate::scope::{self, Names};
use crate::template::lexical::{DEFAULTS_MARKER, EXTEND_KEY, charge_run, pasted_cost};
use crate::yaml;
use expand::identity;
use operation::Security;
use prose::{ImplicitYaml, Prose, Text};
use regex::Regex;
use serde_json::{Map, Value, json};
use std::collections::{BTreeMap, BTreeSet};
use std::sync::LazyLock;
use tokens::split_directive;

/// Operation fields that start an embedded YAML section in a route block.
/// `summary` and `description` are left out: such lines are usually prose.
const YAML_KEYS: &[&str] = &[
    "tags",
    "externalDocs",
    "operationId",
    "parameters",
    "requestBody",
    "responses",
    "callbacks",
    "deprecated",
    "security",
    "servers",
];

/// The finding for a `@scope` line without valid names.
pub(crate) const SCOPE_SYNTAX: &str =
    "`@scope` needs `name[, name...]`; a name may contain letters, digits, `.`, `-` and `_`";

/// Methods that are fixed fields of a path item (OpenAPI 3.2 adds `query`);
/// other methods go to `additionalOperations`.
pub(crate) const METHODS: &[&str] = &[
    "get", "put", "post", "delete", "options", "head", "patch", "trace", "query",
];

/// What a handler's signature says about its operation (signature
/// inference). Built by the extraction from framework extractors such as
/// axum's `Path`, `Query` and `Json`.
///
/// # Examples
///
/// What `async fn create_user(Json(user): Json<NewUser>) -> Json<User>`
/// says, filling the request body and the success response of its route:
///
/// ```
/// use oas_forge::diagnostics::{Diagnostics, Location};
/// use oas_forge::dsl::{self, RouteSource, Signature};
/// use oas_forge::schema::TypeExpr;
/// use serde_json::json;
///
/// let mut signature = Signature::default();
/// signature.body = TypeExpr::parse("NewUser").map(|ty| (ty, "application/json".to_string()));
/// signature.response = TypeExpr::parse("User");
///
/// let lines = ["@route POST /users".to_string()];
/// let location = Location::new("api.rs", 1);
/// let route = dsl::compile(
///     RouteSource::new(&lines, &location).signature(Some(&signature)),
///     &mut Diagnostics::new(),
/// )
/// .ok_or("the route is invalid")?;
/// let operation = &route.document["paths"]["/users"]["post"];
/// assert_eq!(
///     operation["requestBody"],
///     json!({
///         "content": { "application/json": { "schema": { "$ref": "$NewUser" } } },
///         "required": true,
///     })
/// );
/// assert_eq!(
///     operation["responses"]["200"]["content"]["application/json"]["schema"],
///     json!({ "$ref": "$User" })
/// );
/// # Ok::<(), Box<dyn std::error::Error>>(())
/// ```
#[derive(Debug, Clone, Default, PartialEq, Eq)]
#[non_exhaustive]
pub struct Signature {
    /// The types of the path parameters, in path order (`Path<(A, B)>`), or
    /// a single type or struct (`Path<T>`).
    pub path: Vec<TypeExpr>,
    /// The struct whose fields are query parameters (`Query<T>`).
    pub query: Option<TypeExpr>,
    /// Whether the query parameters may all be left out (`Option<Query<T>>`).
    pub optional_query: bool,
    /// The request body type and media type (`Json<T>`, `Form<T>`).
    pub body: Option<(TypeExpr, String)>,
    /// Whether the body may be left out (`Option<Json<T>>`).
    pub optional_body: bool,
    /// The type of the successful response (`Json<T>`, `Result<Json<T>, E>`).
    pub response: Option<TypeExpr>,
}

/// One route block: its lines and how to read them. Created with
/// [`RouteSource::new`], refined with the setters and compiled with
/// [`compile`].
///
/// # Examples
///
/// ```
/// # use oas_forge::diagnostics::{Diagnostics, Location};
/// # use oas_forge::dsl::{self, RouteSource};
/// let lines = vec!["@route GET /ping".to_string()];
/// let location = Location::new("api.rs", 1);
/// let route = dsl::compile(
///     RouteSource::new(&lines, &location).operation_id(Some("ping")),
///     &mut Diagnostics::new(),
/// )
/// .ok_or("the route is invalid")?;
/// assert_eq!(route.path, "/ping");
/// assert_eq!(route.document["paths"]["/ping"]["get"]["operationId"], "ping");
/// # Ok::<(), Box<dyn std::error::Error>>(())
/// ```
#[derive(Debug, Clone, Copy)]
#[non_exhaustive]
pub struct RouteSource<'a> {
    /// The doc lines (after `@insert`/`@extend` expansion).
    pub lines: &'a [String],
    /// Source line of each entry of `lines` (may be shorter after expansion).
    pub line_numbers: &'a [usize],
    /// The operationId to use unless the block sets one.
    pub operation_id: Option<&'a str>,
    /// Tags applied when neither the block nor its defaults have a `@tag`.
    pub default_tags: &'a [String],
    /// Where the block starts.
    pub location: &'a Location,
    /// How Rust types in the block map to schemas.
    pub types: &'a TypeMapper,
    /// `@route-defaults` blocks of the enclosing modules, outermost first.
    /// Each block starts with its `@route-defaults` header line, so that the
    /// tags and security of an inner module replace those of outer ones.
    pub defaults: &'a [String],
    /// Where each entry of `defaults` is (module defaults may be in other
    /// files than the route).
    pub default_locations: &'a [Location],
    /// The handler's signature, used to fill gaps of the block.
    pub signature: Option<&'a Signature>,
}

impl<'a> RouteSource<'a> {
    /// A block of `lines` starting at `location`, with the built-in type table.
    ///
    /// # Examples
    ///
    /// Without further settings the operationId is derived from method and
    /// path:
    ///
    /// ```
    /// use oas_forge::diagnostics::{Diagnostics, Location};
    /// use oas_forge::dsl::{self, RouteSource};
    ///
    /// let lines = ["@route GET /users/{id: u64}".to_string()];
    /// let location = Location::new("api.rs", 1);
    /// let route = dsl::compile(RouteSource::new(&lines, &location), &mut Diagnostics::new())
    ///     .ok_or("the route is invalid")?;
    /// assert_eq!(
    ///     route.document["paths"]["/users/{id}"]["get"]["operationId"],
    ///     "get_users_id"
    /// );
    /// # Ok::<(), Box<dyn std::error::Error>>(())
    /// ```
    pub fn new(lines: &'a [String], location: &'a Location) -> Self {
        Self {
            lines,
            line_numbers: &[],
            operation_id: None,
            default_tags: &[],
            location,
            types: TypeMapper::builtin(),
            defaults: &[],
            default_locations: &[],
            signature: None,
        }
    }

    /// Sets the source line of each entry of `lines`.
    ///
    /// # Examples
    ///
    /// Findings point at the line they are about instead of the block start:
    ///
    /// ```
    /// use oas_forge::diagnostics::{Code, Diagnostics, Location};
    /// use oas_forge::dsl::{self, RouteSource};
    ///
    /// let lines = ["List users", "@route GET /users", "@limit 10"].map(String::from);
    /// let location = Location::new("api.rs", 10);
    /// let mut diagnostics = Diagnostics::new();
    /// let route = dsl::compile(
    ///     RouteSource::new(&lines, &location).line_numbers(&[10, 11, 12]),
    ///     &mut diagnostics,
    /// );
    /// assert!(route.is_some());
    /// let finding = diagnostics.iter().next().ok_or("no finding")?;
    /// assert_eq!(finding.code(), Code::UnknownDirective);
    /// assert_eq!(finding.location(), Some(&Location::new("api.rs", 12)));
    /// # Ok::<(), Box<dyn std::error::Error>>(())
    /// ```
    pub fn line_numbers(mut self, line_numbers: &'a [usize]) -> Self {
        self.line_numbers = line_numbers;
        self
    }

    /// Sets the default operationId (derived from method and path when `None`).
    ///
    /// # Examples
    ///
    /// ```
    /// use oas_forge::diagnostics::{Diagnostics, Location};
    /// use oas_forge::dsl::{self, RouteSource};
    ///
    /// let lines = ["@route GET /users".to_string()];
    /// let location = Location::new("api.rs", 1);
    /// let source = RouteSource::new(&lines, &location);
    ///
    /// let named = dsl::compile(source.operation_id(Some("list_users")), &mut Diagnostics::new())
    ///     .ok_or("the route is invalid")?;
    /// assert_eq!(named.document["paths"]["/users"]["get"]["operationId"], "list_users");
    ///
    /// let derived = dsl::compile(source.operation_id(None), &mut Diagnostics::new())
    ///     .ok_or("the route is invalid")?;
    /// assert_eq!(derived.document["paths"]["/users"]["get"]["operationId"], "get_users");
    /// # Ok::<(), Box<dyn std::error::Error>>(())
    /// ```
    pub fn operation_id(mut self, operation_id: Option<&'a str>) -> Self {
        self.operation_id = operation_id;
        self
    }

    /// Sets the tags of routes without `@tag`.
    ///
    /// # Examples
    ///
    /// ```
    /// use oas_forge::diagnostics::{Diagnostics, Location};
    /// use oas_forge::dsl::{self, RouteSource};
    /// use serde_json::json;
    ///
    /// let tags = ["Users".to_string()];
    /// let location = Location::new("api.rs", 1);
    ///
    /// let lines = ["@route GET /users".to_string()];
    /// let route = dsl::compile(
    ///     RouteSource::new(&lines, &location).default_tags(&tags),
    ///     &mut Diagnostics::new(),
    /// )
    /// .ok_or("the route is invalid")?;
    /// assert_eq!(route.document["paths"]["/users"]["get"]["tags"], json!(["Users"]));
    ///
    /// // `@tag` replaces the default tags.
    /// let lines = ["@route GET /users", "@tag Admin"].map(String::from);
    /// let route = dsl::compile(
    ///     RouteSource::new(&lines, &location).default_tags(&tags),
    ///     &mut Diagnostics::new(),
    /// )
    /// .ok_or("the route is invalid")?;
    /// assert_eq!(route.document["paths"]["/users"]["get"]["tags"], json!(["Admin"]));
    /// # Ok::<(), Box<dyn std::error::Error>>(())
    /// ```
    pub fn default_tags(mut self, tags: &'a [String]) -> Self {
        self.default_tags = tags;
        self
    }

    /// Sets the type mapping.
    ///
    /// # Examples
    ///
    /// ```
    /// use oas_forge::diagnostics::{Diagnostics, Location};
    /// use oas_forge::dsl::{self, RouteSource};
    /// use oas_forge::schema::TypeMapper;
    /// use serde_json::json;
    ///
    /// let types = TypeMapper::new().with_type("UserId", json!({ "type": "string", "format": "uuid" }));
    /// let lines = ["@route GET /users/{id: UserId}".to_string()];
    /// let location = Location::new("api.rs", 1);
    /// let route = dsl::compile(
    ///     RouteSource::new(&lines, &location).types(&types),
    ///     &mut Diagnostics::new(),
    /// )
    /// .ok_or("the route is invalid")?;
    /// assert_eq!(
    ///     route.document["paths"]["/users/{id}"]["get"]["parameters"][0]["schema"],
    ///     json!({ "type": "string", "format": "uuid" })
    /// );
    /// # Ok::<(), Box<dyn std::error::Error>>(())
    /// ```
    pub fn types(mut self, types: &'a TypeMapper) -> Self {
        self.types = types;
        self
    }

    /// Sets the `@route-defaults` lines of the enclosing modules.
    ///
    /// # Examples
    ///
    /// ```
    /// use oas_forge::diagnostics::{Diagnostics, Location};
    /// use oas_forge::dsl::{self, RouteSource};
    /// use serde_json::json;
    ///
    /// let defaults = ["@route-defaults", "@prefix /api", "@tag Users", "@security bearer"]
    ///     .map(String::from);
    /// let lines = ["@route GET /users".to_string()];
    /// let location = Location::new("api.rs", 1);
    /// let route = dsl::compile(
    ///     RouteSource::new(&lines, &location).defaults(&defaults),
    ///     &mut Diagnostics::new(),
    /// )
    /// .ok_or("the route is invalid")?;
    /// assert_eq!(route.path, "/api/users");
    /// let operation = &route.document["paths"]["/api/users"]["get"];
    /// assert_eq!(operation["tags"], json!(["Users"]));
    /// assert_eq!(operation["security"], json!([{ "bearer": [] }]));
    /// # Ok::<(), Box<dyn std::error::Error>>(())
    /// ```
    pub fn defaults(mut self, defaults: &'a [String]) -> Self {
        self.defaults = defaults;
        self
    }

    /// Sets where each entry of the defaults is (see [`Self::defaults`]).
    /// Findings about the defaults point there instead of at the start of
    /// the block, and each of them is reported once into the same
    /// [`Diagnostics`], however many routes share the defaults.
    ///
    /// # Examples
    ///
    /// ```
    /// use oas_forge::diagnostics::{Code, Diagnostics, Location};
    /// use oas_forge::dsl::{self, RouteSource};
    ///
    /// // The module's docs at lines 3 and 4 of `mod.rs`, its routes elsewhere.
    /// let defaults = ["@route-defaults", "@return oops"].map(String::from);
    /// let locations = [Location::new("mod.rs", 3), Location::new("mod.rs", 4)];
    /// let mut diagnostics = Diagnostics::new();
    /// for (path, line) in [("/a", 7), ("/b", 10)] {
    ///     let lines = [format!("@route GET {path}")];
    ///     let location = Location::new("decks.rs", line);
    ///     let source = RouteSource::new(&lines, &location)
    ///         .defaults(&defaults)
    ///         .default_locations(&locations);
    ///     assert!(dsl::compile(source, &mut diagnostics).is_some());
    /// }
    /// assert_eq!(diagnostics.len(), 1);
    /// let finding = diagnostics.iter().next().ok_or("no finding")?;
    /// assert_eq!(finding.code(), Code::InvalidDirective);
    /// assert_eq!(finding.location(), Some(&Location::new("mod.rs", 4)));
    /// # Ok::<(), Box<dyn std::error::Error>>(())
    /// ```
    pub fn default_locations(mut self, locations: &'a [Location]) -> Self {
        self.default_locations = locations;
        self
    }

    /// Sets the handler's signature for inference.
    ///
    /// # Examples
    ///
    /// The path parameter of `async fn get_user(Path(id): Path<u64>)`, which
    /// the block leaves undeclared:
    ///
    /// ```
    /// use oas_forge::diagnostics::{Diagnostics, Location};
    /// use oas_forge::dsl::{self, RouteSource, Signature};
    /// use oas_forge::schema::TypeExpr;
    /// use serde_json::json;
    ///
    /// let mut signature = Signature::default();
    /// signature.path = vec![TypeExpr::parse("u64").ok_or("not a type")?];
    /// let lines = ["@route GET /users/{id}".to_string()];
    /// let location = Location::new("api.rs", 1);
    /// let route = dsl::compile(
    ///     RouteSource::new(&lines, &location).signature(Some(&signature)),
    ///     &mut Diagnostics::new(),
    /// )
    /// .ok_or("the route is invalid")?;
    /// assert_eq!(
    ///     route.document["paths"]["/users/{id}"]["get"]["parameters"],
    ///     json!([{
    ///         "name": "id",
    ///         "in": "path",
    ///         "required": true,
    ///         "schema": { "type": "integer", "format": "int64", "minimum": 0 },
    ///     }])
    /// );
    /// # Ok::<(), Box<dyn std::error::Error>>(())
    /// ```
    pub fn signature(mut self, signature: Option<&'a Signature>) -> Self {
        self.signature = signature;
        self
    }
}

/// A compiled route.
///
/// # Examples
///
/// A webhook is keyed by its name under `webhooks`:
///
/// ```
/// use oas_forge::diagnostics::{Diagnostics, Location};
/// use oas_forge::dsl::{self, RouteSource};
/// use serde_json::json;
///
/// let lines = ["@webhook newUser", "@body $User"].map(String::from);
/// let location = Location::new("api.rs", 1);
/// let route = dsl::compile(RouteSource::new(&lines, &location), &mut Diagnostics::new())
///     .ok_or("the route is invalid")?;
/// assert_eq!(route.path, "newUser");
/// assert_eq!(route.method, "post");
/// assert!(route.webhook);
/// assert_eq!(
///     route.document["webhooks"]["newUser"]["post"]["requestBody"]["content"],
///     json!({ "application/json": { "schema": { "$ref": "$User" } } })
/// );
/// # Ok::<(), Box<dyn std::error::Error>>(())
/// ```
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub struct CompiledRoute {
    /// The normalized path (`/users/{id}`), or the webhook name; of the
    /// first valid `@route` or `@webhook` line when the block has several.
    pub path: String,
    /// The method: lowercase for the fixed path item fields, as written
    /// (uppercase) for `additionalOperations`; of the first valid line.
    pub method: String,
    /// Whether the first valid line is a `@webhook`.
    pub webhook: bool,
    /// The scopes of `@scope` (or of the module defaults) the route belongs
    /// to; empty when it is shared by every scope.
    pub scopes: Vec<String>,
    /// The document fragment: `paths` (or `webhooks`) with the operation of
    /// every line, plus anything from `@openapi document` and the components
    /// the block needs.
    pub document: Value,
}

/// Where lines of an embedded YAML section go.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Scope {
    /// The operation: after `@openapi` or `@openapi operation`, or from a
    /// line that starts with an operation field (`responses:`, `x-…:`).
    Operation,
    /// The path item (`@openapi path`).
    PathItem,
    /// The whole document (`@openapi document`).
    Document,
}

/// What the lines of a route block (or of the module defaults) declare so
/// far, before [`Compiler::finish`] turns it into a document fragment.
#[derive(Default)]
struct Builder {
    /// The path item key of the method (see [`method_key`]); empty until a
    /// valid `@route` or `@webhook` line.
    method: String,
    /// The normalized path of `@route`, without the module prefix (added by
    /// [`Compiler::finish`]); empty for a webhook.
    path: String,
    /// The name of a `@webhook`; `None` for a `@route`.
    webhook: Option<String>,
    /// The `@prefix` paths of the module defaults, joined without trailing
    /// `/`; empty in a route block.
    prefix: String,
    /// Operation fields set by directives (`operationId`, `deprecated`,
    /// `externalDocs`, `servers`, `requestBody`).
    operation: Map<String, Value>,
    /// Parameters and struct expansion markers in declaration order; a later
    /// declaration of the same name and location replaces an earlier one.
    parameters: Parameters,
    /// The responses of `@return`, `@return-header` and `@problem`, by
    /// status code.
    responses: Map<String, Value>,
    /// The descriptions written in `@return` and `@problem` lines, by status
    /// code, in source order.
    explained: BTreeMap<String, Vec<String>>,
    /// The status codes of `@return` and `@problem` lines (which mean their
    /// content, or that there is none), with the line of the first.
    stated: BTreeMap<String, At>,
    /// The `@security` requirements (`{}` for `@security none`); `None`
    /// without a `@security` line.
    security: Option<Vec<Value>>,
    /// The `@tag` names without duplicates; `None` without a `@tag` line, so
    /// that the default tags apply.
    tags: Option<Names>,
    /// The summary and description.
    prose: Prose,
    /// Where the lines read so far leave the next one.
    structure: Structure,
    /// The lines of embedded YAML: their scope, where they are and their
    /// text.
    yaml: Vec<(Scope, At, String)>,
    /// The path parameters declared so far (inline, with `@param path` or
    /// inferred from the signature), checked against the placeholders of
    /// the path.
    declared: BTreeSet<String>,
    /// Document parts that directives add (`components.schemas` of
    /// `@problem`); `@openapi document` YAML is kept in `yaml`.
    document: Map<String, Value>,
    /// Whether `@no-infer` turns signature inference off.
    no_infer: bool,
    /// The number of `@route` and `@webhook` lines read, valid or not: a
    /// block needs one.
    route_lines: usize,
    /// The tags and security so far come from an enclosing module; the next
    /// `@tag` or `@security` of an inner module replaces them.
    outer_tags: bool,
    /// Like `outer_tags`, for the security requirements.
    outer_security: bool,
    /// Where the YAML of each module's defaults starts in `yaml`.
    module_yaml: Vec<usize>,
    /// The `@scope` names; `None` without a `@scope` line.
    scopes: Option<Names>,
    /// Like `outer_tags`, for the scopes.
    outer_scopes: bool,
}

/// The parameters (and struct expansion markers) of a block in declaration
/// order. A later declaration of the same name and location replaces the
/// earlier one in its place; the place is found through an index, as
/// scanning the list for each of 20 000 parameters took two minutes.
#[derive(Default)]
struct Parameters {
    /// The parameters in order.
    list: Vec<Value>,
    /// The entry in `list` of each parameter with a name, by [`identity`].
    index: BTreeMap<String, usize>,
}

impl Parameters {
    /// Adds a parameter; a later declaration of the same name and location
    /// replaces the earlier one. A marker (without `name`) is appended.
    fn add(&mut self, parameter: Value) {
        if parameter.get("name").is_none() {
            self.list.push(parameter);
            return;
        }
        match self.index.get(&identity(&parameter)) {
            Some(at) => {
                if let Some(existing) = self.list.get_mut(*at) {
                    *existing = parameter;
                }
            }
            None => {
                self.index.insert(identity(&parameter), self.list.len());
                self.list.push(parameter);
            }
        }
    }

    /// Whether a parameter with the same name and location exists (a marker
    /// never does).
    fn has(&self, parameter: &Value) -> bool {
        parameter.get("name").is_some() && self.index.contains_key(&identity(parameter))
    }

    /// Puts `front` before the parameters.
    fn prepend(&mut self, mut front: Vec<Value>) {
        front.append(&mut self.list);
        self.list = front;
        self.index.clear();
        for (at, parameter) in self.list.iter().enumerate() {
            if parameter.get("name").is_some() {
                self.index.entry(identity(parameter)).or_insert(at);
            }
        }
    }

    /// Whether there are no parameters.
    fn is_empty(&self) -> bool {
        self.list.is_empty()
    }

    /// The parameters in order.
    fn iter(&self) -> std::slice::Iter<'_, Value> {
        self.list.iter()
    }

    /// The parameters in order.
    fn into_vec(self) -> Vec<Value> {
        self.list
    }
}

/// The JSON text of `value` with the keys of its objects sorted: the same
/// text for equal values, whichever order their keys have, so that values
/// can be compared through a set.
fn canonical(value: &Value) -> String {
    /// Sorts the keys of every object in `value`.
    fn sort_keys(value: &mut Value) {
        match value {
            Value::Object(map) => {
                map.sort_keys();
                map.values_mut().for_each(sort_keys);
            }
            Value::Array(items) => items.iter_mut().for_each(sort_keys),
            _ => {}
        }
    }
    let mut sorted = value.clone();
    sort_keys(&mut sorted);
    sorted.to_string()
}

/// The line of a block (or of its module defaults) a finding is about.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum At {
    /// The whole block: its start.
    Block,
    /// An entry of [`RouteSource::lines`].
    Line(usize),
    /// An entry of [`RouteSource::defaults`].
    Default(usize),
}

/// Where the lines of a block read so far leave the next one: in an
/// embedded YAML section, or in prose (free text, a `@description` or an
/// `@internal` note). The compiler and
/// [`yaml_lines`] read lines with it, so the lexical stage quotes
/// placeholders on exactly the lines the compiler parses as YAML.
#[derive(Debug, Default, Clone)]
struct Structure {
    /// The embedded YAML section that is open; `None` in prose.
    scope: Option<Scope>,
    /// What plain lines continue. In a `@description` or an `@internal`
    /// note, lines such as `tags: …` are text, not YAML. Ends at the next
    /// directive.
    text: Text,
    /// Whether the line before was an `@openapi` directive: a YAML code
    /// fence after it opens a fenced YAML section.
    after_openapi: bool,
    /// The fence of the open fenced YAML section (rendered as code by
    /// rustdoc), which ends at the closing fence.
    fence: Option<String>,
    /// The YAML section a field-name line started, which a paragraph of
    /// prose after a blank line ends.
    implicit: ImplicitYaml,
}

/// What a line of a block is, as [`Structure::read`] sees it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Line<'l> {
    /// A blank line: part of the open YAML section, if any.
    Blank,
    /// A directive: the text after `@`.
    Directive(&'l str),
    /// An `@extend` marker outside YAML: it merges into the operation, but
    /// the lines after it are still prose.
    Marker,
    /// A line of embedded YAML for the scope.
    Yaml(Scope),
    /// The summary or a line of the description.
    Prose,
    /// A line of prose that ends an implicit YAML section after a blank
    /// line: a new paragraph.
    Paragraph,
    /// A fence of a fenced YAML section: neither YAML nor prose.
    Fence,
}

impl Structure {
    /// Reads one line and advances to the next: a directive ends a YAML
    /// section, a `@description` and an `@internal` note (`@openapi`,
    /// `@description` and `@internal` start them); a line that starts with an
    /// operation field (`responses:`, `x-…:`) starts embedded YAML, unless it
    /// continues a `@description` or an `@internal` note.
    fn read<'l>(&mut self, line: &'l str) -> Line<'l> {
        let trimmed = line.trim();
        if let Some(fence) = &self.fence {
            if closes_fence(trimmed, fence) {
                self.fence = None;
                self.scope = None;
                return Line::Fence;
            }
            return self.scope.map_or(Line::Fence, Line::Yaml);
        }
        let after_openapi = std::mem::take(&mut self.after_openapi);
        if let Some(fence) = yaml_fence(trimmed).filter(|_| after_openapi) {
            self.fence = Some(fence.to_string());
            return Line::Fence;
        }
        if trimmed.is_empty() {
            self.implicit.blank();
            return Line::Blank;
        }
        if let Some(directive) = trimmed.strip_prefix('@') {
            self.implicit.stop();
            let (name, rest) = split_directive(directive);
            self.after_openapi = name == "openapi";
            self.text = Text::after(name);
            // An unknown scope is reported and taken for the operation.
            self.scope =
                (name == "openapi").then(|| openapi_scope(&rest).unwrap_or(Scope::Operation));
            return Line::Directive(directive);
        }
        // A key is a word before `:`; prose such as `x-forwarded headers…`
        // is not one.
        let key = trimmed
            .split_once(':')
            .map(|(key, _)| key.trim())
            .filter(|key| !key.is_empty() && !key.contains(char::is_whitespace));
        if self.implicit.ends(line) {
            self.scope = None;
            return Line::Paragraph;
        }
        if self.scope.is_none() && key.is_some_and(|key| key.starts_with(EXTEND_KEY)) {
            return Line::Marker;
        }
        if self.scope.is_none()
            && self.text == Text::Free
            && key.is_some_and(|key| YAML_KEYS.contains(&key) || key.starts_with("x-"))
        {
            self.scope = Some(Scope::Operation);
            self.implicit.start(line);
        }
        match self.scope {
            Some(scope) => Line::Yaml(scope),
            None => Line::Prose,
        }
    }
}

/// Where the YAML after `@openapi rest` goes; `None` for an unknown scope.
fn openapi_scope(rest: &str) -> Option<Scope> {
    match rest {
        "" | "operation" => Some(Scope::Operation),
        "path" => Some(Scope::PathItem),
        "document" => Some(Scope::Document),
        _ => None,
    }
}

/// For each line of a route block (or of `@route-defaults` blocks), whether
/// the compiler reads it as embedded YAML. Summary, description and
/// directive lines are not YAML; [`lexical::expand_block`] quotes
/// whole-value placeholders only on YAML lines.
///
/// [`lexical::expand_block`]: crate::template::lexical::expand_block
///
/// # Examples
///
/// ```
/// use oas_forge::dsl;
///
/// let lines = [
///     "Since: 1.10",
///     "@route GET /status",
///     "x-since: 1.10",
///     "@description",
///     "tags: are prose here",
///     "@openapi path",
///     "summary: Status",
/// ]
/// .map(String::from);
/// assert_eq!(
///     dsl::yaml_lines(&lines),
///     [false, false, true, false, false, false, true]
/// );
/// ```
pub fn yaml_lines(lines: &[String]) -> Vec<bool> {
    let mut structure = Structure::default();
    lines
        .iter()
        .map(|line| match structure.read(line) {
            Line::Blank => structure.scope.is_some(),
            Line::Marker | Line::Yaml(_) => true,
            Line::Directive(_) | Line::Prose | Line::Paragraph | Line::Fence => false,
        })
        .collect()
}

/// An inline path parameter: `{id}`, `{id: u64}`, `{id "User id"}` or
/// `{id: u64 "User id"}`. Captures the name, the type and the description
/// (with `\"` and `\\` escapes).
static INLINE_PARAM_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r#"\{(\w+)(?::\s*([^"}]+))?(?:\s*"((?:[^"\\]|\\.)+)")?\}"#)
        .expect("constant pattern")
});

/// A placeholder of a normalized path (`{id}`); captures the name.
static PLACEHOLDER_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"\{(\w+)\}").expect("constant pattern"));

/// Whether `line` is a `@route` or `@webhook` directive (`@routes` and
/// `@route-defaults` are not).
fn is_header(line: &str) -> bool {
    let line = line.trim();
    ["@route", "@webhook"].iter().any(|header| {
        line.strip_prefix(header)
            .is_some_and(|rest| rest.is_empty() || rest.starts_with(char::is_whitespace))
    })
}

/// Whether `lines` contain a `@route` or `@webhook` directive.
///
/// # Examples
///
/// ```
/// use oas_forge::dsl;
///
/// assert!(dsl::is_route(&["List users".into(), "  @route GET /users".into()]));
/// assert!(dsl::is_route(&["@webhook newUser".into()]));
/// assert!(!dsl::is_route(&["@routes".into(), "@tag Users".into()]));
/// ```
pub fn is_route(lines: &[String]) -> bool {
    lines.iter().any(|l| is_header(l))
}

/// Compiles a route block. Returns `None` (with error diagnostics) when the
/// route is invalid.
///
/// # Examples
///
/// ```
/// use oas_forge::diagnostics::{Diagnostics, Location};
/// use oas_forge::dsl::{self, RouteSource};
/// use serde_json::json;
///
/// let lines = [
///     "Get a user",
///     r#"@route GET /users/{id: u64 "User id"}"#,
///     "@return 200: $User",
///     r#"@problem 404 "No such user""#,
/// ]
/// .map(String::from);
/// let location = Location::new("api.rs", 1);
/// let mut diagnostics = Diagnostics::new();
/// let route = dsl::compile(
///     RouteSource::new(&lines, &location).operation_id(Some("get_user")),
///     &mut diagnostics,
/// )
/// .ok_or("the route is invalid")?;
/// assert!(diagnostics.is_empty());
/// assert_eq!((route.path.as_str(), route.method.as_str()), ("/users/{id}", "get"));
///
/// let operation = &route.document["paths"]["/users/{id}"]["get"];
/// assert_eq!(operation["summary"], "Get a user");
/// assert_eq!(
///     operation["parameters"],
///     json!([{
///         "name": "id",
///         "in": "path",
///         "required": true,
///         "description": "User id",
///         "schema": { "type": "integer", "format": "int64", "minimum": 0 },
///     }])
/// );
/// // `$User` stays a smart reference until the generator resolves it.
/// assert_eq!(
///     operation["responses"]["200"]["content"]["application/json"]["schema"],
///     json!({ "$ref": "$User" })
/// );
/// // `@problem` adds the schema its response refers to.
/// assert_eq!(
///     operation["responses"]["404"]["content"]["application/problem+json"]["schema"],
///     json!({ "$ref": "#/components/schemas/ProblemDetails" })
/// );
/// assert!(route.document["components"]["schemas"]["ProblemDetails"].is_object());
///
/// // A path parameter without a declaration makes the route invalid.
/// let lines = ["@route GET /users/{id}".to_string()];
/// let mut diagnostics = Diagnostics::new();
/// assert!(dsl::compile(RouteSource::new(&lines, &location), &mut diagnostics).is_none());
/// assert!(diagnostics.has_errors());
/// # Ok::<(), Box<dyn std::error::Error>>(())
/// ```
pub fn compile(source: RouteSource<'_>, diagnostics: &mut Diagnostics) -> Option<CompiledRoute> {
    let mut reported = reported_defaults(source, diagnostics);
    compile_with(source, diagnostics, &mut reported)
}

/// The findings about the module defaults of `source` that `diagnostics`
/// has already (from the routes compiled before that share them), keyed as
/// [`Compiler::report`] keys them. A finding about a default line is at the
/// line's location, or at the block's when the line has none.
fn reported_defaults(source: RouteSource<'_>, diagnostics: &Diagnostics) -> BTreeSet<String> {
    if source.defaults.is_empty() {
        return BTreeSet::new();
    }
    let locations: BTreeSet<&Location> = source
        .default_locations
        .iter()
        .chain([source.location])
        .collect();
    diagnostics
        .iter()
        .filter(|finding| finding.location().is_some_and(|at| locations.contains(at)))
        .map(ToString::to_string)
        .collect()
}

/// [`compile`] with `reported`, the findings about the module defaults
/// reported so far (each once, however many routes share the defaults),
/// which the generator keeps for the run: finding them again among the
/// findings of a run took minutes for thousands of routes.
pub(crate) fn compile_with(
    source: RouteSource<'_>,
    diagnostics: &mut Diagnostics,
    reported: &mut BTreeSet<String>,
) -> Option<CompiledRoute> {
    let headers: Vec<usize> = source
        .lines
        .iter()
        .enumerate()
        .filter(|(_, line)| is_header(line))
        .map(|(index, _)| index)
        .collect();
    if headers.len() < 2 {
        return compile_route(source, None, diagnostics, reported);
    }
    // Several `@route` lines: the block is compiled for each of them, and
    // findings about the lines they share are reported once. Each
    // compilation after the first is a copy of the block and of its module
    // defaults, which counts for the run like pasted text: 500 lines with
    // 500 `@param` lines took 2 GB, and 400 under 4000 lines of defaults
    // 9.5 GB.
    let bytes = pasted_cost(
        source
            .lines
            .iter()
            .chain(source.defaults)
            .map(String::as_str),
    );
    let mut compiled: Option<CompiledRoute> = None;
    let mut once: BTreeSet<String> = BTreeSet::new();
    for (copy, header) in headers.into_iter().enumerate() {
        if copy > 0 && !charge_run(bytes) {
            let location = source.line_numbers.get(header).map_or_else(
                || source.location.clone(),
                |line| Location::new(source.location.file(), *line),
            );
            diagnostics.push(
                Diagnostic::new(
                    Code::ExpansionLimit,
                    "the copies of this block for its `@route` lines exceed what is left of the 32 MiB for the pasted text of the run; this line and the ones after it are left out",
                )
                .at(location),
            );
            break;
        }
        let mut findings = Diagnostics::new();
        let route = compile_route(source, Some(header), &mut findings, reported);
        for finding in findings.into_vec() {
            if once.insert(finding.to_string()) {
                diagnostics.push(finding);
            }
        }
        compiled = match (compiled, route) {
            (Some(mut first), Some(route)) => {
                merge(&mut first.document, route.document, Policy::Document);
                Some(first)
            }
            (first, route) => first.or(route),
        };
    }
    compiled
}

/// Compiles a block for one of its `@route` lines (`route`, the entry of
/// [`RouteSource::lines`]; `None` when the block has at most one).
/// `reported` holds the findings about the defaults reported so far.
fn compile_route(
    source: RouteSource<'_>,
    route: Option<usize>,
    diagnostics: &mut Diagnostics,
    reported: &mut BTreeSet<String>,
) -> Option<CompiledRoute> {
    let mut compiler = Compiler {
        source,
        diagnostics,
        route,
        reported,
    };
    // Module defaults are compiled like a block without `@route`.
    let mut defaults = Builder::default();
    let mut module = 0;
    for (index, line) in source.defaults.iter().enumerate() {
        if line.trim() == "@route-defaults" {
            module += 1;
        }
        // `@extend` markers of the defaults must not collide with those of
        // the route, nor with those of other modules (each module's block is
        // expanded on its own).
        let line = line.replace(
            EXTEND_KEY,
            &format!("{EXTEND_KEY}{DEFAULTS_MARKER}{module}-"),
        );
        compiler.line(&mut defaults, At::Default(index), &line, true);
    }
    let mut builder = Builder::default();
    for (index, line) in source.lines.iter().enumerate() {
        compiler.line(&mut builder, At::Line(index), line, false);
    }
    compiler.finish(builder, defaults)
}

/// Compiles one route block and reports its findings.
struct Compiler<'s, 'd> {
    /// The block and how to read it.
    source: RouteSource<'s>,
    /// Where findings go.
    diagnostics: &'d mut Diagnostics,
    /// In a block with several `@route` (or `@webhook`) lines, the one this
    /// compilation is for; the others are left alone.
    route: Option<usize>,
    /// The findings about the module defaults reported so far (their
    /// texts), by every route that shares them.
    reported: &'d mut BTreeSet<String>,
}

impl Compiler<'_, '_> {
    /// The source location of a line of the block or of its defaults; the
    /// start of the block for [`At::Block`] and for a line without line
    /// number or location.
    fn location(&self, at: At) -> Location {
        match at {
            At::Line(index) => self
                .source
                .line_numbers
                .get(index)
                .map(|line| Location::new(self.source.location.file(), *line)),
            At::Default(index) => self.source.default_locations.get(index).cloned(),
            At::Block => None,
        }
        .unwrap_or_else(|| self.source.location.clone())
    }

    /// Reports a finding with `code` at `at` (see [`Self::location`]).
    fn report(&mut self, code: Code, at: At, message: impl Into<String>) {
        let finding = Diagnostic::new(code, message).at(self.location(at));
        // Every route of a module compiles the module's defaults again: a
        // finding about them that is reported already is not repeated.
        if matches!(at, At::Default(_)) && !self.reported.insert(finding.to_string()) {
            return;
        }
        self.diagnostics.push(finding);
    }

    /// Reports a directive with an invalid argument ([`Code::InvalidDirective`]).
    fn invalid(&mut self, at: At, message: impl Into<String>) {
        self.report(Code::InvalidDirective, at, message);
    }

    /// Reads one line into `b`: a directive, a line of embedded YAML, the
    /// summary or a description line. `at` is the line's entry in the block
    /// or in the module defaults (then `defaults` is set).
    fn line(&mut self, b: &mut Builder, at: At, line: &str, defaults: bool) {
        match b.structure.read(line) {
            Line::Blank => match b.structure.scope {
                Some(scope) => b.yaml.push((scope, at, String::new())),
                None => b.prose.line(b.structure.text, ""),
            },
            Line::Directive(directive) => {
                self.directive(b, at, directive, prose::margin(line), defaults);
            }
            Line::Marker => b.yaml.push((Scope::Operation, at, line.to_string())),
            Line::Yaml(scope) => b.yaml.push((scope, at, line.to_string())),
            Line::Prose => b.prose.line(b.structure.text, line),
            Line::Paragraph => {
                b.prose.paragraph(b.structure.text);
                b.prose.line(b.structure.text, line);
            }
            Line::Fence => {}
        }
    }

    /// Applies one directive (`directive` is the text after `@`, `margin`
    /// the indentation of its line). `@route` and `@webhook` belong to the
    /// block, `@route-defaults` and `@prefix` to module defaults; elsewhere,
    /// like unknown names, they are reported as unknown directives.
    fn directive(
        &mut self,
        b: &mut Builder,
        at: At,
        directive: &str,
        margin: &str,
        defaults: bool,
    ) {
        let (name, arguments) = split_directive(directive);
        let rest: &str = &arguments;
        let types = self.source.types;
        match name {
            "route" | "webhook" if !defaults && self.route.is_some_and(|r| at != At::Line(r)) => {}
            "route" if !defaults => self.route(b, at, rest),
            "webhook" if !defaults => self.webhook(b, at, rest),
            // Each module's defaults start with their header: what an inner
            // module sets replaces what the outer modules set.
            "route-defaults" if defaults => {
                b.outer_tags = b.tags.is_some();
                b.outer_security = b.security.is_some();
                b.outer_scopes = b.scopes.is_some();
                b.module_yaml.push(b.yaml.len());
            }
            "prefix" if defaults => {
                let prefix = self.path_template(b, rest);
                b.prefix.push_str(prefix.trim_end_matches('/'));
            }
            "tag" => {
                if std::mem::take(&mut b.outer_tags) {
                    b.tags = None;
                }
                let tags = b.tags.get_or_insert_with(Names::default);
                for tag in rest.split(',').map(str::trim).filter(|t| !t.is_empty()) {
                    tags.add(tag);
                }
            }
            "scope" => match scope::names(rest) {
                Some(names) => {
                    if std::mem::take(&mut b.outer_scopes) {
                        b.scopes = None;
                    }
                    b.scopes.get_or_insert_with(Names::default).extend(names);
                }
                None => self.invalid(at, SCOPE_SYNTAX),
            },
            "summary" => b.prose.summary(rest),
            "description" => b.prose.description(margin, rest),
            // The plain lines after it are left out (`Structure::read`).
            "internal" => {}
            "deprecated" => {
                b.operation.insert("deprecated".into(), json!(true));
            }
            "operation-id" if !rest.is_empty() && self.route.is_some() => self.invalid(
                at,
                "`@operation-id` cannot name the operations of several `@route` lines; each gets its own and this one is ignored",
            ),
            "operation-id" if !rest.is_empty() => {
                b.operation.insert("operationId".into(), json!(rest));
            }
            "external-docs" => match operation::url_with_description(rest) {
                Some(docs) => {
                    b.operation.insert("externalDocs".into(), docs);
                }
                None => self.invalid(at, "`@external-docs` needs `URL [\"description\"]`"),
            },
            "server" => match operation::url_with_description(rest) {
                Some(server) => push(&mut b.operation, "servers", server),
                None => self.invalid(at, "`@server` needs `URL [\"description\"]`"),
            },
            "security" => {
                let requirement = match operation::security(rest) {
                    // `{}` allows anonymous access; alone it means no security.
                    Some(Security::None) => Map::new(),
                    Some(Security::Requirement(requirement)) => requirement,
                    None => {
                        self.invalid(
                            at,
                            "`@security` needs `scheme[(scopes)] [+ scheme...]` or `none`",
                        );
                        return;
                    }
                };
                if std::mem::take(&mut b.outer_security) {
                    b.security = None;
                }
                b.security
                    .get_or_insert_with(Vec::new)
                    .push(Value::Object(requirement));
            }
            "param" => {
                let (location, rest) = rest.split_once(char::is_whitespace).unwrap_or((rest, ""));
                self.parameter(b, at, location, rest);
            }
            "path-param" => self.parameter(b, at, "path", rest),
            "query-param" => self.parameter(b, at, "query", rest),
            "header-param" => self.parameter(b, at, "header", rest),
            "cookie-param" => self.parameter(b, at, "cookie", rest),
            "querystring-param" => self.parameter(b, at, "querystring", rest),
            "path-params" | "query-params" | "header-params" | "cookie-params" => {
                let location = name.trim_end_matches("-params");
                if rest.is_empty() {
                    self.invalid(
                        at,
                        format!("`@{name}` needs a struct, e.g. `@{name} $Filter`"),
                    );
                } else {
                    let here = self.location(at);
                    b.parameters.add(params::expansion(location, rest, &here));
                }
            }
            "body" => match responses::body(rest, types) {
                Ok(body) => {
                    let request = b
                        .operation
                        .entry("requestBody")
                        .or_insert_with(|| json!({ "content": {}, "required": true }));
                    if let Some(Value::Object(content)) = request.get_mut("content") {
                        content.extend(body.content);
                    }
                    if !body.required {
                        request["required"] = json!(false);
                    }
                    if let Some(description) = body.description {
                        request["description"] = json!(description);
                    }
                }
                Err(message) => self.invalid(at, message),
            },
            "return" => match responses::response(rest, types) {
                Ok(response) => add_response(b, response, at),
                Err(message) => self.invalid(at, message),
            },
            "return-header" => match responses::header(rest, types) {
                Some((code, name, header)) => {
                    let response = b.responses.entry(code.clone()).or_insert_with(
                        || json!({ "description": responses::reason_phrase(&code) }),
                    );
                    if let Some(response) = response.as_object_mut()
                        && let Value::Object(headers) =
                            response.entry("headers").or_insert_with(|| json!({}))
                    {
                        headers.insert(name, header);
                    }
                }
                None => self.invalid(
                    at,
                    "`@return-header` needs `CODE Name: Type [\"description\"]`",
                ),
            },
            "problem" => match responses::problem(rest) {
                Some(response) => {
                    add_response(b, response, at);
                    let mut schemas = Map::new();
                    schemas.insert(
                        responses::PROBLEM_SCHEMA.into(),
                        responses::problem_schema(),
                    );
                    let mut document = Value::Object(std::mem::take(&mut b.document));
                    merge(
                        &mut document,
                        json!({ "components": { "schemas": schemas } }),
                        Policy::Document,
                    );
                    if let Value::Object(map) = document {
                        b.document = map;
                    }
                }
                None => self.invalid(at, "`@problem` needs a status code"),
            },
            "no-infer" => b.no_infer = true,
            // The YAML section itself is opened by `Structure::read`.
            "openapi" if openapi_scope(rest).is_none() => self.invalid(
                at,
                format!("unknown `@openapi` scope `{rest}` (use operation, path or document)"),
            ),
            "openapi" => {}
            other => self.report(
                Code::UnknownDirective,
                at,
                format!("unknown directive `@{other}`"),
            ),
        }
    }

    /// Adds the parameter of `@param <location> rest` (and its short forms).
    /// An unknown location or a malformed declaration is reported and
    /// skipped; unknown and contradicting flags are reported and ignored.
    fn parameter(&mut self, b: &mut Builder, at: At, location: &str, rest: &str) {
        if !params::LOCATIONS.contains(&location) {
            self.invalid(
                at,
                format!("unknown parameter location `{location}` (use path, query, header, cookie or querystring)"),
            );
            return;
        }
        let Some(parameter) = params::parse(location, rest, self.source.types) else {
            self.invalid(
                at,
                format!("`@{location}-param` needs `name: [Type] [flags] [\"description\"]`"),
            );
            return;
        };
        for problem in &parameter.problems {
            self.invalid(at, problem.clone());
        }
        if location == "path" {
            b.declared.insert(parameter.name);
        }
        b.parameters.add(parameter.value);
    }

    /// Reads `@route METHOD /path`, declaring the path's inline parameters.
    fn route(&mut self, b: &mut Builder, at: At, rest: &str) {
        b.route_lines += 1;
        let Some((method, raw_path)) = rest.split_once(char::is_whitespace) else {
            self.report(Code::InvalidRoute, at, "`@route` needs a method and a path");
            return;
        };
        if !is_token(method) {
            self.report(
                Code::InvalidRoute,
                at,
                format!("`{method}` is not an HTTP method"),
            );
            return;
        }
        b.method = method_key(method);
        b.path = self.path_template(b, raw_path);
    }

    /// Normalizes a path (or `@prefix`) to start with `/` and declares its
    /// inline parameters (`{id: u64 "User id"}`).
    fn path_template(&mut self, b: &mut Builder, raw_path: &str) -> String {
        let raw_path = raw_path.trim();
        let mut path = String::new();
        if !raw_path.is_empty() && !raw_path.starts_with('/') {
            path.push('/');
        }
        let mut last = 0;
        for caps in INLINE_PARAM_RE.captures_iter(raw_path) {
            let whole = caps.get_match();
            let name = &caps[1];
            path.push_str(raw_path.get(last..whole.start()).unwrap_or_default());
            path.push('{');
            path.push_str(name);
            path.push('}');
            last = whole.end();

            let type_text = caps.get(2).map(|m| m.as_str().trim());
            let description = caps.get(3).map(|m| tokens::unescape(m.as_str()));
            if type_text.is_none() && description.is_none() {
                continue;
            }
            b.declared.insert(name.to_string());
            let param =
                params::inline_path(name, type_text, description.as_deref(), self.source.types);
            b.parameters.add(param);
        }
        path.push_str(raw_path.get(last..).unwrap_or_default());
        path
    }

    /// Reads `@webhook [METHOD] name`; the method defaults to `post`.
    fn webhook(&mut self, b: &mut Builder, at: At, rest: &str) {
        b.route_lines += 1;
        let tokens: Vec<&str> = rest.split_whitespace().collect();
        let (method, name) = match tokens.as_slice() {
            [name] => ("post", *name),
            [method, name] => (*method, *name),
            _ => {
                self.report(Code::InvalidRoute, at, "`@webhook` needs `[METHOD] name`");
                return;
            }
        };
        if !is_token(method) {
            self.report(
                Code::InvalidRoute,
                at,
                format!("`{method}` is not an HTTP method"),
            );
            return;
        }
        b.method = method_key(method);
        b.webhook = Some(name.to_string());
    }

    /// Completes the route with the module `defaults`, the signature and
    /// the embedded YAML, and wraps the operation in its document fragment.
    /// `None` when the route is invalid (the finding is reported).
    fn finish(&mut self, mut b: Builder, defaults: Builder) -> Option<CompiledRoute> {
        if b.method.is_empty() || (b.path.is_empty() && b.webhook.is_none()) {
            // An invalid `@route` or `@webhook` line has been reported.
            if b.route_lines == 0 {
                self.report(
                    Code::InvalidRoute,
                    At::Block,
                    "`@route` needs a method and a path",
                );
            }
            return None;
        }
        let webhook = b.webhook.is_some();
        if !webhook {
            // `/` under a prefix is the prefix itself.
            b.path = if b.path == "/" && !defaults.prefix.is_empty() {
                defaults.prefix.clone()
            } else {
                format!("{}{}", defaults.prefix, b.path)
            };
        }
        // Defaults: parameters first, the rest only where the route has none.
        // A webhook has no path, so path parameters do not apply to it.
        let front: Vec<Value> = defaults
            .parameters
            .into_vec()
            .into_iter()
            .filter(|parameter| !b.parameters.has(parameter) && !(webhook && is_path(parameter)))
            .collect();
        b.parameters.prepend(front);
        if !webhook {
            b.declared.extend(defaults.declared);
        }
        b.no_infer |= defaults.no_infer;
        if b.tags.is_none() {
            b.tags = defaults.tags;
        }
        if b.security.is_none() {
            b.security = defaults.security;
        }
        let scopes = b
            .scopes
            .take()
            .or(defaults.scopes)
            .map(Names::into_vec)
            .unwrap_or_default();
        self.default_responses(&mut b, defaults.responses, defaults.explained);
        describe_responses(&mut b.responses, &b.explained);
        for (key, value) in defaults.operation {
            b.operation.entry(key).or_insert(value);
        }
        let mut document = Value::Object(std::mem::take(&mut b.document));
        merge(
            &mut document,
            Value::Object(defaults.document),
            Policy::Document,
        );

        let signature = self.source.signature.filter(|_| !b.no_infer);
        let deferred = !webhook && self.infer_path(&mut b, signature);
        if !webhook && !self.check_path(&b, deferred) {
            return None;
        }

        let mut operation = Map::new();
        let (summary, description) = std::mem::take(&mut b.prose).finish();
        if let Some(summary) = summary {
            operation.insert("summary".into(), json!(summary));
        }
        if let Some(description) = description {
            operation.insert("description".into(), json!(description));
        }
        let name = b.webhook.clone().unwrap_or_else(|| b.path.clone());
        // With several routes, each gets the handler's name and its own.
        let derived = derived_operation_id(&b.method, &name);
        let operation_id = match self.source.operation_id {
            Some(function) if self.route.is_some() => format!("{function}_{derived}"),
            Some(function) => function.to_string(),
            None => derived,
        };
        operation.insert("operationId".into(), json!(operation_id));
        let tags = b
            .tags
            .take()
            .map_or_else(|| self.source.default_tags.to_vec(), Names::into_vec);
        if !tags.is_empty() {
            operation.insert("tags".into(), json!(tags));
        }
        if !b.parameters.is_empty() {
            operation.insert(
                "parameters".into(),
                Value::Array(std::mem::take(&mut b.parameters).into_vec()),
            );
        }
        if let Some(requirements) = b.security.take() {
            // `{}` alone (only `@security none`) means no security at all.
            let mut security: Vec<Value> = Vec::with_capacity(requirements.len());
            if requirements
                .iter()
                .any(|r| r.as_object().is_none_or(|r| !r.is_empty()))
            {
                // A repeated requirement is one, whichever order its schemes
                // have; comparing each with all before took a minute for
                // 40 000 lines.
                let mut seen: BTreeSet<String> = BTreeSet::new();
                for requirement in requirements {
                    if seen.insert(canonical(&requirement)) {
                        security.push(requirement);
                    }
                }
            }
            operation.insert("security".into(), Value::Array(security));
        }
        operation.extend(std::mem::take(&mut b.operation));
        operation.insert(
            "responses".into(),
            Value::Object(std::mem::take(&mut b.responses)),
        );
        let mut operation = Value::Object(operation);

        let mut path_item = Value::Object(Map::new());
        let yaml = std::mem::take(&mut b.yaml);
        let mut targets = Targets {
            operation: &mut operation,
            path_item: &mut path_item,
            document: &mut document,
            explained: &mut b.explained,
        };
        self.embedded_yaml(&yaml, false, &mut targets);
        // YAML of the module defaults fills what the route leaves open, the
        // innermost module first: each module is parsed on its own, so inner
        // values win over outer ones.
        let mut ends: Vec<usize> = defaults.module_yaml.clone();
        ends.push(defaults.yaml.len());
        let mut starts = vec![0];
        starts.extend(defaults.module_yaml.iter().copied());
        for (start, end) in starts.into_iter().zip(ends).rev() {
            if let Some(module) = defaults.yaml.get(start..end) {
                self.embedded_yaml(module, true, &mut targets);
            }
        }
        // Embedded YAML that is not a mapping is rejected, so the operation
        // is still an object.
        if let Value::Object(map) = &mut operation {
            if let Some(signature) = signature {
                self.infer_rest(map, signature);
            }
            // OpenAPI requires at least one response.
            if let Some(Value::Object(responses)) = map.get_mut("responses")
                && responses.is_empty()
            {
                responses.insert(
                    "default".into(),
                    json!({ "description": responses::reason_phrase("default") }),
                );
            }
            order_fields(map);
        }

        let mut item = match path_item {
            Value::Object(map) => map,
            _ => Map::new(),
        };
        if METHODS.contains(&b.method.as_str()) {
            item.insert(b.method.clone(), operation);
        } else {
            let mut additional = Map::new();
            additional.insert(b.method.clone(), operation);
            let entry = item
                .entry("additionalOperations")
                .or_insert_with(|| json!({}));
            merge(entry, Value::Object(additional), Policy::Document);
        }
        let mut entries = Map::new();
        entries.insert(name.clone(), Value::Object(item));
        let section = if webhook { "webhooks" } else { "paths" };
        let mut wrapper = Map::new();
        wrapper.insert(section.into(), Value::Object(entries));
        merge(&mut document, Value::Object(wrapper), Policy::Document);
        Some(CompiledRoute {
            path: name,
            method: b.method,
            webhook,
            scopes,
            document,
        })
    }

    /// Fills undeclared path parameters from the signature; returns whether a
    /// struct expansion still has to declare some of them.
    fn infer_path(&mut self, b: &mut Builder, signature: Option<&Signature>) -> bool {
        let pending = b.parameters.iter().any(is_path_expansion);
        let Some(signature) = signature else {
            return pending;
        };
        let placeholders: Vec<String> = PLACEHOLDER_RE
            .captures_iter(&b.path)
            .map(|c| c[1].to_string())
            .collect();
        let undeclared: Vec<String> = placeholders
            .iter()
            .filter(|p| !b.declared.contains(*p))
            .cloned()
            .collect();
        if signature.path.is_empty() || undeclared.is_empty() {
            return pending;
        }
        let types = self.source.types;
        match signature.path.as_slice() {
            // `Path<Params>` with a struct: expanded once its schema is known.
            [single] if is_custom(single, types) => {
                let mut marker =
                    params::expansion("path", &type_name(single, types), self.source.location);
                if let [only] = undeclared.as_slice()
                    && let Some(Value::Object(expand)) = marker.get_mut(EXPAND_KEY)
                {
                    expand.insert("name".into(), json!(only));
                }
                b.parameters.add(marker);
                true
            }
            positional if positional.len() == placeholders.len() => {
                for (name, ty) in placeholders.iter().zip(positional) {
                    if b.declared.insert(name.clone()) {
                        let param = params::inline_path(name, Some(&ty.to_string()), None, types);
                        b.parameters.add(param);
                    }
                }
                pending
            }
            positional => {
                self.report(
                    Code::SignatureMismatch,
                    At::Block,
                    format!(
                        "the handler extracts {} path parameter(s) but `{}` has {}",
                        positional.len(),
                        b.path,
                        placeholders.len()
                    ),
                );
                pending
            }
        }
    }

    /// Reports placeholders without a parameter and parameters without a
    /// placeholder. Placeholders a pending expansion may declare are checked
    /// after the expansion.
    fn check_path(&mut self, b: &Builder, deferred: bool) -> bool {
        let mut valid = true;
        let mut placeholders: BTreeSet<&str> = BTreeSet::new();
        for caps in PLACEHOLDER_RE.captures_iter(&b.path) {
            placeholders.insert(caps.get(1).map_or("", |name| name.as_str()));
            if !b.declared.contains(&caps[1]) && !deferred {
                self.report(
                    Code::UndeclaredPathParam,
                    At::Block,
                    format!(
                        "path parameter `{}` of `{}` has no definition (use `{{{}: Type}}` or `@param path`)",
                        &caps[1], b.path, &caps[1]
                    ),
                );
                valid = false;
            }
        }
        // A declared name is usually a placeholder; one the placeholder
        // pattern does not match may still be in the path as text.
        for declared in &b.declared {
            if !placeholders.contains(declared.as_str())
                && !b.path.contains(&format!("{{{declared}}}"))
            {
                self.report(
                    Code::UnusedPathParam,
                    At::Block,
                    format!("path parameter `{declared}` is not used in `{}`", b.path),
                );
                valid = false;
            }
        }
        valid
    }

    /// Query parameters, body and success response from the signature, where
    /// the block (including its YAML and module defaults) has none.
    fn infer_rest(&mut self, map: &mut Map<String, Value>, signature: &Signature) {
        let types = self.source.types;
        if let Some(query) = &signature.query
            && is_custom(query, types)
        {
            // Single query parameters written in the block win over expanded
            // ones of the same name; a struct expansion or a `querystring`
            // parameter describes the query itself.
            let described = map
                .get("parameters")
                .and_then(Value::as_array)
                .is_some_and(|ps| {
                    ps.iter().any(|p| {
                        p.get("in").and_then(Value::as_str) == Some("querystring")
                            || p.get(EXPAND_KEY)
                                .and_then(|m| m.get("in"))
                                .and_then(Value::as_str)
                                == Some("query")
                    })
                });
            if !described {
                let mut marker = params::expansion(
                    "query",
                    &type_name(query, self.source.types),
                    self.source.location,
                );
                if signature.optional_query
                    && let Some(Value::Object(expand)) = marker.get_mut(EXPAND_KEY)
                {
                    expand.insert("optional".into(), json!(true));
                }
                push(map, "parameters", marker);
            }
        }
        if let Some((ty, media)) = &signature.body
            && !map.contains_key("requestBody")
        {
            let required = types.map(ty).required && !signature.optional_body;
            let schema = shadow::typed(types, ty, MappedType::into_schema);
            let mut content = Map::new();
            content.insert(media.clone(), json!({ "schema": schema }));
            map.insert(
                "requestBody".into(),
                json!({ "content": content, "required": required }),
            );
        }
        if let Some(ty) = &signature.response {
            let responses = map.entry("responses").or_insert_with(|| json!({}));
            // `default` usually documents errors; only a `2XX` response
            // replaces the inferred one.
            let has_success = responses
                .as_object()
                .is_some_and(|r| r.keys().any(|code| code.starts_with('2')));
            if !has_success && let Value::Object(responses) = responses {
                let schema = shadow::typed(types, ty, MappedType::into_schema);
                let mut content = Map::new();
                content.insert(responses::DEFAULT_MEDIA.into(), json!({ "schema": schema }));
                // The success response comes first, before `default` and errors.
                let others = std::mem::take(responses);
                responses.insert(
                    "200".into(),
                    json!({
                        "description": responses::reason_phrase("200"),
                        "content": content,
                    }),
                );
                responses.extend(others);
            }
        }
    }

    /// Adds the responses of the module defaults to the block's. A status
    /// code the block has too keeps one response: the defaults add their
    /// media types and the headers the block does not declare (a header of
    /// the block replaces the module's of that name, like a parameter), and
    /// their descriptions come first; when both give one media type a
    /// different schema, the block's response wins and a note says so.
    fn default_responses(
        &mut self,
        b: &mut Builder,
        responses: Map<String, Value>,
        mut explained: BTreeMap<String, Vec<String>>,
    ) {
        for (code, mut response) in responses {
            let outer = explained.remove(&code).unwrap_or_default();
            if let Some(own) = b.responses.get_mut(&code) {
                drop_known_headers(&mut response, own);
                if contents_conflict(own.get("content"), response.get("content")) {
                    let location = self.location(b.stated.get(&code).copied().unwrap_or(At::Block));
                    self.diagnostics.push(
                        Diagnostic::new(
                            Code::MergeConflict,
                            format!(
                                "response `{code}` replaces the one of the module defaults, which has another schema for one of its media types"
                            ),
                        )
                        .with_severity(Severity::Note)
                        .at(location),
                    );
                    continue;
                }
                merge(own, response, Policy::Extend);
            } else {
                b.responses.insert(code.clone(), response);
            }
            let own = b.explained.remove(&code).unwrap_or_default();
            b.explained
                .insert(code, outer.into_iter().chain(own).collect());
        }
    }

    /// Removes the reserved keys of the embedded YAML `value` (see
    /// [`reserved`]) and reports each at the line of `lines` that writes
    /// it, or at the first line of the section.
    fn strip_reserved(&mut self, value: &mut Value, lines: &[(At, &str)]) {
        let text = lines
            .iter()
            .map(|(_, line)| *line)
            .collect::<Vec<&str>>()
            .join("\n");
        let key_lines = reserved::key_lines(&text);
        for removed in reserved::strip(value, Keep::Markers) {
            let at = key_lines
                .get(removed.key.as_str())
                .and_then(|i| lines.get(*i))
                .or_else(|| lines.first())
                .map_or(At::Block, |(at, _)| *at);
            self.report(Code::ReservedKey, at, removed.message());
        }
    }

    /// Merges the embedded YAML sections into the operation, the path item
    /// and the document. YAML of the module `defaults` only fills gaps.
    fn embedded_yaml(
        &mut self,
        yaml: &[(Scope, At, String)],
        defaults: bool,
        targets: &mut Targets<'_>,
    ) {
        let Targets {
            operation,
            path_item,
            document,
            explained,
        } = targets;
        for scope in [Scope::Operation, Scope::PathItem, Scope::Document] {
            let lines: Vec<(At, &str)> = yaml
                .iter()
                .filter(|(s, _, _)| *s == scope)
                .map(|(_, at, line)| (*at, line.as_str()))
                .collect();
            let text: Vec<&str> = lines.iter().map(|(_, line)| *line).collect();
            if text.iter().all(|l| l.trim().is_empty()) {
                continue;
            }
            match yaml::parse(&text.join("\n")) {
                Ok(Value::Null) => {}
                Ok(value) if !value.is_object() => self.report(
                    Code::InvalidRouteYaml,
                    lines.first().map_or(At::Block, |(at, _)| *at),
                    "embedded OpenAPI YAML must be a mapping and was ignored",
                ),
                Ok(mut value) if defaults => {
                    self.strip_reserved(&mut value, &lines);
                    match scope {
                        Scope::Operation | Scope::PathItem => {
                            let target = if scope == Scope::Operation {
                                &mut *operation
                            } else {
                                &mut *path_item
                            };
                            drop_known_parameters(&mut value, target);
                            merge(target, value, Policy::Extend);
                        }
                        Scope::Document => {
                            merge(document, value, Policy::Document);
                        }
                    }
                }
                Ok(mut value) => {
                    self.strip_reserved(&mut value, &lines);
                    if scope == Scope::Operation {
                        add_yaml_descriptions(&mut value, operation, explained);
                    }
                    let conflicts = match scope {
                        Scope::Operation => merge(operation, value, Policy::Operation),
                        Scope::PathItem => merge(path_item, value, Policy::Operation),
                        Scope::Document => merge(document, value, Policy::Document),
                    };
                    let text_lines: Vec<&str> = lines.iter().map(|(_, line)| *line).collect();
                    for pointer in conflicts {
                        let at = conflict_line(&text_lines, &pointer)
                            .or(Some(0))
                            .and_then(|i| lines.get(i))
                            .map_or(At::Block, |(at, _)| *at);
                        let location = self.location(at);
                        self.diagnostics.push(
                            Diagnostic::new(
                                Code::MergeConflict,
                                format!("embedded YAML overrides the DSL value at `{pointer}`"),
                            )
                            .with_severity(Severity::Note)
                            .at(location),
                        );
                    }
                }
                Err(e) => {
                    let at = lines
                        .get(e.line.saturating_sub(1))
                        .map_or(At::Block, |(at, _)| *at);
                    self.report(
                        Code::InvalidRouteYaml,
                        at,
                        format!(
                            "embedded OpenAPI YAML is invalid and was ignored: {}",
                            e.message
                        ),
                    );
                }
            }
        }
    }
}

/// The line of `lines` (YAML) that sets the value at the JSON `pointer`:
/// each segment is looked up as a key below the line of the previous one.
/// Array indexes cannot be matched to lines; the search stops there.
fn conflict_line(lines: &[&str], pointer: &str) -> Option<usize> {
    let indent = |line: &str| line.len() - line.trim_start().len();
    let mut found: Option<(usize, usize)> = None;
    for segment in pointer.split('/').skip(1) {
        let segment = segment.replace("~1", "/").replace("~0", "~");
        let start = found.map_or(0, |(line, _)| line + 1);
        let hit = lines
            .iter()
            .enumerate()
            .skip(start)
            .filter(|(_, line)| !line.trim().is_empty())
            .take_while(|(_, line)| found.is_none_or(|(_, parent)| indent(line) > parent))
            .find(|(_, line)| yaml_key(line) == Some(segment.as_str()));
        match hit {
            Some((line, text)) => found = Some((line, indent(text))),
            None => break,
        }
    }
    found.map(|(line, _)| line)
}

/// The key of a YAML line (`key: …`, `- key: …`, `'200': …`).
fn yaml_key(line: &str) -> Option<&str> {
    let mut rest = line.trim_start();
    while let Some(item) = rest.strip_prefix("- ") {
        rest = item.trim_start();
    }
    let (key, _) = rest.split_once(':')?;
    let key = key.trim();
    Some(
        key.strip_prefix('\'')
            .and_then(|k| k.strip_suffix('\''))
            .or_else(|| key.strip_prefix('"').and_then(|k| k.strip_suffix('"')))
            .unwrap_or(key),
    )
}

/// Operation fields in output order; other keys (`x-…`) follow.
const FIELD_ORDER: &[&str] = &[
    "summary",
    "description",
    "operationId",
    "tags",
    "parameters",
    "security",
    "externalDocs",
    "servers",
    "deprecated",
    "requestBody",
    "responses",
    "callbacks",
];

/// Puts the fields of an operation in [`FIELD_ORDER`], so inferred and
/// embedded fields do not depend on where they were added.
fn order_fields(map: &mut Map<String, Value>) {
    let mut ordered = Map::with_capacity(map.len());
    for key in FIELD_ORDER {
        if let Some(value) = map.remove(*key) {
            ordered.insert((*key).to_string(), value);
        }
    }
    ordered.extend(std::mem::take(map));
    *map = ordered;
}

/// The path item key of a method: standard methods in lowercase, others
/// (for `additionalOperations`) in uppercase.
fn method_key(method: &str) -> String {
    let lower = method.to_ascii_lowercase();
    if METHODS.contains(&lower.as_str()) {
        lower
    } else {
        method.to_ascii_uppercase()
    }
}

/// Whether `text` is an RFC 9110 token (a valid method name).
fn is_token(text: &str) -> bool {
    !text.is_empty()
        && text
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || "!#$%&'*+.^_`|~-".contains(c))
}

/// The name of the struct `ty` for a parameter expansion: the schema name
/// of the documented type it means in the module of the route (`$Filter`),
/// else `ty` as written.
fn type_name(ty: &TypeExpr, types: &TypeMapper) -> String {
    let schema = types.map(ty).schema;
    let name = schema
        .get("$ref")
        .and_then(Value::as_str)
        .and_then(|reference| reference.strip_prefix('$'));
    name.map_or_else(|| ty.to_string(), |name| format!("${name}"))
}

/// Whether a type is documented as its own schema (a struct), not a
/// built-in type.
fn is_custom(ty: &TypeExpr, types: &TypeMapper) -> bool {
    types.map(ty).schema.get("$ref").is_some()
}

/// Appends `value` to the list at `key` (a value that is not a list is
/// replaced).
fn push(map: &mut Map<String, Value>, key: &str, value: Value) {
    match map.entry(key).or_insert_with(|| json!([])) {
        Value::Array(items) => items.push(value),
        other => *other = json!([value]),
    }
}

/// Whether a parameter (or struct expansion) is in the path.
fn is_path(parameter: &Value) -> bool {
    parameter.get("in").and_then(Value::as_str) == Some("path") || is_path_expansion(parameter)
}

/// Whether a parameter is a struct expansion in the path (`Path<Params>`).
fn is_path_expansion(parameter: &Value) -> bool {
    parameter
        .get(EXPAND_KEY)
        .and_then(|m| m.get("in"))
        .and_then(Value::as_str)
        == Some("path")
}

/// Removes from `response`, a response of the module defaults, the headers
/// that `target`, the block's response of the same status, declares: the
/// block's header replaces the module's, like a parameter. Header names
/// are compared without regard to ASCII case, as HTTP reads them.
fn drop_known_headers(response: &mut Value, target: &Value) {
    let Some(Value::Object(headers)) = response.get_mut("headers") else {
        return;
    };
    let known: Vec<String> = target
        .get("headers")
        .and_then(Value::as_object)
        .into_iter()
        .flat_map(Map::keys)
        .map(|name| name.to_ascii_lowercase())
        .collect();
    headers.retain(|name, _| !known.contains(&name.to_ascii_lowercase()));
}

/// Removes the parameters of default YAML that the operation (or path
/// item) already declares (by name and location), so the route's own
/// declaration wins. A parameter without a name is never known.
fn drop_known_parameters(value: &mut Value, target: &Value) {
    let known: BTreeSet<String> = target
        .get("parameters")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter(|parameter| parameter.get("name").is_some())
        .map(identity)
        .collect();
    if let Some(Value::Array(parameters)) = value.get_mut("parameters") {
        parameters.retain(|parameter| {
            parameter.get("name").is_none() || !known.contains(&identity(parameter))
        });
    }
}

/// What embedded YAML is merged into, and the descriptions of the DSL's
/// responses (see [`add_yaml_descriptions`]).
struct Targets<'t> {
    /// The Operation Object.
    operation: &'t mut Value,
    /// The Path Item Object.
    path_item: &'t mut Value,
    /// The document fragment.
    document: &'t mut Value,
    /// The explicit descriptions of the responses, by status code.
    explained: &'t mut BTreeMap<String, Vec<String>>,
}

/// Adds the response of a `@return` or `@problem` line (`at`): one
/// response per status code, repeated codes add their media types and their
/// descriptions.
fn add_response(b: &mut Builder, response: responses::Response, at: At) {
    let responses::Response {
        code,
        value,
        description,
    } = response;
    if let Some(description) = description {
        b.explained
            .entry(code.clone())
            .or_default()
            .push(description);
    }
    b.stated.entry(code.clone()).or_insert(at);
    match b.responses.get_mut(&code) {
        Some(existing) => {
            merge(existing, value, Policy::Override);
        }
        None => {
            b.responses.insert(code, value);
        }
    }
}

/// Whether two `content` maps of responses give one media type different
/// contents.
fn contents_conflict(one: Option<&Value>, other: Option<&Value>) -> bool {
    let (Some(Value::Object(one)), Some(Value::Object(other))) = (one, other) else {
        return false;
    };
    one.iter()
        .any(|(media, entry)| other.get(media).is_some_and(|o| o != entry))
}

/// Sets the description of each response from the descriptions written for
/// it (see [`combined_description`]); without any, it keeps the reason
/// phrase.
fn describe_responses(
    responses: &mut Map<String, Value>,
    explained: &BTreeMap<String, Vec<String>>,
) {
    for (code, descriptions) in explained {
        if let (Some(Value::Object(response)), Some(text)) =
            (responses.get_mut(code), combined_description(descriptions))
        {
            response.insert("description".into(), json!(text));
        }
    }
}

/// The description of a response written in several places: one as it is,
/// several (without repeats, in source order) as a Markdown list.
fn combined_description(descriptions: &[String]) -> Option<String> {
    let mut unique: Vec<&str> = Vec::new();
    for description in descriptions {
        if !unique.contains(&description.as_str()) {
            unique.push(description);
        }
    }
    match unique.as_slice() {
        [] => None,
        [one] => Some((*one).to_string()),
        several => Some(
            several
                .iter()
                .map(|description| format!("- {description}"))
                .collect::<Vec<_>>()
                .join("\n"),
        ),
    }
}

/// Embedded YAML that describes a response of the DSL adds its description
/// to the DSL's instead of replacing it, unless it gives one of the DSL's
/// media types another content: then it is merged as usual (it wins, with a
/// note).
fn add_yaml_descriptions(
    value: &mut Value,
    operation: &mut Value,
    explained: &mut BTreeMap<String, Vec<String>>,
) {
    let (Some(Value::Object(yaml)), Some(Value::Object(own))) =
        (value.get_mut("responses"), operation.get_mut("responses"))
    else {
        return;
    };
    for (code, response) in yaml.iter_mut() {
        let (Some(Value::Object(response)), Some(Value::Object(dsl))) =
            (Some(response), own.get_mut(code))
        else {
            continue;
        };
        let other_content = contents_conflict(response.get("content"), dsl.get("content"));
        let Some(Value::String(description)) = response.get("description").cloned() else {
            continue;
        };
        if other_content {
            continue;
        }
        response.remove("description");
        let descriptions = explained.entry(code.clone()).or_default();
        descriptions.push(description);
        if let Some(text) = combined_description(descriptions) {
            dsl.insert("description".into(), json!(text));
        }
    }
}

/// A stable operationId for routes without a function (`get_legacy_proxy`).
fn derived_operation_id(method: &str, path: &str) -> String {
    let mut id = method.to_ascii_lowercase();
    for part in path.split(|c: char| !c.is_alphanumeric()) {
        if !part.is_empty() {
            id.push('_');
            id.push_str(&part.to_lowercase());
        }
    }
    id
}
