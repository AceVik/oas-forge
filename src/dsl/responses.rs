//! Request bodies and responses: `@body`, `@return`, `@return-header` and
//! `@problem`.

use super::tokens::{is_media_type, tokenize, trailing_description};
use crate::schema::{MappedType, TypeExpr, TypeMapper, shadow};
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
    /// The Media Type objects by media type.
    pub(super) content: Map<String, Value>,
    /// `false` for `optional` and for `Option<T>` types.
    pub(super) required: bool,
    /// The quoted description, if any.
    pub(super) description: Option<String>,
}

/// Parses a `@body` directive; the error says what is wrong with it.
pub(super) fn body(rest: &str, types: &TypeMapper) -> Result<Body, String> {
    let (rest, description) = trailing_description(rest);
    let (rest, optional) = match rest.strip_prefix("optional") {
        Some(after) if after.starts_with(char::is_whitespace) => (after.trim(), true),
        _ => (rest, false),
    };
    let (type_text, media) = split_media(rest);
    if type_text.is_empty() {
        return Err("`@body` needs a type".into());
    }
    let (schema, required) = mapped_schema(&type_text, types)?;
    let optional = optional || !required;
    let mut content = Map::new();
    for media in media_or(&media, DEFAULT_MEDIA) {
        content.insert(media, json!({ "schema": schema }));
    }
    Ok(Body {
        content,
        required: !optional,
        description,
    })
}

/// A response: `@return CODE: [Type] [media...] ["description"]`.
pub(super) struct Response {
    /// The status code or range (`200`, `4XX`, `default`).
    pub(super) code: String,
    /// The Response object; its description is the reason phrase without
    /// an explicit one.
    pub(super) value: Value,
    /// The description written in the directive, if any.
    pub(super) description: Option<String>,
}

/// Parses a `@return` directive; the error says what is wrong with it.
pub(super) fn response(rest: &str, types: &TypeMapper) -> Result<Response, String> {
    /// The finding for a `@return` line that is not `code: …`.
    const SYNTAX: &str = "`@return` needs `code: [Type] [media...] [\"description\"]`";
    let (code, residue) = rest.split_once(':').ok_or(SYNTAX)?;
    let code = code.trim().to_string();
    if code.is_empty() || code.contains(char::is_whitespace) {
        return Err(SYNTAX.into());
    }
    let (residue, explicit) = trailing_description(residue);
    let explicit = explicit.filter(|d| !d.is_empty());
    let description = explicit
        .clone()
        .unwrap_or_else(|| reason_phrase(&code).to_string());
    let mut value = Map::new();
    value.insert("description".into(), json!(description));
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
            let schema = type_schema(item, types)?;
            for media in media_or(&media, STREAM_MEDIA) {
                content.insert(media, json!({ "itemSchema": schema }));
            }
        } else {
            let entry = if unit {
                json!({})
            } else {
                json!({ "schema": type_schema(type_text, types)? })
            };
            for media in media_or(&media, DEFAULT_MEDIA) {
                content.insert(media, entry.clone());
            }
        }
        value.insert("content".into(), Value::Object(content));
    }
    Ok(Response {
        code,
        value: Value::Object(value),
        description: explicit,
    })
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
    let ty = TypeExpr::parse(type_text)?;
    let mut header = Map::new();
    header.insert("schema".into(), shadow::typed(types, &ty, |m| m.schema));
    header.insert("required".into(), json!(types.map(&ty).required));
    if let Some(description) = description {
        header.insert("description".into(), json!(description));
    }
    Some((code.to_string(), name.to_string(), Value::Object(header)))
}

/// A problem details response: `@problem CODE ["description"]`.
pub(super) fn problem(rest: &str) -> Option<Response> {
    let (code, explicit) = trailing_description(rest);
    if code.is_empty() || code.contains(char::is_whitespace) {
        return None;
    }
    let explicit = explicit.filter(|d| !d.is_empty());
    let description = explicit
        .clone()
        .unwrap_or_else(|| reason_phrase(code).to_string());
    Some(Response {
        code: code.to_string(),
        value: json!({
            "description": description,
            "content": { "application/problem+json": {
                "schema": { "$ref": format!("#/components/schemas/{PROBLEM_SCHEMA}") }
            } },
        }),
        description: explicit,
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

/// The media types written in the directive, or `default` without any.
fn media_or(media: &[String], default: &str) -> Vec<String> {
    if media.is_empty() {
        vec![default.to_string()]
    } else {
        media.to_vec()
    }
}

/// The schema of a DSL type: `$Ref`, `Vec<T>`, `$Page<User>`, `{ inline }`.
/// The error describes text that is neither a type nor valid inline YAML.
pub(super) fn type_schema(text: &str, types: &TypeMapper) -> Result<Value, String> {
    mapped_schema(text, types).map(|(schema, _)| schema)
}

/// [`type_schema`] and whether a value is required (`false` for
/// `Option<T>`).
fn mapped_schema(text: &str, types: &TypeMapper) -> Result<(Value, bool), String> {
    /// Generic types the type table maps itself; other generics
    /// (`Page<User>`) are instances of blueprints.
    const STD: &[&str] = &[
        "Option<", "Vec<", "Box<", "Arc<", "Rc<", "Cow<", "Json<", "Form<",
    ];
    if text.starts_with('{') {
        return yaml::parse(text).map(|schema| (schema, true)).map_err(|e| {
            format!(
                "the inline schema `{text}` is not valid YAML: {}",
                e.message
            )
        });
    }
    if text.contains('<') && !STD.iter().any(|p| text.starts_with(p)) {
        // `Page<User>` and `$Page<User>`: instantiated from the blueprint.
        let reference = format!("${}", text.trim_start_matches('$'));
        return Ok((json!({ "$ref": reference }), true));
    }
    if text.starts_with('$') {
        // Resolved with the other smart references: renamed types are found
        // under their Rust name, unknown names are reported.
        return Ok((json!({ "$ref": text }), true));
    }
    match TypeExpr::parse(text) {
        // A bare built-in name (`Duration`) may stand for a documented schema.
        Some(t) => Ok((
            shadow::typed(types, &t, MappedType::into_schema),
            types.map(&t).required,
        )),
        // A schema name that is no Rust identifier (`api-error`).
        None if is_component_name(text) => {
            let reference = format!("#/components/schemas/{text}");
            Ok((json!({ "$ref": reference }), true))
        }
        None => Err(format!("`{text}` is neither a type nor a schema name")),
    }
}

/// Whether `text` may name a component (`^[a-zA-Z0-9.\-_]+$`).
fn is_component_name(text: &str) -> bool {
    !text.is_empty()
        && text
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | '_'))
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
