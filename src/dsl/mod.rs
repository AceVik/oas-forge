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
use crate::schema::{TypeExpr, TypeMapper};
use crate::yaml;
use operation::Security;
use regex::Regex;
use serde_json::{Map, Value, json};
use std::collections::BTreeSet;
use std::sync::OnceLock;

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
#[derive(Debug, Clone, Default, PartialEq, Eq)]
#[non_exhaustive]
pub struct Signature {
    /// The types of the path parameters, in path order (`Path<(A, B)>`), or
    /// a single type or struct (`Path<T>`).
    pub path: Vec<TypeExpr>,
    /// The struct whose fields are query parameters (`Query<T>`).
    pub query: Option<TypeExpr>,
    /// The request body type and media type (`Json<T>`, `Form<T>`).
    pub body: Option<(TypeExpr, String)>,
    /// The type of the successful response (`Json<T>`, `Result<Json<T>, E>`).
    pub response: Option<TypeExpr>,
}

/// One route block.
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
/// .unwrap();
/// assert_eq!(route.path, "/ping");
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
    /// `@route-defaults` lines of the enclosing modules, outermost first.
    pub defaults: &'a [String],
    /// The handler's signature, used to fill gaps of the block.
    pub signature: Option<&'a Signature>,
}

impl<'a> RouteSource<'a> {
    /// A block of `lines` starting at `location`, with the built-in type table.
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
    pub fn line_numbers(mut self, line_numbers: &'a [usize]) -> Self {
        self.line_numbers = line_numbers;
        self
    }

    /// Sets the default operationId (derived from method and path when `None`).
    pub fn operation_id(mut self, operation_id: Option<&'a str>) -> Self {
        self.operation_id = operation_id;
        self
    }

    /// Sets the tags of routes without `@tag`.
    pub fn default_tags(mut self, tags: &'a [String]) -> Self {
        self.default_tags = tags;
        self
    }

    /// Sets the type mapping.
    pub fn types(mut self, types: &'a TypeMapper) -> Self {
        self.types = types;
        self
    }

    /// Sets the `@route-defaults` lines of the enclosing modules.
    pub fn defaults(mut self, defaults: &'a [String]) -> Self {
        self.defaults = defaults;
        self
    }

    /// Sets the handler's signature for inference.
    pub fn signature(mut self, signature: Option<&'a Signature>) -> Self {
        self.signature = signature;
        self
    }
}

/// A compiled route.
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
    Operation,
    PathItem,
    Document,
}

#[derive(Default)]
struct Builder {
    method: String,
    path: String,
    webhook: Option<String>,
    prefix: String,
    operation: Map<String, Value>,
    parameters: Vec<Value>,
    responses: Map<String, Value>,
    security: Option<Vec<Value>>,
    tags: Option<Vec<String>>,
    summary: Option<String>,
    description: Vec<String>,
    in_description: bool,
    yaml: Vec<(Scope, usize, String)>,
    scope: Option<Scope>,
    declared: BTreeSet<String>,
    document: Map<String, Value>,
    no_infer: bool,
    query_declared: bool,
    route_lines: usize,
}

fn inline_param_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r#"\{(\w+)(?::\s*([^"}]+))?(?:\s*"([^"]+)")?\}"#).unwrap())
}

fn placeholder_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"\{(\w+)\}").unwrap())
}

fn is_header(line: &str) -> bool {
    let line = line.trim();
    ["@route", "@webhook"].iter().any(|header| {
        line.strip_prefix(header)
            .is_some_and(|rest| rest.is_empty() || rest.starts_with(char::is_whitespace))
    })
}

/// Whether `lines` contain a `@route` or `@webhook` directive.
pub fn is_route(lines: &[String]) -> bool {
    lines.iter().any(|l| is_header(l))
}

/// Compiles a route block. Returns `None` (with error diagnostics) when the
/// route is invalid.
pub fn compile(source: RouteSource<'_>, diagnostics: &mut Diagnostics) -> Option<CompiledRoute> {
    let mut compiler = Compiler {
        source,
        diagnostics,
    };
    // Module defaults are compiled like a block without `@route`.
    let mut defaults = Builder::default();
    for line in source.defaults {
        compiler.line(&mut defaults, None, line, true);
    }
    let mut builder = Builder::default();
    for (index, line) in source.lines.iter().enumerate() {
        compiler.line(&mut builder, Some(index), line, false);
    }
    compiler.finish(builder, defaults)
}

struct Compiler<'s, 'd> {
    source: RouteSource<'s>,
    diagnostics: &'d mut Diagnostics,
}

impl Compiler<'_, '_> {
    fn location(&self, index: Option<usize>) -> Location {
        match index.and_then(|i| self.source.line_numbers.get(i)) {
            Some(line) => Location::new(self.source.location.file(), *line),
            None => self.source.location.clone(),
        }
    }

    fn report(&mut self, code: Code, index: Option<usize>, message: impl Into<String>) {
        let location = self.location(index);
        self.diagnostics
            .push(Diagnostic::new(code, message).at(location));
    }

    fn invalid(&mut self, index: Option<usize>, message: impl Into<String>) {
        self.report(Code::InvalidDirective, index, message);
    }

    fn line(&mut self, b: &mut Builder, index: Option<usize>, line: &str, defaults: bool) {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            match b.scope {
                Some(scope) => b.yaml.push((scope, index.unwrap_or(0), String::new())),
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
        let key = trimmed.split(':').next().unwrap_or_default().trim();
        if b.scope.is_none()
            && !b.in_description
            && (YAML_KEYS.contains(&key) || key.starts_with("x-"))
        {
            b.scope = Some(Scope::Operation);
        }
        match b.scope {
            Some(scope) => b.yaml.push((scope, index.unwrap_or(0), line.to_string())),
            None if b.in_description || b.summary.is_some() => {
                b.description.push(line.to_string());
            }
            None => b.summary = Some(trimmed.to_string()),
        }
    }

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
            "prefix" if defaults => b.prefix.push_str(rest.trim_end_matches('/')),
            "tag" => {
                b.tags = Some(
                    rest.split(',')
                        .map(|t| t.trim().to_string())
                        .filter(|t| !t.is_empty())
                        .collect(),
                );
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
            "external-docs" => match operation::external_docs(rest) {
                Some(docs) => {
                    b.operation.insert("externalDocs".into(), docs);
                }
                None => self.invalid(index, "`@external-docs` needs `URL [\"description\"]`"),
            },
            "server" => match operation::server(rest) {
                Some(server) => push(&mut b.operation, "servers", server),
                None => self.invalid(index, "`@server` needs `URL [\"description\"]`"),
            },
            "security" => match operation::security(rest) {
                // `{}` allows anonymous access; alone it means no security.
                Some(Security::None) => b.security.get_or_insert_with(Vec::new).push(json!({})),
                Some(Security::Requirement(requirement)) => b
                    .security
                    .get_or_insert_with(Vec::new)
                    .push(Value::Object(requirement)),
                None => self.invalid(
                    index,
                    "`@security` needs `scheme[(scopes)] [+ scheme...]` or `none`",
                ),
            },
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
                    if location == "query" {
                        b.query_declared = true;
                    }
                    b.parameters.push(params::expansion(location, rest));
                }
            }
            "body" => match responses::body(rest, types) {
                Some(body) => {
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
                None => self.invalid(index, "`@body` needs a type"),
            },
            "return" => match responses::response(rest, types) {
                Some(response) => add_response(&mut b.responses, response.code, response.value),
                None => self.invalid(
                    index,
                    "`@return` needs `code: [Type] [media...] [\"description\"]`",
                ),
            },
            "return-header" => match responses::header(rest, types) {
                Some((code, name, header)) => {
                    let response = b.responses.entry(code.clone()).or_insert_with(
                        || json!({ "description": responses::reason_phrase(&code) }),
                    );
                    response["headers"][name] = header;
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
        match location {
            "path" => {
                b.declared.insert(parameter.name);
            }
            "query" | "querystring" => b.query_declared = true,
            _ => {}
        }
        add_parameter(&mut b.parameters, parameter.value);
    }

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
        let raw_path = raw_path.trim();
        let mut path = String::new();
        let mut last = 0;
        for caps in inline_param_re().captures_iter(raw_path) {
            let whole = caps.get(0).unwrap();
            let name = &caps[1];
            path.push_str(&raw_path[last..whole.start()]);
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
        path.push_str(&raw_path[last..]);
        b.path = path;
    }

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

    fn finish(&mut self, mut b: Builder, defaults: Builder) -> Option<CompiledRoute> {
        if b.method.is_empty() || (b.path.is_empty() && b.webhook.is_none()) {
            if !self
                .diagnostics
                .iter()
                .any(|d| d.code() == Code::InvalidRoute)
            {
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
            b.path = format!("{}{}", defaults.prefix, b.path);
            if b.path.is_empty() {
                b.path.push('/');
            }
        }
        // Defaults: parameters first, the rest only where the route has none.
        for parameter in defaults.parameters.into_iter().rev() {
            if !has_parameter(&b.parameters, &parameter) {
                b.parameters.insert(0, parameter);
            }
        }
        b.declared.extend(defaults.declared);
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
        if let Some(mut security) = b.security.take() {
            if security.iter().all(|r| *r == json!({})) {
                security.clear();
            }
            let mut seen = Vec::new();
            security.retain(|r| {
                let new = !seen.contains(r);
                seen.push(r.clone());
                new
            });
            operation.insert("security".into(), Value::Array(security));
        }
        operation.extend(std::mem::take(&mut b.operation));
        operation.insert(
            "responses".into(),
            Value::Object(std::mem::take(&mut b.responses)),
        );
        let mut operation = Value::Object(operation);

        let mut path_item = Value::Object(Map::new());
        self.embedded_yaml(&b, &mut operation, &mut path_item, &mut document);
        if let Some(signature) = signature {
            self.infer_rest(&b, &mut operation, signature);
        }
        // OpenAPI requires at least one response.
        if let Some(Value::Object(responses)) = operation.get_mut("responses")
            && responses.is_empty()
        {
            responses.insert(
                "default".into(),
                json!({ "description": responses::reason_phrase("default") }),
            );
        }

        order_fields(&mut operation);

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
        let pending = b
            .parameters
            .iter()
            .any(|p| p.get(EXPAND_KEY).is_some_and(|m| m["in"] == "path"));
        let Some(signature) = signature else {
            return pending;
        };
        let placeholders: Vec<String> = placeholder_re()
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
                let mut marker = params::expansion("path", &single.to_string());
                if let [only] = undeclared.as_slice() {
                    marker[EXPAND_KEY]["name"] = json!(only);
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
        for caps in placeholder_re().captures_iter(&b.path) {
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
    /// the block (including its YAML) has none.
    fn infer_rest(&mut self, b: &Builder, operation: &mut Value, signature: &Signature) {
        let types = self.source.types;
        let Value::Object(map) = operation else {
            return;
        };
        if let Some(query) = &signature.query {
            let yaml_query = map
                .get("parameters")
                .and_then(Value::as_array)
                .is_some_and(|ps| {
                    ps.iter()
                        .any(|p| p["in"] == "query" || p["in"] == "querystring")
                });
            if !b.query_declared && !yaml_query && is_custom(query, types) {
                push(
                    map,
                    "parameters",
                    params::expansion("query", &query.to_string()),
                );
            }
        }
        if let Some((ty, media)) = &signature.body
            && !map.contains_key("requestBody")
        {
            let mapped = types.map(ty);
            let required = mapped.required;
            let schema = mapped.into_schema();
            let mut content = Map::new();
            content.insert(media.clone(), json!({ "schema": schema }));
            map.insert(
                "requestBody".into(),
                json!({ "content": content, "required": required }),
            );
        }
        if let Some(ty) = &signature.response {
            let responses = map.entry("responses").or_insert_with(|| json!({}));
            let has_success = responses.as_object().is_some_and(|r| {
                r.keys()
                    .any(|code| code.starts_with('2') || code == "default")
            });
            if !has_success {
                let schema = types.map(ty).into_schema();
                let mut content = Map::new();
                content.insert(responses::DEFAULT_MEDIA.into(), json!({ "schema": schema }));
                responses["200"] = json!({
                    "description": responses::reason_phrase("200"),
                    "content": content,
                });
            }
        }
    }

    /// Merges the embedded YAML sections into the operation, the path item
    /// and the document.
    fn embedded_yaml(
        &mut self,
        b: &Builder,
        operation: &mut Value,
        path_item: &mut Value,
        document: &mut Value,
    ) {
        for scope in [Scope::Operation, Scope::PathItem, Scope::Document] {
            let lines: Vec<(usize, &str)> = b
                .yaml
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
                    lines.first().map(|(i, _)| *i),
                    "embedded OpenAPI YAML must be a mapping and was ignored",
                ),
                Ok(value) => {
                    let conflicts = match scope {
                        Scope::Operation => merge(operation, value, Policy::Operation),
                        Scope::PathItem => merge(path_item, value, Policy::Operation),
                        Scope::Document => merge(document, value, Policy::Document),
                    };
                    for pointer in conflicts {
                        let location = self.source.location.clone();
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
                    let index = lines.get(e.line - 1).map(|(index, _)| *index);
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
fn order_fields(operation: &mut Value) {
    let Value::Object(map) = operation else {
        return;
    };
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

fn push(map: &mut Map<String, Value>, key: &str, value: Value) {
    match map.entry(key).or_insert_with(|| json!([])) {
        Value::Array(items) => items.push(value),
        other => *other = json!([value]),
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
    let position = parameter.get("name").and_then(|_| {
        parameters.iter().position(|p| {
            p.get("name") == parameter.get("name") && p.get("in") == parameter.get("in")
        })
    });
    match position {
        Some(position) => parameters[position] = parameter,
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
    let lines = &lines[start..=end];
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
