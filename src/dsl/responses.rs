//! Request bodies and responses: `@body`, `@return`, `@return-header` and
//! `@problem`.

use super::tokens::{is_media_type, tokenize, trailing_description};
use crate::schema::{TypeExpr, TypeMapper};
use crate::yaml;
use serde_json::{Map, Value, json};

/// The media type of bodies and responses without one.
pub(super) const DEFAULT_MEDIA: &str = "application/json";
/// The media type of `stream<T>` responses without one (server-sent events).
const STREAM_MEDIA: &str = "text/event-stream";

/// The RFC 9457 problem details schema, added by `@problem`.
pub(super) const PROBLEM_SCHEMA: &str = "ProblemDetails";

/// A request body: `@body [optional] Type [media...] ["description"]`.
pub(super) struct Body {
    pub(super) content: Map<String, Value>,
    pub(super) required: bool,
    pub(super) description: Option<String>,
}

/// Parses a `@body` directive; `None` without a type.
pub(super) fn body(rest: &str, types: &TypeMapper) -> Option<Body> {
    let (rest, description) = trailing_description(rest);
    let (rest, optional) = match rest.strip_prefix("optional") {
        Some(after) if after.starts_with(char::is_whitespace) => (after.trim(), true),
        _ => (rest, false),
    };
    let (type_text, media) = split_media(rest);
    if type_text.is_empty() {
        return None;
    }
    let optional =
        optional || TypeExpr::parse(&type_text).is_some_and(|t| t.name() == Some("Option"));
    let schema = type_schema(&type_text, types);
    let mut content = Map::new();
    for media in media_or(&media, DEFAULT_MEDIA) {
        content.insert(media, json!({ "schema": schema }));
    }
    Some(Body {
        content,
        required: !optional,
        description,
    })
}

/// A response: `@return CODE: [Type] [media...] ["description"]`.
pub(super) struct Response {
    pub(super) code: String,
    pub(super) value: Value,
}

/// Parses a `@return` directive; `None` without `CODE:`.
pub(super) fn response(rest: &str, types: &TypeMapper) -> Option<Response> {
    let (code, residue) = rest.split_once(':')?;
    let code = code.trim().to_string();
    if code.is_empty() || code.contains(char::is_whitespace) {
        return None;
    }
    let (residue, description) = trailing_description(residue);
    let description = description
        .filter(|d| !d.is_empty())
        .unwrap_or_else(|| reason_phrase(&code).to_string());
    let mut value = json!({ "description": description });
    let (type_text, media) = split_media(residue);
    let type_text = type_text.as_str();
    let unit = type_text.is_empty() || type_text == "()" || type_text == "unit";
    if !unit || !media.is_empty() {
        let mut content = Map::new();
        if let Some(item) = type_text
            .strip_prefix("stream<")
            .and_then(|t| t.strip_suffix('>'))
        {
            // OpenAPI 3.2: a stream of items (SSE, JSON Lines, ...).
            let schema = type_schema(item, types);
            for media in media_or(&media, STREAM_MEDIA) {
                content.insert(media, json!({ "itemSchema": schema }));
            }
        } else {
            let entry = if unit {
                json!({})
            } else {
                json!({ "schema": type_schema(type_text, types) })
            };
            for media in media_or(&media, DEFAULT_MEDIA) {
                content.insert(media, entry.clone());
            }
        }
        value["content"] = Value::Object(content);
    }
    Some(Response { code, value })
}

/// A response header: `@return-header CODE Name: Type ["description"]`.
pub(super) fn header(rest: &str, types: &TypeMapper) -> Option<(String, String, Value)> {
    let (code, rest) = rest.trim().split_once(char::is_whitespace)?;
    let (name, spec) = rest.split_once(':')?;
    let name = name.trim();
    if name.is_empty() {
        return None;
    }
    let (type_text, description) = trailing_description(spec);
    let type_text = if type_text.is_empty() {
        "String"
    } else {
        type_text
    };
    let mapped = TypeExpr::parse(type_text).map(|t| types.map(&t))?;
    let mut header = json!({ "schema": mapped.schema, "required": mapped.required });
    if let Some(description) = description {
        header["description"] = json!(description);
    }
    Some((code.to_string(), name.to_string(), header))
}

/// A problem details response: `@problem CODE ["description"]`.
pub(super) fn problem(rest: &str) -> Option<Response> {
    let (code, description) = trailing_description(rest);
    if code.is_empty() || code.contains(char::is_whitespace) {
        return None;
    }
    let description = description
        .filter(|d| !d.is_empty())
        .unwrap_or_else(|| reason_phrase(code).to_string());
    Some(Response {
        code: code.to_string(),
        value: json!({
            "description": description,
            "content": { "application/problem+json": {
                "schema": { "$ref": format!("#/components/schemas/{PROBLEM_SCHEMA}") }
            } },
        }),
    })
}

/// The RFC 9457 problem details object.
pub(super) fn problem_schema() -> Value {
    json!({
        "type": "object",
        "description": "Problem details (RFC 9457).",
        "properties": {
            "type": { "type": "string", "format": "uri-reference", "default": "about:blank",
                      "description": "A URI reference that identifies the problem type." },
            "title": { "type": "string", "description": "A short summary of the problem type." },
            "status": { "type": "integer", "format": "int32", "minimum": 100, "maximum": 599,
                        "description": "The HTTP status code." },
            "detail": { "type": "string", "description": "An explanation specific to this occurrence." },
            "instance": { "type": "string", "format": "uri-reference",
                          "description": "A URI reference that identifies this occurrence." },
        },
    })
}

/// Splits trailing media type tokens off a type: `User application/json`.
fn split_media(text: &str) -> (String, Vec<String>) {
    let mut tokens = tokenize(text);
    let mut media = Vec::new();
    while let Some(last) = tokens.last() {
        let candidate = last.trim_end_matches(',');
        if !is_media_type(candidate) {
            break;
        }
        media.push(candidate.to_string());
        tokens.pop();
    }
    media.reverse();
    (tokens.join(" "), media)
}

fn media_or(media: &[String], default: &str) -> Vec<String> {
    if media.is_empty() {
        vec![default.to_string()]
    } else {
        media.to_vec()
    }
}

/// The schema of a DSL type: `$Ref`, `Vec<T>`, `$Page<User>`, `{ inline }`.
pub(super) fn type_schema(text: &str, types: &TypeMapper) -> Value {
    const STD: &[&str] = &[
        "Option<", "Vec<", "Box<", "Arc<", "Rc<", "Cow<", "Json<", "Form<",
    ];
    if text.starts_with('{') {
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

/// The standard reason phrase of a status code (or code range), used when a
/// response has no description (OpenAPI requires one).
pub(super) fn reason_phrase(code: &str) -> &'static str {
    match code {
        "100" => "Continue",
        "101" => "Switching Protocols",
        "200" => "OK",
        "201" => "Created",
        "202" => "Accepted",
        "203" => "Non-Authoritative Information",
        "204" => "No Content",
        "205" => "Reset Content",
        "206" => "Partial Content",
        "207" => "Multi-Status",
        "300" => "Multiple Choices",
        "301" => "Moved Permanently",
        "302" => "Found",
        "303" => "See Other",
        "304" => "Not Modified",
        "307" => "Temporary Redirect",
        "308" => "Permanent Redirect",
        "400" => "Bad Request",
        "401" => "Unauthorized",
        "402" => "Payment Required",
        "403" => "Forbidden",
        "404" => "Not Found",
        "405" => "Method Not Allowed",
        "406" => "Not Acceptable",
        "408" => "Request Timeout",
        "409" => "Conflict",
        "410" => "Gone",
        "411" => "Length Required",
        "412" => "Precondition Failed",
        "413" => "Content Too Large",
        "414" => "URI Too Long",
        "415" => "Unsupported Media Type",
        "416" => "Range Not Satisfiable",
        "417" => "Expectation Failed",
        "421" => "Misdirected Request",
        "422" => "Unprocessable Content",
        "423" => "Locked",
        "424" => "Failed Dependency",
        "425" => "Too Early",
        "426" => "Upgrade Required",
        "428" => "Precondition Required",
        "429" => "Too Many Requests",
        "431" => "Request Header Fields Too Large",
        "451" => "Unavailable For Legal Reasons",
        "500" => "Internal Server Error",
        "501" => "Not Implemented",
        "502" => "Bad Gateway",
        "503" => "Service Unavailable",
        "504" => "Gateway Timeout",
        "505" => "HTTP Version Not Supported",
        "1XX" => "Informational",
        "2XX" => "Success",
        "3XX" => "Redirection",
        "4XX" => "Client Error",
        "5XX" => "Server Error",
        "default" => "Default response",
        _ => "Response",
    }
}
