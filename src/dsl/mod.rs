//! The route DSL: `@route` blocks compiled to OpenAPI path items.
//!
//! ```text
//! /// List users
//! /// @route GET /users/{id: u64 "User id"}
//! /// @query-param limit: Option<u32> "Page size"
//! /// @return 200: $Page<User> "Users"
//! ```

mod tokens;

use crate::diagnostics::{Code, Diagnostic, Diagnostics, Location};
use crate::merge::{Policy, merge};
use crate::schema::types::{TypeExpr, TypeMapper};
use crate::yaml;
use regex::Regex;
use serde_json::{Map, Value, json};
use std::collections::BTreeSet;
use std::sync::OnceLock;
use tokens::tokenize;

/// Operation fields that start an embedded YAML section in a route block.
const YAML_KEYS: &[&str] = &[
    "parameters",
    "requestBody",
    "responses",
    "security",
    "externalDocs",
    "callbacks",
    "servers",
];

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
    /// Tags applied when the block has no `@tag`.
    pub default_tags: &'a [String],
    /// Where the block starts.
    pub location: &'a Location,
    /// How Rust types in the block map to schemas.
    pub types: &'a TypeMapper,
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
}

/// A compiled route.
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub struct CompiledRoute {
    /// The normalized path (`/users/{id}`).
    pub path: String,
    /// The lowercase HTTP method.
    pub method: String,
    /// The document fragment: `paths`, plus anything from `@openapi document`.
    pub document: Value,
}

/// Where lines of an embedded YAML section go.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Scope {
    Operation,
    PathItem,
    Document,
}

struct Builder<'a> {
    source: RouteSource<'a>,
    method: String,
    path: String,
    operation: Map<String, Value>,
    parameters: Vec<Value>,
    responses: Map<String, Value>,
    tags: Option<Vec<String>>,
    summary: Option<String>,
    description: Vec<String>,
    /// Embedded YAML lines with their scope and index in the block.
    yaml: Vec<(Scope, usize, String)>,
    scope: Option<Scope>,
    declared: BTreeSet<String>,
}

fn inline_param_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r#"\{(\w+)(?::\s*([^"}]+))?(?:\s*"([^"]+)")?\}"#).unwrap())
}

fn placeholder_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"\{(\w+)\}").unwrap())
}

/// Whether `lines` contain a `@route` directive.
pub fn is_route(lines: &[String]) -> bool {
    lines.iter().any(|l| l.trim().starts_with("@route"))
}

/// Compiles a route block. Returns `None` (with error diagnostics) when the
/// route is invalid.
pub fn compile(source: RouteSource<'_>, diagnostics: &mut Diagnostics) -> Option<CompiledRoute> {
    let mut builder = Builder {
        source,
        method: String::new(),
        path: String::new(),
        operation: Map::new(),
        parameters: Vec::new(),
        responses: Map::new(),
        tags: None,
        summary: None,
        description: Vec::new(),
        yaml: Vec::new(),
        scope: None,
        declared: BTreeSet::new(),
    };
    for (index, line) in source.lines.iter().enumerate() {
        builder.line(index, line, diagnostics);
    }
    builder.finish(diagnostics)
}

impl Builder<'_> {
    fn location(&self, index: usize) -> Location {
        match self.source.line_numbers.get(index) {
            Some(line) => Location::new(self.source.location.file(), *line),
            None => self.source.location.clone(),
        }
    }

    fn line(&mut self, index: usize, line: &str, diagnostics: &mut Diagnostics) {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            match self.scope {
                Some(scope) => self.yaml.push((scope, index, String::new())),
                None if self.summary.is_some() => self.description.push(String::new()),
                None => {}
            }
            return;
        }
        if let Some(directive) = trimmed.strip_prefix('@') {
            self.scope = None;
            self.directive(index, directive, diagnostics);
            return;
        }
        let key = trimmed.split(':').next().unwrap_or_default().trim();
        if self.scope.is_none() && (YAML_KEYS.contains(&key) || key.starts_with("x-")) {
            self.scope = Some(Scope::Operation);
        }
        match self.scope {
            Some(scope) => self.yaml.push((scope, index, line.to_string())),
            None if self.summary.is_none() => self.summary = Some(trimmed.to_string()),
            None => self.description.push(line.to_string()),
        }
    }

    fn directive(&mut self, index: usize, directive: &str, diagnostics: &mut Diagnostics) {
        let (name, rest) = directive
            .split_once(char::is_whitespace)
            .map_or((directive, ""), |(n, r)| (n, r.trim()));
        match name {
            "route" => self.route(index, rest, diagnostics),
            "tag" => {
                self.tags = Some(
                    rest.split(',')
                        .map(|t| t.trim().to_string())
                        .filter(|t| !t.is_empty())
                        .collect(),
                );
            }
            "path-param" => self.parameter(index, "path", rest, diagnostics),
            "query-param" => self.parameter(index, "query", rest, diagnostics),
            "header-param" => self.parameter(index, "header", rest, diagnostics),
            "cookie-param" => self.parameter(index, "cookie", rest, diagnostics),
            "body" => self.body(rest),
            "return" => self.response(index, rest, diagnostics),
            "security" => self.security(rest),
            "openapi" => {
                self.scope = Some(match rest {
                    "" | "operation" => Scope::Operation,
                    "path" => Scope::PathItem,
                    "document" => Scope::Document,
                    other => {
                        diagnostics.push(
                            Diagnostic::new(
                                Code::InvalidDirective,
                                format!("unknown `@openapi` scope `{other}` (use operation, path or document)"),
                            )
                            .at(self.location(index)),
                        );
                        Scope::Operation
                    }
                });
            }
            other => diagnostics.push(
                Diagnostic::new(
                    Code::UnknownDirective,
                    format!("unknown directive `@{other}`"),
                )
                .at(self.location(index)),
            ),
        }
    }

    fn route(&mut self, index: usize, rest: &str, diagnostics: &mut Diagnostics) {
        let Some((method, raw_path)) = rest.split_once(char::is_whitespace) else {
            diagnostics.push(
                Diagnostic::new(Code::InvalidRoute, "`@route` needs a method and a path")
                    .at(self.location(index)),
            );
            return;
        };
        self.method = method.to_lowercase();
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
            self.declared.insert(name.to_string());
            let schema = TypeExpr::parse(type_text.unwrap_or("String")).map_or_else(
                || json!({ "type": "string" }),
                |t| self.source.types.map(&t).schema,
            );
            let mut param =
                json!({ "name": name, "in": "path", "required": true, "schema": schema });
            if let Some(d) = description {
                param["description"] = json!(d);
            }
            self.parameters.push(param);
        }
        path.push_str(&raw_path[last..]);
        self.path = path;
    }

    fn parameter(
        &mut self,
        index: usize,
        location: &str,
        rest: &str,
        diagnostics: &mut Diagnostics,
    ) {
        let Some((name, spec)) = rest.split_once(':') else {
            diagnostics.push(
                Diagnostic::new(
                    Code::InvalidDirective,
                    format!("`@{location}-param` needs `name: [Type] [flags] [\"description\"]`"),
                )
                .at(self.location(index)),
            );
            return;
        };
        let name = name.trim();
        let tokens = tokenize(spec);
        let first = tokens.first().map(String::as_str).unwrap_or_default();
        let is_flag = |t: &str| {
            t == "deprecated" || t == "required" || t.starts_with("example=") || t.starts_with('"')
        };
        let (type_text, flags) = if first.is_empty() || is_flag(first) {
            ("String", &tokens[..])
        } else {
            (first, &tokens[1..])
        };
        let (schema, mut required) = match TypeExpr::parse(type_text) {
            Some(t) => {
                let mapped = self.source.types.map(&t);
                (mapped.schema, mapped.required)
            }
            None => (json!({ "type": "string" }), true),
        };

        let mut deprecated = false;
        let mut example = None;
        let mut description: Vec<&str> = Vec::new();
        for token in flags {
            if !description.is_empty() {
                description.push(token);
            } else if token == "deprecated" {
                deprecated = true;
            } else if token == "required" {
                required = true;
            } else if let Some(value) = token.strip_prefix("example=") {
                example = Some(value.trim_matches('"').to_string());
            } else if token.starts_with('"') {
                description.push(token);
            }
        }
        if location == "path" {
            required = true;
            self.declared.insert(name.to_string());
        }
        let mut param =
            json!({ "name": name, "in": location, "required": required, "schema": schema });
        if deprecated {
            param["deprecated"] = json!(true);
        }
        if let Some(example) = example {
            param["example"] = json!(example);
        }
        if !description.is_empty() {
            param["description"] = json!(description.join(" ").trim_matches('"'));
        }
        self.parameters.push(param);
    }

    fn body(&mut self, rest: &str) {
        let mut parts = rest.split_whitespace();
        let Some(type_text) = parts.next() else {
            return;
        };
        let media = parts.next().unwrap_or("application/json");
        let mut content = Map::new();
        content.insert(
            media.to_string(),
            json!({ "schema": type_schema(type_text, false, self.source.types) }),
        );
        self.operation
            .insert("requestBody".into(), json!({ "content": content }));
    }

    fn response(&mut self, index: usize, rest: &str, diagnostics: &mut Diagnostics) {
        let Some((code, residue)) = rest.split_once(':') else {
            diagnostics.push(
                Diagnostic::new(
                    Code::InvalidDirective,
                    "`@return` needs `code: [Type] [\"description\"]`",
                )
                .at(self.location(index)),
            );
            return;
        };
        let (type_text, description) = split_trailing_description(residue.trim());
        let unit = type_text.is_empty() || type_text == "()" || type_text == "unit";
        let mut response = json!({ "description": description.unwrap_or_default() });
        if !unit {
            response["content"] = json!({ "application/json": { "schema": type_schema(type_text, true, self.source.types) } });
        }
        self.responses.insert(code.trim().to_string(), response);
    }

    fn security(&mut self, rest: &str) {
        let (scheme, scopes) = match rest.find('(') {
            Some(open) => {
                let args = &rest[open + 1..];
                let inner = args.strip_suffix(')').unwrap_or(args);
                let scopes: Vec<String> = inner
                    .split(',')
                    .map(|s| s.trim().trim_matches('"').to_string())
                    .filter(|s| !s.is_empty())
                    .collect();
                (rest[..open].trim(), scopes)
            }
            None => (rest, Vec::new()),
        };
        let mut requirement = Map::new();
        requirement.insert(scheme.to_string(), json!(scopes));
        let security = self
            .operation
            .entry("security")
            .or_insert_with(|| Value::Array(Vec::new()));
        if let Value::Array(list) = security {
            list.push(Value::Object(requirement));
        }
    }

    fn finish(mut self, diagnostics: &mut Diagnostics) -> Option<CompiledRoute> {
        if self.method.is_empty() || self.path.is_empty() {
            if !diagnostics.iter().any(|d| d.code() == Code::InvalidRoute) {
                diagnostics.push(
                    Diagnostic::new(Code::InvalidRoute, "`@route` needs a method and a path")
                        .at(self.source.location.clone()),
                );
            }
            return None;
        }
        let mut valid = true;
        for caps in placeholder_re().captures_iter(&self.path) {
            if !self.declared.contains(&caps[1]) {
                diagnostics.push(
                    Diagnostic::new(
                        Code::UndeclaredPathParam,
                        format!(
                            "path parameter `{}` of `{}` has no definition (use `{{{}: Type}}` or `@path-param`)",
                            &caps[1], self.path, &caps[1]
                        ),
                    )
                    .at(self.source.location.clone()),
                );
                valid = false;
            }
        }
        for declared in &self.declared {
            if !self.path.contains(&format!("{{{declared}}}")) {
                diagnostics.push(
                    Diagnostic::new(
                        Code::UnusedPathParam,
                        format!("path parameter `{declared}` is not used in `{}`", self.path),
                    )
                    .at(self.source.location.clone()),
                );
                valid = false;
            }
        }
        if !valid {
            return None;
        }

        let mut operation = Map::new();
        if let Some(summary) = self.summary.take() {
            operation.insert("summary".into(), json!(summary));
        }
        if let Some(description) = description_text(&self.description) {
            operation.insert("description".into(), json!(description));
        }
        let operation_id = self
            .source
            .operation_id
            .map(str::to_string)
            .unwrap_or_else(|| derived_operation_id(&self.method, &self.path));
        operation.insert("operationId".into(), json!(operation_id));
        let tags = self
            .tags
            .take()
            .unwrap_or_else(|| self.source.default_tags.to_vec());
        operation.insert("tags".into(), json!(tags));
        operation.insert(
            "parameters".into(),
            Value::Array(std::mem::take(&mut self.parameters)),
        );
        operation.extend(std::mem::take(&mut self.operation));
        operation.insert(
            "responses".into(),
            Value::Object(std::mem::take(&mut self.responses)),
        );
        let mut operation = Value::Object(operation);

        let mut path_item = Map::new();
        let mut document = Value::Object(Map::new());
        for scope in [Scope::Operation, Scope::PathItem, Scope::Document] {
            let lines: Vec<(usize, &str)> = self
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
                Ok(value) if !value.is_object() => diagnostics.push(
                    Diagnostic::new(
                        Code::InvalidRouteYaml,
                        "embedded OpenAPI YAML must be a mapping and was ignored",
                    )
                    .at(self.source.location.clone()),
                ),
                Ok(value) => {
                    let conflicts = match scope {
                        Scope::Operation => merge(&mut operation, value, Policy::Operation),
                        Scope::PathItem => {
                            let mut item = Value::Object(std::mem::take(&mut path_item));
                            let conflicts = merge(&mut item, value, Policy::Operation);
                            if let Value::Object(map) = item {
                                path_item = map;
                            }
                            conflicts
                        }
                        Scope::Document => merge(&mut document, value, Policy::Document),
                    };
                    for pointer in conflicts {
                        diagnostics.push(
                            Diagnostic::new(
                                Code::MergeConflict,
                                format!("embedded YAML overrides the DSL value at `{pointer}`"),
                            )
                            .with_severity(crate::diagnostics::Severity::Note)
                            .at(self.source.location.clone()),
                        );
                    }
                }
                Err(e) => {
                    let at = lines.get(e.line - 1).map_or_else(
                        || self.source.location.clone(),
                        |(index, _)| self.location(*index),
                    );
                    diagnostics.push(
                        Diagnostic::new(
                            Code::InvalidRouteYaml,
                            format!(
                                "embedded OpenAPI YAML is invalid and was ignored: {}",
                                e.message
                            ),
                        )
                        .at(at),
                    );
                }
            }
        }

        let mut item = path_item;
        item.insert(self.method.clone(), operation);
        let mut paths = Map::new();
        paths.insert(self.path.clone(), Value::Object(item));
        merge(&mut document, json!({ "paths": paths }), Policy::Document);
        Some(CompiledRoute {
            path: self.path,
            method: self.method,
            document,
        })
    }
}

/// The schema of a DSL type: `$Ref`, `Vec<T>`, `$Page<User>`, `{ inline }`.
fn type_schema(text: &str, allow_inline: bool, types: &TypeMapper) -> Value {
    const STD: &[&str] = &["Option<", "Vec<", "Box<", "Arc<", "Rc<", "Cow<"];
    if allow_inline && text.starts_with('{') {
        return yaml::parse(text).unwrap_or_else(|_| json!({ "$ref": text }));
    }
    if text.contains('<') && !STD.iter().any(|p| text.starts_with(p)) {
        return json!({ "$ref": text });
    }
    if let Some(name) = text.strip_prefix('$') {
        return json!({ "$ref": format!("#/components/schemas/{name}") });
    }
    match TypeExpr::parse(text) {
        Some(t) => types.map(&t).into_schema(),
        None => json!({ "$ref": format!("#/components/schemas/{text}") }),
    }
}

/// Splits `Type "Description"` into the type and its trailing quoted description.
fn split_trailing_description(residue: &str) -> (&str, Option<String>) {
    match residue.strip_suffix('"') {
        Some(body) => match body.rfind('"') {
            Some(open) => (body[..open].trim(), Some(body[open + 1..].to_string())),
            None => (body.trim(), None),
        },
        None => (residue, None),
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
    let mut id = method.to_string();
    for part in path.split(|c: char| !c.is_alphanumeric()) {
        if !part.is_empty() {
            id.push('_');
            id.push_str(&part.to_lowercase());
        }
    }
    id
}
