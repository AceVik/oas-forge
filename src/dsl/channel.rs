//! The AsyncAPI channel DSL: `@channel` blocks compiled to AsyncAPI 3
//! channels, operations and messages.
//!
//! ```text
//! /// A user signed up.
//! /// @channel user/{id}/signedup
//! /// @send UserSignedUp: $User "The new user"
//! fn publish_signup() {}
//! ```

use super::responses::type_schema;
use super::tokens::trailing_description;
use crate::diagnostics::{Code, Diagnostic, Diagnostics, Location};
use crate::schema::TypeMapper;
use serde_json::{Map, Value, json};

/// One `@channel` block.
#[derive(Debug, Clone, Copy)]
#[non_exhaustive]
pub struct ChannelSource<'a> {
    /// The doc lines (after `@insert`/`@extend` expansion).
    pub lines: &'a [String],
    /// Source line of each entry of `lines`.
    pub line_numbers: &'a [usize],
    /// The operationId (the function name), if any.
    pub operation_id: Option<&'a str>,
    /// Where the block starts.
    pub location: &'a Location,
    /// How Rust types in the block map to schemas.
    pub types: &'a TypeMapper,
}

impl<'a> ChannelSource<'a> {
    /// A block of `lines` starting at `location`, with the built-in type table.
    pub fn new(lines: &'a [String], location: &'a Location) -> Self {
        Self {
            lines,
            line_numbers: &[],
            operation_id: None,
            location,
            types: TypeMapper::builtin(),
        }
    }

    /// Sets the source line of each entry of `lines`.
    pub fn line_numbers(mut self, line_numbers: &'a [usize]) -> Self {
        self.line_numbers = line_numbers;
        self
    }

    /// Sets the operationId (derived from the action and channel when `None`).
    pub fn operation_id(mut self, operation_id: Option<&'a str>) -> Self {
        self.operation_id = operation_id;
        self
    }

    /// Sets the type mapping.
    pub fn types(mut self, types: &'a TypeMapper) -> Self {
        self.types = types;
        self
    }
}

/// A compiled channel block.
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub struct CompiledChannel {
    /// The channel id (key under `channels`).
    pub id: String,
    /// The channel address (`user/{id}/signedup`).
    pub address: String,
    /// The AsyncAPI fragment: `channels`, `operations` and
    /// `components.messages`.
    pub document: Value,
}

/// Whether `lines` contain a `@channel` directive.
pub fn is_channel(lines: &[String]) -> bool {
    lines.iter().any(|line| {
        line.trim()
            .strip_prefix("@channel")
            .is_some_and(|rest| rest.is_empty() || rest.starts_with(char::is_whitespace))
    })
}

/// Compiles a `@channel` block; `None` (with an error) without an address.
pub fn compile_channel(
    source: ChannelSource<'_>,
    diagnostics: &mut Diagnostics,
) -> Option<CompiledChannel> {
    let at = |index: usize| match source.line_numbers.get(index) {
        Some(line) => Location::new(source.location.file(), *line),
        None => source.location.clone(),
    };
    let mut channel: Option<(String, String)> = None;
    // An invalid `@channel` line is reported where it is.
    let mut channel_lines = 0;
    let mut summary: Option<String> = None;
    let mut description: Vec<String> = Vec::new();
    let mut tags: Vec<String> = Vec::new();
    let mut messages: Vec<(&'static str, String, Value)> = Vec::new();

    for (index, line) in source.lines.iter().enumerate() {
        let trimmed = line.trim();
        let Some(directive) = trimmed.strip_prefix('@') else {
            match (&summary, trimmed.is_empty()) {
                (None, false) => summary = Some(trimmed.to_string()),
                (Some(_), _) => description.push(trimmed.to_string()),
                (None, true) => {}
            }
            continue;
        };
        let (name, rest) = directive
            .split_once(char::is_whitespace)
            .map_or((directive, ""), |(n, r)| (n, r.trim()));
        match name {
            "channel" => {
                channel_lines += 1;
                let tokens: Vec<&str> = rest.split_whitespace().collect();
                channel = match tokens.as_slice() {
                    [address] => Some((derived_id(address), (*address).to_string())),
                    [id, address] if is_component_key(id) => {
                        Some(((*id).to_string(), (*address).to_string()))
                    }
                    _ => {
                        diagnostics.push(
                            Diagnostic::new(
                                Code::InvalidRoute,
                                "`@channel` needs `[id] address`; an id may contain letters, digits, `.`, `-` and `_`",
                            )
                            .at(at(index)),
                        );
                        None
                    }
                };
            }
            "send" | "receive" => {
                let action = if name == "send" { "send" } else { "receive" };
                let Some((message, spec)) = rest.split_once(':') else {
                    diagnostics.push(
                        Diagnostic::new(
                            Code::InvalidDirective,
                            format!("`@{name}` needs `Message: Type [\"description\"]`"),
                        )
                        .at(at(index)),
                    );
                    continue;
                };
                let message = message.trim();
                if !is_component_key(message) {
                    diagnostics.push(
                        Diagnostic::new(
                            Code::InvalidDirective,
                            format!("message name `{message}` may only contain letters, digits, `.`, `-` and `_`"),
                        )
                        .at(at(index)),
                    );
                    continue;
                }
                let (type_text, text) = trailing_description(spec);
                let mut object = Map::new();
                object.insert("name".into(), json!(message));
                if !type_text.is_empty() {
                    match type_schema(type_text, source.types) {
                        Ok(payload) => {
                            object.insert("payload".into(), payload);
                        }
                        Err(message) => {
                            diagnostics.push(
                                Diagnostic::new(Code::InvalidDirective, message).at(at(index)),
                            );
                            continue;
                        }
                    }
                }
                if let Some(text) = text {
                    object.insert("description".into(), json!(text));
                }
                messages.push((action, message.to_string(), Value::Object(object)));
            }
            "tag" => tags.extend(
                rest.split(',')
                    .map(|t| t.trim().to_string())
                    .filter(|t| !t.is_empty()),
            ),
            "summary" => summary = Some(rest.to_string()),
            "description" => description.push(rest.to_string()),
            other => diagnostics.push(
                Diagnostic::new(
                    Code::UnknownDirective,
                    format!("unknown directive `@{other}`"),
                )
                .at(at(index)),
            ),
        }
    }

    let Some((id, address)) = channel else {
        if channel_lines == 0 {
            diagnostics.push(
                Diagnostic::new(Code::InvalidRoute, "`@channel` needs an address")
                    .at(source.location.clone()),
            );
        }
        return None;
    };

    let mut channel_object = Map::new();
    channel_object.insert("address".into(), json!(address));
    let parameters: Map<String, Value> = placeholders(&address)
        .into_iter()
        .map(|p| (p, json!({})))
        .collect();
    if !parameters.is_empty() {
        channel_object.insert("parameters".into(), Value::Object(parameters));
    }
    let mut channel_messages = Map::new();
    let mut component_messages = Map::new();
    for (_, name, object) in &messages {
        channel_messages.insert(
            name.clone(),
            json!({ "$ref": format!("#/components/messages/{name}") }),
        );
        component_messages.insert(name.clone(), object.clone());
    }
    if !channel_messages.is_empty() {
        channel_object.insert("messages".into(), Value::Object(channel_messages));
    }

    let mut operations = Map::new();
    let actions: Vec<&str> = ["send", "receive"]
        .into_iter()
        .filter(|action| messages.iter().any(|(a, _, _)| a == action))
        .collect();
    for action in &actions {
        let operation_id = match (source.operation_id, actions.len()) {
            (Some(id), 1) => id.to_string(),
            (Some(id), _) => format!("{id}_{action}"),
            (None, _) => format!("{action}_{id}"),
        };
        let refs: Vec<Value> = messages
            .iter()
            .filter(|(a, _, _)| a == action)
            .map(|(_, name, _)| json!({ "$ref": format!("#/channels/{id}/messages/{name}") }))
            .collect();
        let mut operation = Map::new();
        operation.insert("action".into(), json!(action));
        operation.insert(
            "channel".into(),
            json!({ "$ref": format!("#/channels/{id}") }),
        );
        operation.insert("messages".into(), Value::Array(refs));
        if let Some(summary) = &summary {
            operation.insert("summary".into(), json!(summary));
        }
        if !description.is_empty() {
            operation.insert("description".into(), json!(description.join("\n").trim()));
        }
        if !tags.is_empty() {
            operation.insert(
                "tags".into(),
                Value::Array(tags.iter().map(|t| json!({ "name": t })).collect()),
            );
        }
        operations.insert(operation_id, Value::Object(operation));
    }

    let mut document = Map::new();
    let mut channels = Map::new();
    channels.insert(id.clone(), Value::Object(channel_object));
    document.insert("channels".into(), Value::Object(channels));
    if !operations.is_empty() {
        document.insert("operations".into(), Value::Object(operations));
    }
    if !component_messages.is_empty() {
        document.insert(
            "components".into(),
            json!({ "messages": component_messages }),
        );
    }
    Some(CompiledChannel {
        id,
        address,
        document: Value::Object(document),
    })
}

/// A component key (`^[\w\d.\-_]+$`).
fn is_component_key(text: &str) -> bool {
    !text.is_empty()
        && text
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || "._-".contains(c))
}

/// A channel id from its address: `user/{id}/signedup` → `user_id_signedup`.
fn derived_id(address: &str) -> String {
    let mut id = String::with_capacity(address.len());
    for c in address.chars() {
        if c.is_ascii_alphanumeric() || ".-".contains(c) {
            id.push(c);
        } else if !id.ends_with('_') {
            id.push('_');
        }
    }
    let id = id.trim_matches('_');
    if id.is_empty() {
        "channel".to_string()
    } else {
        id.to_string()
    }
}

fn placeholders(address: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut rest = address;
    while let Some((_, after)) = rest.split_once('{') {
        let Some((name, tail)) = after.split_once('}') else {
            break;
        };
        out.push(name.to_string());
        rest = tail;
    }
    out
}
