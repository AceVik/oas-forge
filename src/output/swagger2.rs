//! The specification converted to Swagger 2.0.
//!
//! Schemas are lowered by [`dialect`]; this module
//! converts the document structure: servers become `host`/`basePath`/
//! `schemes`, request bodies `body` or `formData` parameters, media types
//! `consumes`/`produces`, components `definitions`, `parameters`,
//! `responses` and `securityDefinitions`.

use super::refs::rewrite_refs;
use crate::diagnostics::{Code, Diagnostic, Diagnostics};
use crate::schema::dialect::{self, Dialect};
use serde_json::{Map, Value, json};
use std::cell::RefCell;
use std::collections::BTreeSet;

/// Methods a Swagger 2.0 path item has.
const METHODS: &[&str] = &["get", "put", "post", "delete", "options", "head", "patch"];

/// Keywords a non-body parameter, header or array item takes from a schema.
const INLINE: &[&str] = &[
    "type",
    "format",
    "default",
    "maximum",
    "exclusiveMaximum",
    "minimum",
    "exclusiveMinimum",
    "maxLength",
    "minLength",
    "pattern",
    "maxItems",
    "minItems",
    "uniqueItems",
    "enum",
    "multipleOf",
];

/// Form media types, sent as `formData` parameters.
const FORM_MEDIA: &[&str] = &["application/x-www-form-urlencoded", "multipart/form-data"];

/// The canonical document as Swagger 2.0.
pub(super) fn export(canonical: &Value, diagnostics: &mut Diagnostics) -> Value {
    let mut source = canonical.clone();
    dialect::lower(&mut source, Dialect::Swagger20, diagnostics);
    let mut converter = Converter {
        components: source
            .as_object_mut()
            .and_then(|map| map.remove("components"))
            .unwrap_or_else(|| json!({})),
        lost: RefCell::new(BTreeSet::new()),
        dropped_schemes: BTreeSet::new(),
    };
    let mut out = Map::new();
    out.insert("swagger".into(), json!("2.0"));
    // First, so that requirements of left-out schemes can be recognized.
    converter.components(&mut out);
    if let Some(info) = source.get("info") {
        out.insert("info".into(), converter.info(info));
    }
    if let Some(Value::Array(servers)) = source.get("servers") {
        converter.servers(servers, &mut out);
    }
    let mut paths = Map::new();
    if let Some(Value::Object(items)) = source.get("paths") {
        for (path, item) in items {
            paths.insert(path.clone(), converter.path_item(item));
        }
    }
    out.insert("paths".into(), Value::Object(paths));
    converter.definitions(&mut out);
    if let Some(security) = source.get("security") {
        converter.security(security, &mut out);
    }
    if let Some(value) = source.get("externalDocs") {
        out.insert("externalDocs".into(), value.clone());
    }
    if let Some(Value::Array(tags)) = source.get("tags") {
        let tags: Vec<Value> = tags.iter().map(|tag| converter.tag(tag)).collect();
        out.insert("tags".into(), Value::Array(tags));
    }
    let mut document = Value::Object(out);
    for (from, to) in [
        ("#/components/schemas/", "#/definitions/"),
        ("#/components/parameters/", "#/parameters/"),
        ("#/components/responses/", "#/responses/"),
    ] {
        rewrite_refs(&mut document, from, to);
    }
    for what in converter.lost.into_inner() {
        diagnostics.push(Diagnostic::new(
            Code::LossyLowering,
            format!("Swagger 2.0 has no {what}; the export leaves them out or approximates them"),
        ));
    }
    document
}

/// The state of one export: the lowered components and what Swagger 2.0
/// cannot express.
struct Converter {
    /// The lowered components, for resolving referenced request bodies,
    /// form schemas, parameters and headers.
    components: Value,
    /// What the target cannot express, reported once each.
    lost: RefCell<BTreeSet<String>>,
    /// Security schemes Swagger 2.0 cannot express (left out).
    dropped_schemes: BTreeSet<String>,
}

impl Converter {
    /// Records that Swagger 2.0 cannot express `what`; [`export`] reports
    /// each distinct `what` once as [`Code::LossyLowering`].
    fn lose(&self, what: &str) {
        self.lost.borrow_mut().insert(what.to_string());
    }

    /// `info` without `summary` and `license.identifier`; an identifier that
    /// repeats `license.name` is not reported as lost.
    fn info(&self, info: &Value) -> Value {
        let mut info = info.clone();
        if let Value::Object(map) = &mut info {
            if map.remove("summary").is_some() {
                self.lose("`info.summary`");
            }
            // An identifier that repeats the name loses nothing.
            if let Some(Value::Object(license)) = map.get_mut("license")
                && let Some(identifier) = license.remove("identifier")
                && license.get("name") != Some(&identifier)
            {
                self.lose("`license.identifier`");
            }
        }
        info
    }

    /// A tag without the OpenAPI 3.2 `summary`, `parent` and `kind`.
    fn tag(&self, tag: &Value) -> Value {
        let mut tag = tag.clone();
        if let Value::Object(map) = &mut tag {
            for key in ["summary", "parent", "kind"] {
                if map.remove(key).is_some() {
                    self.lose("tag `summary`/`parent`/`kind`");
                }
            }
        }
        tag
    }

    /// `https://api.example.com:8443/v1` → `schemes`, `host`, `basePath`.
    fn servers(&self, servers: &[Value], out: &mut Map<String, Value>) {
        if servers.len() > 1 {
            self.lose("multiple servers (the first one is used)");
        }
        let Some(server) = servers.first() else {
            return;
        };
        let mut url = server["url"].as_str().unwrap_or_default().to_string();
        if let Some(Value::Object(variables)) = server.get("variables") {
            for (name, variable) in variables {
                let default = variable["default"].as_str().unwrap_or_default();
                url = url.replace(&format!("{{{name}}}"), default);
            }
        }
        let (scheme, rest) = match url.split_once("://") {
            Some((scheme, rest)) => (Some(scheme.to_ascii_lowercase()), rest.to_string()),
            None => (None, url.clone()),
        };
        let (host, path) = if scheme.is_some() || url.starts_with("//") {
            let rest = rest.trim_start_matches('/');
            // The path keeps its leading `/`.
            match rest
                .find('/')
                .and_then(|slash| rest.split_at_checked(slash))
            {
                Some((host, path)) => (Some(host.to_string()), path.to_string()),
                None => (Some(rest.to_string()), String::new()),
            }
        } else {
            (None, rest)
        };
        if let Some(host) = host.filter(|h| !h.is_empty() && !h.contains('{')) {
            out.insert("host".into(), json!(host));
        }
        let base = path.trim_end_matches('/');
        if base.starts_with('/') && !base.contains('{') {
            out.insert("basePath".into(), json!(base));
        }
        if let Some(scheme) =
            scheme.filter(|s| ["http", "https", "ws", "wss"].contains(&s.as_str()))
        {
            out.insert("schemes".into(), json!([scheme]));
        }
    }

    /// A path item with its operations and parameters converted; keeps
    /// `$ref` and `x-` extensions and loses `trace`, `summary`,
    /// `description` and `servers`.
    fn path_item(&self, item: &Value) -> Value {
        let Value::Object(item) = item else {
            return item.clone();
        };
        let mut out = Map::new();
        for (key, value) in item {
            match key.as_str() {
                "$ref" => {
                    out.insert(key.clone(), value.clone());
                }
                "parameters" => {
                    let parameters = self.parameters(value);
                    out.insert(key.clone(), Value::Array(parameters));
                }
                method if METHODS.contains(&method) => {
                    out.insert(key.clone(), self.operation(value));
                }
                "trace" => self.lose("`TRACE` operations"),
                k if k.starts_with("x-") => {
                    out.insert(key.clone(), value.clone());
                }
                _ => self.lose("path item `summary`, `description` and `servers`"),
            }
        }
        Value::Object(out)
    }

    /// An operation with its request body as `body`/`formData` parameters,
    /// media types as `consumes`/`produces` and converted responses; loses
    /// `callbacks` and `servers`.
    fn operation(&self, operation: &Value) -> Value {
        let Value::Object(operation) = operation else {
            return operation.clone();
        };
        let mut out = Map::new();
        let mut parameters = operation
            .get("parameters")
            .map(|p| self.parameters(p))
            .unwrap_or_default();
        let mut consumes = Vec::new();
        if let Some(body) = operation.get("requestBody") {
            let body = self.resolve(body, "requestBodies");
            consumes = self.body(body, &mut parameters);
        }
        let mut produces = Vec::new();
        let mut responses = Map::new();
        if let Some(Value::Object(source)) = operation.get("responses") {
            for (code, response) in self.statuses(source) {
                let response = self.response(response, &mut produces);
                responses.insert(code, response);
            }
        }
        for (key, value) in operation {
            match key.as_str() {
                "tags" | "summary" | "description" | "externalDocs" | "operationId"
                | "deprecated" => {
                    out.insert(key.clone(), value.clone());
                }
                "security" => self.security(value, &mut out),
                k if k.starts_with("x-") => {
                    out.insert(key.clone(), value.clone());
                }
                "parameters" | "requestBody" | "responses" => {}
                "callbacks" => self.lose("callbacks"),
                _ => self.lose("operation `servers`"),
            }
        }
        if !consumes.is_empty() {
            out.insert("consumes".into(), json!(consumes));
        }
        if !produces.is_empty() {
            out.insert("produces".into(), json!(produces));
        }
        if !parameters.is_empty() {
            out.insert("parameters".into(), Value::Array(parameters));
        }
        out.insert("responses".into(), Value::Object(responses));
        Value::Object(out)
    }

    /// The responses by Swagger status: `2XX` becomes `200` (Swagger 2.0
    /// has no status code ranges) unless the operation has an explicit
    /// `200`, which wins.
    fn statuses<'a>(&self, responses: &'a Map<String, Value>) -> Vec<(String, &'a Value)> {
        let explicit: BTreeSet<&str> = responses
            .keys()
            .map(String::as_str)
            .filter(|code| !code.ends_with("XX"))
            .collect();
        let mut out = Vec::new();
        for (code, response) in responses {
            match code.strip_suffix("XX") {
                Some(class) => {
                    self.lose("status code ranges (`2XX` is written as `200`)");
                    let status = format!("{class}00");
                    if explicit.contains(status.as_str()) {
                        self.lose(
                            "status code ranges that collide with an explicit status (the explicit one is kept)",
                        );
                    } else {
                        out.push((status, response));
                    }
                }
                None => out.push((code.clone(), response)),
            }
        }
        out
    }

    /// Writes `security` without the alternatives that name a scheme the
    /// export leaves out; when none remains, the requirements are kept as
    /// `x-security` so the document does not claim the operation is public.
    fn security(&self, requirements: &Value, out: &mut Map<String, Value>) {
        let Value::Array(alternatives) = requirements else {
            out.insert("security".into(), requirements.clone());
            return;
        };
        let names_dropped = |alternative: &Value| {
            alternative
                .as_object()
                .is_some_and(|names| names.keys().any(|n| self.dropped_schemes.contains(n)))
        };
        let kept: Vec<Value> = alternatives
            .iter()
            .filter(|a| !names_dropped(a))
            .cloned()
            .collect();
        if kept.len() == alternatives.len() {
            out.insert("security".into(), requirements.clone());
            return;
        }
        self.lose(
            "security requirements that name schemes it cannot express (kept as `x-security` when no alternative remains)",
        );
        if kept.is_empty() {
            out.insert("x-security".into(), requirements.clone());
        } else {
            out.insert("security".into(), Value::Array(kept));
        }
    }

    /// The converted parameters of a `parameters` array, without cookie
    /// parameters (empty when it is not an array).
    fn parameters(&self, parameters: &Value) -> Vec<Value> {
        let Value::Array(parameters) = parameters else {
            return Vec::new();
        };
        parameters
            .iter()
            .filter_map(|p| self.parameter(p))
            .collect()
    }

    /// A parameter with its schema inlined and, for an array, `style` and
    /// `explode` as `collectionFormat`; `deprecated` and `example` become
    /// `x-deprecated` and `x-example`. `None` for a cookie parameter or a
    /// `$ref` to one.
    fn parameter(&self, parameter: &Value) -> Option<Value> {
        let Value::Object(source) = parameter else {
            return Some(parameter.clone());
        };
        if source.contains_key("$ref") {
            // A reference to a left-out (cookie) parameter goes with it.
            let target = self.resolve(parameter, "parameters");
            if target.get("in").and_then(Value::as_str) == Some("cookie") {
                self.lose("cookie parameters");
                return None;
            }
            return Some(parameter.clone());
        }
        let location = source.get("in").and_then(Value::as_str).unwrap_or_default();
        if location == "cookie" {
            self.lose("cookie parameters");
            return None;
        }
        let mut out = Map::new();
        for key in ["name", "in", "description", "required"] {
            if let Some(value) = source.get(key) {
                out.insert(key.into(), value.clone());
            }
        }
        if location == "path" {
            out.insert("required".into(), json!(true));
        }
        if location == "query"
            && let Some(allow) = source.get("allowEmptyValue")
        {
            out.insert("allowEmptyValue".into(), allow.clone());
        }
        let schema = source
            .get("schema")
            .cloned()
            .or_else(|| first_media_schema(source.get("content")))
            .unwrap_or_else(|| json!({ "type": "string" }));
        self.inline(&schema, &mut out);
        if out.get("type") == Some(&json!("array")) {
            let style = source.get("style").and_then(Value::as_str);
            let explode = source.get("explode").and_then(Value::as_bool);
            let format = match (location, style, explode) {
                ("query" | "cookie", None | Some("form"), None | Some(true)) => "multi",
                (_, Some("spaceDelimited"), _) => "ssv",
                (_, Some("pipeDelimited"), _) => "pipes",
                _ => "csv",
            };
            out.insert("collectionFormat".into(), json!(format));
        }
        if let Some(value) = source.get("deprecated") {
            out.insert("x-deprecated".into(), value.clone());
        }
        if let Some(value) = source.get("example") {
            out.insert("x-example".into(), value.clone());
        }
        Some(Value::Object(out))
    }

    /// Copies the primitive keywords of `schema` into a parameter, header or
    /// item; anything else is documented as a string.
    fn inline(&self, schema: &Value, out: &mut Map<String, Value>) {
        let primitive = schema.get("$ref").is_none()
            && matches!(
                schema.get("type").and_then(Value::as_str),
                Some("string" | "number" | "integer" | "boolean" | "array")
            );
        if !primitive {
            self.lose(
                "parameters and headers that are objects or references (documented as strings)",
            );
            out.insert("type".into(), json!("string"));
            if let Some(description) = schema.get("description")
                && !out.contains_key("description")
            {
                out.insert("description".into(), description.clone());
            }
            return;
        }
        for key in INLINE {
            if let Some(value) = schema.get(*key) {
                out.insert((*key).to_string(), value.clone());
            }
        }
        if out.get("type") == Some(&json!("array")) {
            let mut items = Map::new();
            self.inline(schema.get("items").unwrap_or(&json!({})), &mut items);
            out.insert("items".into(), Value::Object(items));
        }
        if schema
            .get("nullable")
            .or_else(|| schema.get("x-nullable"))
            .is_some()
        {
            out.insert("x-nullable".into(), json!(true));
        }
    }

    /// A request body as `body` or `formData` parameters; returns `consumes`.
    fn body(&self, body: &Value, parameters: &mut Vec<Value>) -> Vec<String> {
        let Some(Value::Object(content)) = body.get("content") else {
            return Vec::new();
        };
        let consumes: Vec<String> = content.keys().cloned().collect();
        let required = body.get("required").cloned().unwrap_or(json!(false));
        let form = consumes.iter().any(|m| FORM_MEDIA.contains(&m.as_str()));
        let other = consumes.iter().find(|m| !FORM_MEDIA.contains(&m.as_str()));
        match (other, form) {
            (Some(media), form) => {
                if form {
                    self.lose("bodies that are both forms and documents (the document is used)");
                }
                let schema = content[media].get("schema").cloned().unwrap_or(json!({}));
                let mut parameter = Map::new();
                parameter.insert("name".into(), json!("body"));
                parameter.insert("in".into(), json!("body"));
                parameter.insert("required".into(), required);
                parameter.insert("schema".into(), schema);
                if let Some(description) = body.get("description") {
                    parameter.insert("description".into(), description.clone());
                }
                parameters.push(Value::Object(parameter));
            }
            (None, _) => {
                let multipart = consumes.iter().any(|m| m == "multipart/form-data");
                let schema = consumes
                    .first()
                    .and_then(|m| content[m].get("schema"))
                    .cloned()
                    .unwrap_or(json!({}));
                let schema = self.resolve(&schema, "schemas");
                let names: Vec<&Value> = schema
                    .get("required")
                    .and_then(Value::as_array)
                    .map(|r| r.iter().collect())
                    .unwrap_or_default();
                let Some(Value::Object(properties)) = schema.get("properties") else {
                    self.lose("form bodies that are not objects");
                    return consumes;
                };
                for (name, property) in properties {
                    let mut parameter = Map::new();
                    parameter.insert("name".into(), json!(name));
                    parameter.insert("in".into(), json!("formData"));
                    parameter.insert("required".into(), json!(names.contains(&&json!(name))));
                    if let Some(description) = property.get("description") {
                        parameter.insert("description".into(), description.clone());
                    }
                    let binary = property.get("format").and_then(Value::as_str) == Some("binary");
                    if multipart && binary {
                        parameter.insert("type".into(), json!("file"));
                    } else {
                        self.inline(property, &mut parameter);
                    }
                    parameters.push(Value::Object(parameter));
                }
            }
        }
        consumes
    }

    /// A response with one `schema` (see [`preferred_schema`]) and inlined
    /// headers; adds its media types to `produces` and keeps a `$ref` as is.
    fn response(&self, response: &Value, produces: &mut Vec<String>) -> Value {
        let Value::Object(source) = response else {
            return response.clone();
        };
        if source.contains_key("$ref") {
            return response.clone();
        }
        let mut out = Map::new();
        out.insert(
            "description".into(),
            source.get("description").cloned().unwrap_or(json!("")),
        );
        if let Some(Value::Object(content)) = source.get("content") {
            for media in content.keys() {
                if !produces.contains(media) {
                    produces.push(media.clone());
                }
            }
            if let Some(schema) = preferred_schema(content) {
                out.insert("schema".into(), schema);
            }
        }
        if let Some(Value::Object(headers)) = source.get("headers") {
            let mut converted = Map::new();
            for (name, header) in headers {
                let header = self.resolve(header, "headers");
                let mut out = Map::new();
                if let Some(description) = header.get("description") {
                    out.insert("description".into(), description.clone());
                }
                let schema = header
                    .get("schema")
                    .cloned()
                    .unwrap_or(json!({ "type": "string" }));
                self.inline(&schema, &mut out);
                converted.insert(name.clone(), Value::Object(out));
            }
            out.insert("headers".into(), Value::Object(converted));
        }
        if source.contains_key("links") {
            self.lose("response links");
        }
        for (key, value) in source {
            if key.starts_with("x-") {
                out.insert(key.clone(), value.clone());
            }
        }
        Value::Object(out)
    }

    /// The target of a `$ref` into `components.<section>` (`Null` when it
    /// does not exist), or the value itself.
    fn resolve<'a>(&'a self, value: &'a Value, section: &str) -> &'a Value {
        const MISSING: &Value = &Value::Null;
        let prefix = format!("#/components/{section}/");
        match value
            .get("$ref")
            .and_then(Value::as_str)
            .and_then(|r| r.strip_prefix(&prefix))
        {
            Some(name) => self
                .components
                .get(section)
                .and_then(|entries| entries.get(name))
                .unwrap_or(MISSING),
            None => value,
        }
    }

    /// Converts the reusable parameters, responses and security schemes
    /// (`schemas` move to `definitions` at the end of the export, after the
    /// form bodies that use them).
    fn components(&mut self, out: &mut Map<String, Value>) {
        let Value::Object(components) = &self.components else {
            return;
        };
        let mut dropped = Vec::new();
        for (section, value) in components {
            match section.as_str() {
                "schemas" | "requestBodies" => {
                    // Request bodies are inlined into the operations that use them.
                }
                "parameters" => {
                    let mut parameters = Map::new();
                    for (name, parameter) in value.as_object().into_iter().flatten() {
                        if let Some(parameter) = self.parameter(parameter) {
                            parameters.insert(name.clone(), parameter);
                        }
                    }
                    out.insert("parameters".into(), Value::Object(parameters));
                }
                "responses" => {
                    let mut responses = Map::new();
                    for (name, response) in value.as_object().into_iter().flatten() {
                        responses.insert(name.clone(), self.response(response, &mut Vec::new()));
                    }
                    out.insert("responses".into(), Value::Object(responses));
                }
                "securitySchemes" => {
                    let mut definitions = Map::new();
                    for (name, scheme) in value.as_object().into_iter().flatten() {
                        match self.security_scheme(scheme) {
                            Some(definition) => {
                                definitions.insert(name.clone(), definition);
                            }
                            None => dropped.push(name.clone()),
                        }
                    }
                    out.insert("securityDefinitions".into(), Value::Object(definitions));
                }
                k if k.starts_with("x-") => {}
                other => self.lose(&format!("`components.{other}`")),
            }
        }
        self.dropped_schemes.extend(dropped);
    }

    /// Moves the lowered component schemas to `definitions`.
    fn definitions(&mut self, out: &mut Map<String, Value>) {
        if let Some(schemas) = self
            .components
            .as_object_mut()
            .and_then(|components| components.remove("schemas"))
        {
            out.insert("definitions".into(), schemas);
        }
    }

    /// The `securityDefinitions` entry of a security scheme; `None` for a
    /// scheme without `type` and for schemes Swagger 2.0 cannot express
    /// (API keys in cookies, `openIdConnect`, `mutualTLS`, `oauth2` without a
    /// known flow). HTTP schemes other than basic become an `Authorization`
    /// header API key.
    fn security_scheme(&self, scheme: &Value) -> Option<Value> {
        let description = scheme.get("description").cloned();
        let mut out = match scheme["type"].as_str()? {
            "http"
                if scheme["scheme"]
                    .as_str()
                    .is_some_and(|s| s.eq_ignore_ascii_case("basic")) =>
            {
                json!({ "type": "basic" })
            }
            "http" => {
                self.lose("HTTP authentication other than basic (documented as an `Authorization` header)");
                json!({ "type": "apiKey", "in": "header", "name": "Authorization" })
            }
            "apiKey" if scheme["in"] == "cookie" => {
                self.lose("API keys in cookies");
                return None;
            }
            "apiKey" => json!({ "type": "apiKey", "in": scheme["in"], "name": scheme["name"] }),
            "oauth2" => {
                let flows = scheme.get("flows").and_then(Value::as_object)?;
                if flows.len() > 1 {
                    self.lose("several OAuth2 flows in one scheme (the first in Swagger order is used: implicit, password, application, accessCode)");
                }
                let (flow, settings) = [
                    "implicit",
                    "password",
                    "clientCredentials",
                    "authorizationCode",
                ]
                .iter()
                .find_map(|name| flows.get(*name).map(|settings| (*name, settings)))?;
                let mut out = Map::new();
                out.insert("type".into(), json!("oauth2"));
                out.insert(
                    "flow".into(),
                    json!(match flow {
                        "clientCredentials" => "application",
                        "authorizationCode" => "accessCode",
                        other => other,
                    }),
                );
                out.insert(
                    "scopes".into(),
                    settings.get("scopes").cloned().unwrap_or(json!({})),
                );
                for key in ["authorizationUrl", "tokenUrl"] {
                    if let Some(url) = settings.get(key) {
                        out.insert(key.into(), url.clone());
                    }
                }
                Value::Object(out)
            }
            _ => {
                self.lose("OpenID Connect and mutual TLS security schemes");
                return None;
            }
        };
        if let Some(description) = description
            && let Value::Object(map) = &mut out
        {
            map.insert("description".into(), description);
        }
        Some(out)
    }
}

/// The schema of the first media type of a parameter's `content`.
fn first_media_schema(content: Option<&Value>) -> Option<Value> {
    content?
        .as_object()?
        .values()
        .next()?
        .get("schema")
        .cloned()
}

/// The schema of a JSON media type, else of the first media type.
fn preferred_schema(content: &Map<String, Value>) -> Option<Value> {
    let json = content
        .iter()
        .find(|(media, _)| media.as_str() == "application/json" || media.ends_with("+json"));
    json.or_else(|| content.iter().next())
        .and_then(|(_, media)| media.get("schema").cloned())
}
