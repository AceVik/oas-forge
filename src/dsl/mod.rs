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
mod responses;
mod tokens;

pub use channel::{ChannelSource, CompiledChannel, compile_channel, is_channel};
pub(crate) use params::EXPAND_KEY;

use crate::diagnostics::{Code, Diagnostic, Diagnostics, Location, Severity};
use crate::merge::{Policy, merge};
use crate::schema::{MappedType, TypeExpr, TypeMapper, shadow};
use crate::template::lexical::{DEFAULTS_MARKER, EXTEND_KEY};
use crate::yaml;
use operation::Security;
use regex::Regex;
use serde_json::{Map, Value, json};
use std::collections::BTreeSet;
use std::sync::LazyLock;

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
    /// The normalized path (`/users/{id}`), or the webhook name.
    pub path: String,
    /// The method: lowercase for the fixed path item fields, as written
    /// (uppercase) for `additionalOperations`.
    pub method: String,
    /// Whether the block is a `@webhook`.
    pub webhook: bool,
    /// The document fragment: `paths` (or `webhooks`), plus anything from
    /// `@openapi document` and the components the block needs.
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
    parameters: Vec<Value>,
    /// The responses of `@return`, `@return-header` and `@problem`, by
    /// status code.
    responses: Map<String, Value>,
    /// The `@security` requirements (`{}` for `@security none`); `None`
    /// without a `@security` line.
    security: Option<Vec<Value>>,
    /// The `@tag` names without duplicates; `None` without a `@tag` line, so
    /// that the default tags apply.
    tags: Option<Vec<String>>,
    /// The summary: `@summary`, or the first prose line.
    summary: Option<String>,
    /// The description lines (`@description` and the prose after the
    /// summary), blank lines included.
    description: Vec<String>,
    /// Whether prose lines continue a `@description`; then lines such as
    /// `tags: …` are text, not YAML. Ends at the next directive.
    in_description: bool,
    /// The lines of embedded YAML: their scope, their index in
    /// [`RouteSource::lines`] (`None` for module defaults) and their text.
    yaml: Vec<(Scope, Option<usize>, String)>,
    /// The embedded YAML section that is open; `None` in prose.
    scope: Option<Scope>,
    /// The path parameters declared so far (inline, with `@param path` or
    /// inferred from the signature), checked against the placeholders of
    /// the path.
    declared: BTreeSet<String>,
    /// Document parts that directives add (`components.schemas` of
    /// `@problem`); `@openapi document` YAML is kept in `yaml`.
    document: Map<String, Value>,
    /// Whether `@no-infer` turns signature inference off.
    no_infer: bool,
    /// The number of `@route` and `@webhook` lines, valid or not: a block
    /// needs exactly one.
    route_lines: usize,
    /// The tags and security so far come from an enclosing module; the next
    /// `@tag` or `@security` of an inner module replaces them.
    outer_tags: bool,
    /// Like `outer_tags`, for the security requirements.
    outer_security: bool,
    /// Where the YAML of each module's defaults starts in `yaml`.
    module_yaml: Vec<usize>,
}

/// An inline path parameter: `{id}`, `{id: u64}`, `{id "User id"}` or
/// `{id: u64 "User id"}`. Captures the name, the type and the description.
static INLINE_PARAM_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r#"\{(\w+)(?::\s*([^"}]+))?(?:\s*"([^"]+)")?\}"#).expect("constant pattern")
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
    let mut compiler = Compiler {
        source,
        diagnostics,
    };
    // Module defaults are compiled like a block without `@route`.
    let mut defaults = Builder::default();
    let mut module = 0;
    for line in source.defaults {
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
        compiler.line(&mut defaults, None, &line, true);
    }
    let mut builder = Builder::default();
    for (index, line) in source.lines.iter().enumerate() {
        compiler.line(&mut builder, Some(index), line, false);
    }
    compiler.finish(builder, defaults)
}

/// Compiles one route block and reports its findings.
struct Compiler<'s, 'd> {
    /// The block and how to read it.
    source: RouteSource<'s>,
    /// Where findings go.
    diagnostics: &'d mut Diagnostics,
}

impl Compiler<'_, '_> {
    /// The source location of entry `index` of the block's lines; the start
    /// of the block when `index` is `None` (module defaults, findings about
    /// the whole block) or has no line number.
    fn location(&self, index: Option<usize>) -> Location {
        match index.and_then(|i| self.source.line_numbers.get(i)) {
            Some(line) => Location::new(self.source.location.file(), *line),
            None => self.source.location.clone(),
        }
    }

    /// Reports a finding with `code` at entry `index` (see [`Self::location`]).
    fn report(&mut self, code: Code, index: Option<usize>, message: impl Into<String>) {
        let location = self.location(index);
        self.diagnostics
            .push(Diagnostic::new(code, message).at(location));
    }

    /// Reports a directive with an invalid argument ([`Code::InvalidDirective`]).
    fn invalid(&mut self, index: Option<usize>, message: impl Into<String>) {
        self.report(Code::InvalidDirective, index, message);
    }

    /// Reads one line into `b`: a directive, a line of embedded YAML, the
    /// summary or a description line. `index` is the line's entry in the
    /// block (`None` for module defaults, then `defaults` is set).
    fn line(&mut self, b: &mut Builder, index: Option<usize>, line: &str, defaults: bool) {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            match b.scope {
                Some(scope) => b.yaml.push((scope, index, String::new())),
                None if b.in_description || b.summary.is_some() => {
                    b.description.push(String::new());
                }
                None => {}
            }
            return;
        }
        if let Some(directive) = trimmed.strip_prefix('@') {
            b.scope = None;
            b.in_description = false;
            self.directive(b, index, directive, defaults);
            return;
        }
        // A key is a word before `:`; prose such as `x-forwarded headers…`
        // is not one.
        let key = trimmed
            .split_once(':')
            .map(|(key, _)| key.trim())
            .filter(|key| !key.is_empty() && !key.contains(char::is_whitespace));
        if b.scope.is_none() && key.is_some_and(|key| key.starts_with(EXTEND_KEY)) {
            // An `@extend` line merges into the operation but does not start
            // a YAML section: the lines after it are still prose.
            b.yaml.push((Scope::Operation, index, line.to_string()));
            return;
        }
        if b.scope.is_none()
            && !b.in_description
            && key.is_some_and(|key| YAML_KEYS.contains(&key) || key.starts_with("x-"))
        {
            b.scope = Some(Scope::Operation);
        }
        match b.scope {
            Some(scope) => b.yaml.push((scope, index, line.to_string())),
            None if b.in_description || b.summary.is_some() => {
                b.description.push(line.to_string());
            }
            None => b.summary = Some(trimmed.to_string()),
        }
    }

    /// Applies one directive (`directive` is the text after `@`). `@route`
    /// and `@webhook` belong to the block, `@route-defaults` and `@prefix` to
    /// module defaults; elsewhere, like unknown names, they are reported as
    /// unknown directives.
    fn directive(
        &mut self,
        b: &mut Builder,
        index: Option<usize>,
        directive: &str,
        defaults: bool,
    ) {
        let (name, rest) = directive
            .split_once(char::is_whitespace)
            .map_or((directive, ""), |(n, r)| (n, r.trim()));
        let types = self.source.types;
        match name {
            "route" if !defaults => self.route(b, index, rest),
            "webhook" if !defaults => self.webhook(b, index, rest),
            // Each module's defaults start with their header: what an inner
            // module sets replaces what the outer modules set.
            "route-defaults" if defaults => {
                b.outer_tags = b.tags.is_some();
                b.outer_security = b.security.is_some();
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
                let tags = b.tags.get_or_insert_with(Vec::new);
                for tag in rest.split(',').map(str::trim).filter(|t| !t.is_empty()) {
                    if !tags.iter().any(|t| t == tag) {
                        tags.push(tag.to_string());
                    }
                }
            }
            "summary" => b.summary = Some(rest.to_string()),
            "description" => {
                b.in_description = true;
                if !rest.is_empty() {
                    b.description.push(rest.to_string());
                }
            }
            "deprecated" => {
                b.operation.insert("deprecated".into(), json!(true));
            }
            "operation-id" if !rest.is_empty() => {
                b.operation.insert("operationId".into(), json!(rest));
            }
            "external-docs" => match operation::url_with_description(rest) {
                Some(docs) => {
                    b.operation.insert("externalDocs".into(), docs);
                }
                None => self.invalid(index, "`@external-docs` needs `URL [\"description\"]`"),
            },
            "server" => match operation::url_with_description(rest) {
                Some(server) => push(&mut b.operation, "servers", server),
                None => self.invalid(index, "`@server` needs `URL [\"description\"]`"),
            },
            "security" => {
                let requirement = match operation::security(rest) {
                    // `{}` allows anonymous access; alone it means no security.
                    Some(Security::None) => Map::new(),
                    Some(Security::Requirement(requirement)) => requirement,
                    None => {
                        self.invalid(
                            index,
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
                self.parameter(b, index, location, rest);
            }
            "path-param" => self.parameter(b, index, "path", rest),
            "query-param" => self.parameter(b, index, "query", rest),
            "header-param" => self.parameter(b, index, "header", rest),
            "cookie-param" => self.parameter(b, index, "cookie", rest),
            "querystring-param" => self.parameter(b, index, "querystring", rest),
            "path-params" | "query-params" | "header-params" | "cookie-params" => {
                let location = name.trim_end_matches("-params");
                if rest.is_empty() {
                    self.invalid(
                        index,
                        format!("`@{name}` needs a struct, e.g. `@{name} $Filter`"),
                    );
                } else {
                    let at = self.location(index);
                    b.parameters.push(params::expansion(location, rest, &at));
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
                Err(message) => self.invalid(index, message),
            },
            "return" => match responses::response(rest, types) {
                Ok(response) => add_response(&mut b.responses, response.code, response.value),
                Err(message) => self.invalid(index, message),
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
                    index,
                    "`@return-header` needs `CODE Name: Type [\"description\"]`",
                ),
            },
            "problem" => match responses::problem(rest) {
                Some(response) => {
                    add_response(&mut b.responses, response.code, response.value);
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
                None => self.invalid(index, "`@problem` needs a status code"),
            },
            "no-infer" => b.no_infer = true,
            "openapi" => {
                b.scope = Some(match rest {
                    "" | "operation" => Scope::Operation,
                    "path" => Scope::PathItem,
                    "document" => Scope::Document,
                    other => {
                        self.invalid(
                            index,
                            format!("unknown `@openapi` scope `{other}` (use operation, path or document)"),
                        );
                        Scope::Operation
                    }
                });
            }
            other => self.report(
                Code::UnknownDirective,
                index,
                format!("unknown directive `@{other}`"),
            ),
        }
    }

    /// Adds the parameter of `@param <location> rest` (and its short forms).
    /// An unknown location or a malformed declaration is reported and
    /// skipped; unknown flags are reported and ignored.
    fn parameter(&mut self, b: &mut Builder, index: Option<usize>, location: &str, rest: &str) {
        if !params::LOCATIONS.contains(&location) {
            self.invalid(
                index,
                format!("unknown parameter location `{location}` (use path, query, header, cookie or querystring)"),
            );
            return;
        }
        let Some(parameter) = params::parse(location, rest, self.source.types) else {
            self.invalid(
                index,
                format!("`@{location}-param` needs `name: [Type] [flags] [\"description\"]`"),
            );
            return;
        };
        for token in &parameter.ignored {
            self.invalid(
                index,
                format!("`{token}` is not a parameter flag and was ignored"),
            );
        }
        if location == "path" {
            b.declared.insert(parameter.name);
        }
        add_parameter(&mut b.parameters, parameter.value);
    }

    /// Reads `@route METHOD /path`, declaring the path's inline parameters.
    fn route(&mut self, b: &mut Builder, index: Option<usize>, rest: &str) {
        b.route_lines += 1;
        let Some((method, raw_path)) = rest.split_once(char::is_whitespace) else {
            self.report(
                Code::InvalidRoute,
                index,
                "`@route` needs a method and a path",
            );
            return;
        };
        if !is_token(method) {
            self.report(
                Code::InvalidRoute,
                index,
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
            let description = caps.get(3).map(|m| m.as_str());
            if type_text.is_none() && description.is_none() {
                continue;
            }
            b.declared.insert(name.to_string());
            let param = params::inline_path(name, type_text, description, self.source.types);
            add_parameter(&mut b.parameters, param);
        }
        path.push_str(raw_path.get(last..).unwrap_or_default());
        path
    }

    /// Reads `@webhook [METHOD] name`; the method defaults to `post`.
    fn webhook(&mut self, b: &mut Builder, index: Option<usize>, rest: &str) {
        b.route_lines += 1;
        let tokens: Vec<&str> = rest.split_whitespace().collect();
        let (method, name) = match tokens.as_slice() {
            [name] => ("post", *name),
            [method, name] => (*method, *name),
            _ => {
                self.report(
                    Code::InvalidRoute,
                    index,
                    "`@webhook` needs `[METHOD] name`",
                );
                return;
            }
        };
        if !is_token(method) {
            self.report(
                Code::InvalidRoute,
                index,
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
                    None,
                    "`@route` needs a method and a path",
                );
            }
            return None;
        }
        if b.route_lines > 1 {
            self.report(
                Code::InvalidRoute,
                None,
                "a block may contain only one `@route` or `@webhook`; the last one is used",
            );
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
        for parameter in defaults.parameters.into_iter().rev() {
            if !has_parameter(&b.parameters, &parameter) && !(webhook && is_path(&parameter)) {
                b.parameters.insert(0, parameter);
            }
        }
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
        for (code, response) in defaults.responses {
            b.responses.entry(code).or_insert(response);
        }
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
        if let Some(summary) = b.summary.take() {
            operation.insert("summary".into(), json!(summary));
        }
        if let Some(description) = description_text(&b.description) {
            operation.insert("description".into(), json!(description));
        }
        let name = b.webhook.clone().unwrap_or_else(|| b.path.clone());
        let operation_id = self
            .source
            .operation_id
            .map(str::to_string)
            .unwrap_or_else(|| derived_operation_id(&b.method, &name));
        operation.insert("operationId".into(), json!(operation_id));
        let tags = b
            .tags
            .take()
            .unwrap_or_else(|| self.source.default_tags.to_vec());
        if !tags.is_empty() {
            operation.insert("tags".into(), json!(tags));
        }
        if !b.parameters.is_empty() {
            operation.insert(
                "parameters".into(),
                Value::Array(std::mem::take(&mut b.parameters)),
            );
        }
        if let Some(requirements) = b.security.take() {
            // `{}` alone (only `@security none`) means no security at all.
            let mut security: Vec<Value> = Vec::with_capacity(requirements.len());
            if requirements
                .iter()
                .any(|r| r.as_object().is_none_or(|r| !r.is_empty()))
            {
                for requirement in requirements {
                    if !security.contains(&requirement) {
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
        self.embedded_yaml(&yaml, false, &mut operation, &mut path_item, &mut document);
        // YAML of the module defaults fills what the route leaves open, the
        // innermost module first: each module is parsed on its own, so inner
        // values win over outer ones.
        let mut ends: Vec<usize> = defaults.module_yaml.clone();
        ends.push(defaults.yaml.len());
        let mut starts = vec![0];
        starts.extend(defaults.module_yaml.iter().copied());
        for (start, end) in starts.into_iter().zip(ends).rev() {
            if let Some(module) = defaults.yaml.get(start..end) {
                self.embedded_yaml(module, true, &mut operation, &mut path_item, &mut document);
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
                    params::expansion("path", &single.to_string(), self.source.location);
                if let [only] = undeclared.as_slice()
                    && let Some(Value::Object(expand)) = marker.get_mut(EXPAND_KEY)
                {
                    expand.insert("name".into(), json!(only));
                }
                b.parameters.push(marker);
                true
            }
            positional if positional.len() == placeholders.len() => {
                for (name, ty) in placeholders.iter().zip(positional) {
                    if b.declared.insert(name.clone()) {
                        let param = params::inline_path(name, Some(&ty.to_string()), None, types);
                        add_parameter(&mut b.parameters, param);
                    }
                }
                pending
            }
            positional => {
                self.report(
                    Code::SignatureMismatch,
                    None,
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
        for caps in PLACEHOLDER_RE.captures_iter(&b.path) {
            if !b.declared.contains(&caps[1]) && !deferred {
                self.report(
                    Code::UndeclaredPathParam,
                    None,
                    format!(
                        "path parameter `{}` of `{}` has no definition (use `{{{}: Type}}` or `@param path`)",
                        &caps[1], b.path, &caps[1]
                    ),
                );
                valid = false;
            }
        }
        for declared in &b.declared {
            if !b.path.contains(&format!("{{{declared}}}")) {
                self.report(
                    Code::UnusedPathParam,
                    None,
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
                let mut marker =
                    params::expansion("query", &query.to_string(), self.source.location);
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

    /// Merges the embedded YAML sections into the operation, the path item
    /// and the document. YAML of the module `defaults` only fills gaps.
    fn embedded_yaml(
        &mut self,
        yaml: &[(Scope, Option<usize>, String)],
        defaults: bool,
        operation: &mut Value,
        path_item: &mut Value,
        document: &mut Value,
    ) {
        for scope in [Scope::Operation, Scope::PathItem, Scope::Document] {
            let lines: Vec<(Option<usize>, &str)> = yaml
                .iter()
                .filter(|(s, _, _)| *s == scope)
                .map(|(_, index, line)| (*index, line.as_str()))
                .collect();
            let text: Vec<&str> = lines.iter().map(|(_, line)| *line).collect();
            if text.iter().all(|l| l.trim().is_empty()) {
                continue;
            }
            match yaml::parse(&text.join("\n")) {
                Ok(Value::Null) => {}
                Ok(value) if !value.is_object() => self.report(
                    Code::InvalidRouteYaml,
                    lines.first().and_then(|(i, _)| *i),
                    "embedded OpenAPI YAML must be a mapping and was ignored",
                ),
                Ok(mut value) if defaults => match scope {
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
                },
                Ok(value) => {
                    let conflicts = match scope {
                        Scope::Operation => merge(operation, value, Policy::Operation),
                        Scope::PathItem => merge(path_item, value, Policy::Operation),
                        Scope::Document => merge(document, value, Policy::Document),
                    };
                    let text_lines: Vec<&str> = lines.iter().map(|(_, line)| *line).collect();
                    for pointer in conflicts {
                        let index = conflict_line(&text_lines, &pointer)
                            .or(Some(0))
                            .and_then(|i| lines.get(i))
                            .and_then(|(index, _)| *index);
                        let location = self.location(index);
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
                    let index = lines
                        .get(e.line.saturating_sub(1))
                        .and_then(|(index, _)| *index);
                    self.report(
                        Code::InvalidRouteYaml,
                        index,
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

/// Removes the parameters of default YAML that the operation (or path
/// item) already declares, so the route's own declaration wins.
fn drop_known_parameters(value: &mut Value, target: &Value) {
    let known = target.get("parameters").and_then(Value::as_array);
    if let (Some(known), Some(Value::Array(parameters))) = (known, value.get_mut("parameters")) {
        parameters.retain(|p| !has_parameter(known, p));
    }
}

/// Whether a parameter with the same name and location exists.
fn has_parameter(parameters: &[Value], parameter: &Value) -> bool {
    parameter.get("name").is_some()
        && parameters
            .iter()
            .any(|p| p.get("name") == parameter.get("name") && p.get("in") == parameter.get("in"))
}

/// Adds a parameter; a later declaration of the same name and location
/// replaces the earlier one.
fn add_parameter(parameters: &mut Vec<Value>, parameter: Value) {
    let existing = parameter.get("name").and_then(|_| {
        parameters
            .iter_mut()
            .find(|p| p.get("name") == parameter.get("name") && p.get("in") == parameter.get("in"))
    });
    match existing {
        Some(existing) => *existing = parameter,
        None => parameters.push(parameter),
    }
}

/// Adds a response; repeated codes add their media types.
fn add_response(responses: &mut Map<String, Value>, code: String, response: Value) {
    match responses.get_mut(&code) {
        Some(existing) => {
            merge(existing, response, Policy::Override);
        }
        None => {
            responses.insert(code, response);
        }
    }
}

/// Description lines with their shared indentation removed; blank lines
/// separate paragraphs.
fn description_text(lines: &[String]) -> Option<String> {
    let start = lines.iter().position(|l| !l.trim().is_empty())?;
    let end = lines.iter().rposition(|l| !l.trim().is_empty())?;
    let lines = lines.get(start..=end)?;
    let indent = lines
        .iter()
        .filter(|l| !l.trim().is_empty())
        .map(|l| l.chars().take_while(|c| c.is_whitespace()).count())
        .min()
        .unwrap_or(0);
    Some(
        lines
            .iter()
            .map(|l| l.chars().skip(indent).collect::<String>())
            .collect::<Vec<_>>()
            .join("\n"),
    )
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
