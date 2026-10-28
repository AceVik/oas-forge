//! The AsyncAPI channel DSL: `@channel` blocks compiled to AsyncAPI 3
//! channels, operations and messages.
//!
//! ```text
//! /// A user signed up.
//! /// @channel user/{id}/signedup
//! /// @send UserSignedUp: $User "The new user"
//! fn publish_signup() {}
//! ```
//!
//! `@ws-query` and `@ws-header` describe the handshake of a WebSocket
//! channel (its `ws` binding); YAML in the block (`bindings:`, `@openapi
//! message Name`, …) is merged into the channel, a message, the operations
//! or the document.

use super::prose::{self, ImplicitYaml, Prose, Text};
use super::responses::{split_media, type_schema};
use super::tokens::{split_directive, trailing_description};
use super::{SCOPE_SYNTAX, canonical, conflict_line, params};
use crate::diagnostics::{Code, Diagnostic, Diagnostics, Location, Severity};
use crate::docs::{closes_fence, yaml_fence};
use crate::merge::{Policy, merge};
use crate::proto::{self, PROTOBUF_MEDIA_TYPE, ProtoIndex};
use crate::reserved::{self, Keep};
use crate::schema::TypeMapper;
use crate::scope::{self, Names};
use crate::template::lexical;
use crate::yaml;
use serde_json::{Map, Value, json};
use std::collections::{BTreeMap, BTreeSet};

/// Fields of the Channel Object that start an embedded YAML section in a
/// channel block; `address:` overrides the `@channel` line (with a note).
const CHANNEL_KEYS: &[&str] = &[
    "address",
    "title",
    "summary",
    "description",
    "servers",
    "parameters",
    "messages",
    "tags",
    "externalDocs",
    "bindings",
];

/// The version of the AsyncAPI WebSocket channel binding.
const WS_BINDING_VERSION: &str = "0.1.0";

/// Where the lines of an embedded YAML section of a channel block go.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
enum Target {
    /// The channel: `@openapi`, `@openapi channel` or a line that starts
    /// with a channel field (`bindings:`, `x-…:`).
    Channel,
    /// `components.messages.<name>` (`@openapi message Name`).
    Message(String),
    /// Every operation of the block (`@openapi operation`).
    Operation,
    /// The AsyncAPI document (`@openapi document`).
    Document,
    /// Nowhere: the `@openapi` line was invalid (and reported).
    Ignored,
}

/// The `@ws-query` and `@ws-header` parameters of a block: the properties
/// of the binding's `query` and `headers` schemas.
#[derive(Default)]
struct WebSocket {
    /// The `@ws-query` parameters.
    query: Properties,
    /// The `@ws-header` parameters.
    headers: Properties,
}

/// The properties of a handshake schema in declaration order: name, schema
/// and whether each is required. A later declaration of a name replaces
/// the earlier one in its place, found through an index (looking for it in
/// the list took a minute for 100 000 lines).
#[derive(Default)]
struct Properties {
    /// The properties in order.
    list: Vec<(String, Value, bool)>,
    /// The entry in `list` of each name.
    index: BTreeMap<String, usize>,
}

impl Properties {
    /// Adds a property, replacing an earlier one of the same name.
    fn set(&mut self, property: (String, Value, bool)) {
        match self.index.get(&property.0) {
            Some(at) => {
                if let Some(existing) = self.list.get_mut(*at) {
                    *existing = property;
                }
            }
            None => {
                self.index.insert(property.0.clone(), self.list.len());
                self.list.push(property);
            }
        }
    }
}

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
    /// `@route-defaults` blocks of the enclosing modules, outermost first,
    /// each starting with its header; only their `@scope` applies to a
    /// channel.
    pub defaults: &'a [String],
    /// The `.proto` files that `proto(package.Message)` payloads name.
    pub protos: &'a ProtoIndex,
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
            defaults: &[],
            protos: &proto::EMPTY,
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

    /// Sets the `@route-defaults` lines of the enclosing modules; the
    /// `@scope` of the innermost module that has one applies unless the
    /// block has its own.
    ///
    /// # Examples
    ///
    /// ```
    /// use oas_forge::diagnostics::{Diagnostics, Location};
    /// use oas_forge::dsl::{ChannelSource, compile_channel};
    ///
    /// let defaults = ["@route-defaults", "@scope admin"].map(String::from);
    /// let lines = ["@channel jobs", "@send JobDone: { type: object }"].map(String::from);
    /// let location = Location::new("admin.rs", 1);
    /// let channel = compile_channel(
    ///     ChannelSource::new(&lines, &location).defaults(&defaults),
    ///     &mut Diagnostics::new(),
    /// )
    /// .ok_or("the channel is invalid")?;
    /// assert_eq!(channel.scopes, ["admin"]);
    /// # Ok::<(), Box<dyn std::error::Error>>(())
    /// ```
    pub fn defaults(mut self, defaults: &'a [String]) -> Self {
        self.defaults = defaults;
        self
    }

    /// Sets the `.proto` files for `proto(package.Message)` payloads.
    ///
    /// # Examples
    ///
    /// ```
    /// use oas_forge::diagnostics::{Diagnostics, Location};
    /// use oas_forge::dsl::{ChannelSource, compile_channel};
    /// use oas_forge::proto::ProtoIndex;
    /// use oas_forge::source::{SourceFile, SourceKind};
    /// use serde_json::json;
    ///
    /// let text = "syntax = \"proto3\";\npackage chat.v1;\nmessage Hello { string name = 1; }\n";
    /// let mut protos = ProtoIndex::new();
    /// protos.add(&SourceFile::new("chat.proto", SourceKind::Proto, text), &mut Diagnostics::new());
    ///
    /// let lines = ["@channel chat", "@send Hello: proto(chat.v1.Hello)"].map(String::from);
    /// let location = Location::new("chat.rs", 1);
    /// let channel = compile_channel(
    ///     ChannelSource::new(&lines, &location).protos(&protos),
    ///     &mut Diagnostics::new(),
    /// )
    /// .ok_or("the channel is invalid")?;
    /// assert_eq!(
    ///     channel.document["components"]["messages"]["Hello"],
    ///     json!({
    ///         "name": "Hello",
    ///         "contentType": "application/x-protobuf",
    ///         "payload": {
    ///             "schemaFormat": "application/vnd.google.protobuf;version=3",
    ///             "schema": text,
    ///         },
    ///         "x-protobuf-message": "chat.v1.Hello",
    ///     })
    /// );
    /// # Ok::<(), Box<dyn std::error::Error>>(())
    /// ```
    pub fn protos(mut self, protos: &'a ProtoIndex) -> Self {
        self.protos = protos;
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
    /// The scopes of `@scope` (or of the module defaults) the channel
    /// belongs to; empty when it is shared by every scope.
    pub scopes: Vec<String>,
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
    // Whether there is a `@channel` line; an invalid one is reported where
    // it is.
    let mut channel_line = false;
    let mut prose = Prose::default();
    // What plain lines continue.
    let mut text = Text::Free;
    let mut tags: Vec<String> = Vec::new();
    let mut scopes: Option<Names> = None;
    let mut messages = Messages::default();
    let mut websocket = WebSocket::default();
    // Embedded YAML: the open section and every line with its target.
    let mut section: Option<Target> = None;
    let mut yaml: Vec<(Target, usize, String)> = Vec::new();
    // The fence of a fenced YAML section (rendered as code by rustdoc), and
    // whether the line before was `@openapi …`, after which a fence opens
    // one.
    let mut fence: Option<String> = None;
    let mut after_openapi = false;
    // The YAML section a field-name line started, which prose after a blank
    // line ends.
    let mut implicit = ImplicitYaml::default();
    // The `@openapi message Name` lines: the name and the entry.
    let mut message_sections: Vec<(String, usize)> = Vec::new();

    for (index, line) in source.lines.iter().enumerate() {
        let trimmed = line.trim();
        if let Some(open) = &fence {
            if closes_fence(trimmed, open) {
                fence = None;
                section = None;
            } else if let Some(target) = &section {
                yaml.push((target.clone(), index, line.clone()));
            }
            continue;
        }
        let opens_fence = std::mem::take(&mut after_openapi) && section.is_some();
        if let Some(open) = yaml_fence(trimmed).filter(|_| opens_fence) {
            fence = Some(open.to_string());
            continue;
        }
        let Some(directive) = trimmed.strip_prefix('@') else {
            if trimmed.is_empty() {
                implicit.blank();
            } else if implicit.ends(line) {
                // Prose after YAML and a blank line: a new paragraph.
                section = None;
                prose.paragraph(text);
            } else if section.is_none() && text == Text::Free && starts_channel_yaml(trimmed) {
                section = Some(Target::Channel);
                implicit.start(line);
            }
            if let Some(target) = &section {
                yaml.push((target.clone(), index, line.clone()));
                continue;
            }
            prose.line(text, line);
            continue;
        };
        section = None;
        implicit.stop();
        let (name, arguments) = split_directive(directive);
        let rest: &str = &arguments;
        text = Text::after(name);
        after_openapi = name == "openapi";
        match name {
            "channel" => {
                channel_line = true;
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
                let (type_text, media) = split_media(type_text);
                let mut object = Map::new();
                object.insert("name".into(), json!(message));
                if let Some(media) = media.first() {
                    object.insert("contentType".into(), json!(media));
                }
                if let Some(name) = proto_name(&type_text) {
                    let Some(file) = source.protos.message(name) else {
                        diagnostics.push(
                            Diagnostic::new(
                                Code::UnknownProtoMessage,
                                format!("`proto({name})` names no message of the `.proto` inputs (package and message, e.g. `proto(chat.v1.Hello)`); the message is left out"),
                            )
                            .at(at(index)),
                        );
                        continue;
                    };
                    // Each message holds a copy of the file: it counts for the run.
                    if !lexical::charge_run(file.text().len()) {
                        diagnostics.push(
                            Diagnostic::new(
                                Code::ExpansionLimit,
                                format!("the `.proto` file of `proto({name})` exceeds what is left of the 32 MiB for the pasted text of the run; the message is left out"),
                            )
                            .at(at(index)),
                        );
                        continue;
                    }
                    if media.is_empty() {
                        object.insert("contentType".into(), json!(PROTOBUF_MEDIA_TYPE));
                    }
                    object.insert(
                        "payload".into(),
                        json!({ "schemaFormat": file.schema_format(), "schema": file.text() }),
                    );
                    if let Some(text) = text {
                        object.insert("description".into(), json!(text));
                    }
                    object.insert("x-protobuf-message".into(), json!(name));
                    messages.add(
                        (action, message, Value::Object(object)),
                        at(index),
                        diagnostics,
                    );
                    continue;
                }
                if !type_text.is_empty() {
                    match type_schema(&type_text, source.types) {
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
                messages.add(
                    (action, message, Value::Object(object)),
                    at(index),
                    diagnostics,
                );
            }
            "tag" => tags.extend(
                rest.split(',')
                    .map(|t| t.trim().to_string())
                    .filter(|t| !t.is_empty()),
            ),
            "scope" => match scope::names(rest) {
                Some(names) => scopes.get_or_insert_with(Names::default).extend(names),
                None => diagnostics
                    .push(Diagnostic::new(Code::InvalidDirective, SCOPE_SYNTAX).at(at(index))),
            },
            "ws-query" | "ws-header" => {
                if let Some(property) = ws_property(name, rest, &source, index, diagnostics) {
                    let properties = if name == "ws-query" {
                        &mut websocket.query
                    } else {
                        &mut websocket.headers
                    };
                    // A later declaration of a name replaces the earlier one.
                    properties.set(property);
                }
            }
            "openapi" => {
                let target = yaml_target(rest).unwrap_or_else(|message| {
                    diagnostics
                        .push(Diagnostic::new(Code::InvalidDirective, message).at(at(index)));
                    Target::Ignored
                });
                if let Target::Message(name) = &target {
                    message_sections.push((name.clone(), index));
                }
                section = Some(target);
            }
            "summary" => prose.summary(rest),
            "description" => prose.description(prose::margin(line), rest),
            // The plain lines after it are left out.
            "internal" => {}
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
        if !channel_line {
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
    for (_, name, object) in &messages.list {
        channel_messages.insert(
            name.clone(),
            json!({ "$ref": format!("#/components/messages/{name}") }),
        );
        component_messages.insert(name.clone(), object.clone());
    }
    if !channel_messages.is_empty() {
        channel_object.insert("messages".into(), Value::Object(channel_messages));
    }
    if let Some(binding) = websocket.binding() {
        channel_object.insert("bindings".into(), json!({ "ws": binding }));
    }

    let (summary, description) = prose.finish();
    let mut operations = Map::new();
    let actions: Vec<&str> = ["send", "receive"]
        .into_iter()
        .filter(|action| messages.list.iter().any(|(a, _, _)| a == action))
        .collect();
    for action in &actions {
        let operation_id = match (source.operation_id, actions.len()) {
            (Some(id), 1) => id.to_string(),
            (Some(id), _) => format!("{id}_{action}"),
            (None, _) => format!("{action}_{id}"),
        };
        // Each message once, where it is first sent or received.
        let mut refs: Vec<Value> = Vec::new();
        let mut referenced: BTreeSet<&str> = BTreeSet::new();
        for (_, name, _) in messages.list.iter().filter(|(a, _, _)| a == action) {
            if referenced.insert(name) {
                refs.push(json!({ "$ref": format!("#/channels/{id}/messages/{name}") }));
            }
        }
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
        if let Some(description) = &description {
            operation.insert("description".into(), json!(description));
        }
        if !tags.is_empty() {
            operation.insert(
                "tags".into(),
                Value::Array(tags.iter().map(|t| json!({ "name": t })).collect()),
            );
        }
        operations.insert(operation_id, Value::Object(operation));
    }

    let mut channel_object = Value::Object(channel_object);
    let mut block_prose = Map::new();
    for (key, value) in [("summary", &summary), ("description", &description)] {
        if let Some(value) = value {
            block_prose.insert(key.into(), json!(value));
        }
    }
    let embedded = Embedded::new(&source, &yaml, block_prose);
    embedded.merge_into(&Target::Channel, &mut channel_object, diagnostics);
    // YAML for a message the block does not send or receive is ignored
    // (`component_messages` has every message the block declares).
    for (name, index) in &message_sections {
        if !component_messages.contains_key(name) {
            diagnostics.push(
                Diagnostic::new(
                    Code::InvalidDirective,
                    format!("`@openapi message {name}` names no `@send` or `@receive` message of this block; its YAML is ignored"),
                )
                .at(at(*index)),
            );
        }
    }
    for name in embedded.messages() {
        let Some(mut message) = component_messages.get(&name).cloned() else {
            continue;
        };
        embedded.merge_into(&Target::Message(name.clone()), &mut message, diagnostics);
        // YAML that was ignored adds no empty message.
        if message.as_object().is_some_and(|m| !m.is_empty()) {
            component_messages.insert(name, message);
        }
    }
    if operations.is_empty() {
        embedded.without_operation(diagnostics);
    }
    for operation in operations.values_mut() {
        embedded.merge_into(&Target::Operation, operation, diagnostics);
    }

    let mut document = Map::new();
    let mut channels = Map::new();
    channels.insert(id.clone(), channel_object);
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
    let mut document = Value::Object(document);
    embedded.merge_into(&Target::Document, &mut document, diagnostics);
    let scopes = match scopes {
        Some(scopes) => scopes.into_vec(),
        None => default_scopes(source, diagnostics),
    };
    Some(CompiledChannel {
        id,
        address,
        scopes,
        document,
    })
}

/// Moves the messages that the compiled channel blocks `channels` define
/// differently under one name from `components.messages` into each
/// channel (`channels.<id>.messages.<Name>`), where the channel's
/// operations refer to them already. A name that every block defines
/// alike stays a shared component.
pub(crate) fn channel_messages(channels: &mut [&mut Value]) {
    // The JSON texts of the definitions of each name: a set, as comparing
    // each with all before took minutes for thousands of blocks.
    let mut definitions: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
    for document in channels.iter() {
        if let Some(Value::Object(messages)) = document.pointer("/components/messages") {
            for (name, message) in messages {
                definitions
                    .entry(name.clone())
                    .or_default()
                    .insert(message.to_string());
            }
        }
    }
    let differing: Vec<String> = definitions
        .into_iter()
        .filter(|(_, seen)| seen.len() > 1)
        .map(|(name, _)| name)
        .collect();
    if differing.is_empty() {
        return;
    }
    for document in channels.iter_mut() {
        let Some(Value::Object(components)) = document.get_mut("components") else {
            continue;
        };
        let Some(Value::Object(messages)) = components.get_mut("messages") else {
            continue;
        };
        let moved: Vec<(String, Value)> = differing
            .iter()
            .filter_map(|name| Some((name.clone(), messages.shift_remove(name)?)))
            .collect();
        if messages.is_empty() {
            components.shift_remove("messages");
        }
        if components.is_empty()
            && let Some(map) = document.as_object_mut()
        {
            map.shift_remove("components");
        }
        let Some(Value::Object(channels)) = document.get_mut("channels") else {
            continue;
        };
        for channel in channels.values_mut() {
            let Some(Value::Object(references)) = channel.get_mut("messages") else {
                continue;
            };
            for (name, message) in &moved {
                let shared = json!({ "$ref": format!("#/components/messages/{name}") });
                if let Some(entry) = references.get_mut(name)
                    && *entry == shared
                {
                    *entry = message.clone();
                }
            }
        }
    }
}

/// The messages of `@send` and `@receive` lines of a block in order.
#[derive(Default)]
struct Messages {
    /// Each message: action, name and Message Object.
    list: Vec<(&'static str, String, Value)>,
    /// The distinct contents (JSON texts with sorted keys) defined so far
    /// for each name: comparing a message with every earlier one of its
    /// name took minutes for 20 000 `@send` lines.
    contents: BTreeMap<String, BTreeSet<String>>,
}

impl Messages {
    /// Adds a message (action, name, Message Object) at `location`. The
    /// same name with other content is reported: the channel has one
    /// message per name, and the later definition is used.
    fn add(
        &mut self,
        (action, name, object): (&'static str, &str, Value),
        location: Location,
        diagnostics: &mut Diagnostics,
    ) {
        let seen = self.contents.entry(name.to_string()).or_default();
        let text = canonical(&object);
        // An earlier definition with other content: another one, or several.
        let differs = !seen.is_empty() && (seen.len() > 1 || !seen.contains(&text));
        seen.insert(text);
        if differs {
            diagnostics.push(
                Diagnostic::new(
                    Code::MergeConflict,
                    format!("message `{name}` is defined again in this channel with other content; the later definition is used (give the messages distinct names)"),
                )
                .at(location),
            );
        }
        self.list.push((action, name.to_string(), object));
    }
}

/// The message name of a `proto(package.Message)` type, `None` for other
/// types.
fn proto_name(type_text: &str) -> Option<&str> {
    type_text
        .strip_prefix("proto(")
        .and_then(|rest| rest.strip_suffix(')'))
        .map(str::trim)
}

/// Whether a prose line of a channel block starts embedded YAML: a channel
/// field (`bindings:`) or an extension (`x-…:`) followed by `:`.
fn starts_channel_yaml(trimmed: &str) -> bool {
    trimmed
        .split_once(':')
        .map(|(key, _)| key.trim())
        .is_some_and(|key| {
            !key.contains(char::is_whitespace)
                && (CHANNEL_KEYS.contains(&key) || key.starts_with("x-"))
        })
}

/// The target of `@openapi [channel | operation | document | message Name]`;
/// the error says what is wrong.
fn yaml_target(rest: &str) -> Result<Target, String> {
    let mut words = rest.split_whitespace();
    let target = match (words.next(), words.next()) {
        (None | Some("channel"), None) => Target::Channel,
        (Some("operation"), None) => Target::Operation,
        (Some("document"), None) => Target::Document,
        (Some("message"), Some(name)) if is_component_key(name) => {
            Target::Message(name.to_string())
        }
        _ => {
            return Err(format!(
                "`@openapi {rest}` is no target of a channel block (use channel, operation, document or message Name); the YAML is ignored"
            ));
        }
    };
    match words.next() {
        None => Ok(target),
        Some(_) => Err(format!(
            "`@openapi {rest}` is no target of a channel block (use channel, operation, document or message Name); the YAML is ignored"
        )),
    }
}

/// Parses `@ws-query`/`@ws-header` (`directive`) `rest` like a query
/// parameter (headers parse alike; the location is dropped); returns the
/// property name, its schema and whether it is required. Flags that only
/// parameters have are reported and ignored.
fn ws_property(
    directive: &str,
    rest: &str,
    source: &ChannelSource<'_>,
    index: usize,
    diagnostics: &mut Diagnostics,
) -> Option<(String, Value, bool)> {
    let at = match source.line_numbers.get(index) {
        Some(line) => Location::new(source.location.file(), *line),
        None => source.location.clone(),
    };
    let Some(parameter) = params::parse("query", rest, source.types) else {
        diagnostics.push(
            Diagnostic::new(
                Code::InvalidDirective,
                format!("`@{directive}` needs `name: [Type] [flags] [\"description\"]`"),
            )
            .at(at),
        );
        return None;
    };
    for problem in &parameter.problems {
        diagnostics.push(Diagnostic::new(Code::InvalidDirective, problem.clone()).at(at.clone()));
    }
    let Value::Object(mut fields) = parameter.value else {
        return None;
    };
    let required = fields.shift_remove("required") == Some(json!(true));
    let mut schema = fields.shift_remove("schema").unwrap_or_else(|| json!({}));
    if let Value::Object(schema) = &mut schema {
        if let Some(description) = fields.shift_remove("description") {
            schema.insert("description".into(), description);
        }
        if let Some(deprecated) = fields.shift_remove("deprecated") {
            schema.insert("deprecated".into(), deprecated);
        }
        if let Some(example) = fields.shift_remove("example") {
            schema.insert("examples".into(), json!([example]));
        }
    }
    for flag in fields
        .keys()
        .filter(|key| !matches!(key.as_str(), "name" | "in"))
    {
        diagnostics.push(
            Diagnostic::new(
                Code::InvalidDirective,
                format!("`{flag}` does not apply to `@{directive}` and was ignored"),
            )
            .at(at.clone()),
        );
    }
    Some((parameter.name, schema, required))
}

impl WebSocket {
    /// The `ws` channel binding, `None` without `@ws-query` and
    /// `@ws-header`.
    fn binding(&self) -> Option<Value> {
        if self.query.list.is_empty() && self.headers.list.is_empty() {
            return None;
        }
        let mut binding = Map::new();
        for (key, properties) in [("query", &self.query), ("headers", &self.headers)] {
            if !properties.list.is_empty() {
                binding.insert(key.into(), object_schema(&properties.list));
            }
        }
        binding.insert("bindingVersion".into(), json!(WS_BINDING_VERSION));
        Some(Value::Object(binding))
    }
}

/// An object schema with `properties`, and `required` when a property is.
fn object_schema(properties: &[(String, Value, bool)]) -> Value {
    let mut schema = Map::new();
    schema.insert("type".into(), json!("object"));
    schema.insert(
        "properties".into(),
        Value::Object(
            properties
                .iter()
                .map(|(name, schema, _)| (name.clone(), schema.clone()))
                .collect(),
        ),
    );
    let required: Vec<&String> = properties
        .iter()
        .filter(|(_, _, required)| *required)
        .map(|(name, _, _)| name)
        .collect();
    if !required.is_empty() {
        schema.insert("required".into(), json!(required));
    }
    Value::Object(schema)
}

/// The embedded YAML of a channel block.
struct Embedded<'s, 'a> {
    /// The block, for locations.
    source: &'s ChannelSource<'a>,
    /// The YAML lines of each target, in block order: each line's entry in
    /// the block and its text. Grouped once: filtering all lines for each
    /// of 40 000 messages took over a minute.
    lines: BTreeMap<&'s Target, Vec<(usize, &'s str)>>,
    /// The summary and description the directives and plain lines set for
    /// the block (they go to its operations): channel YAML that sets
    /// another value is noted like a changed DSL value.
    prose: Map<String, Value>,
}

impl<'s, 'a> Embedded<'s, 'a> {
    /// The embedded YAML `lines` of the block `source` (each line's target,
    /// entry in the block and text), with the `prose` of the block.
    fn new(
        source: &'s ChannelSource<'a>,
        lines: &'s [(Target, usize, String)],
        prose: Map<String, Value>,
    ) -> Self {
        let mut grouped: BTreeMap<&'s Target, Vec<(usize, &'s str)>> = BTreeMap::new();
        for (target, index, line) in lines {
            grouped
                .entry(target)
                .or_default()
                .push((*index, line.as_str()));
        }
        Self {
            source,
            lines: grouped,
            prose,
        }
    }

    /// The source location of entry `index` of the block.
    fn at(&self, index: usize) -> Location {
        match self.source.line_numbers.get(index) {
            Some(line) => Location::new(self.source.location.file(), *line),
            None => self.source.location.clone(),
        }
    }

    /// The names of the `@openapi message Name` sections, sorted.
    fn messages(&self) -> Vec<String> {
        self.lines
            .keys()
            .filter_map(|target| match target {
                Target::Message(name) => Some(name.clone()),
                _ => None,
            })
            .collect()
    }

    /// Reports `@openapi operation` YAML in a block without operation.
    fn without_operation(&self, diagnostics: &mut Diagnostics) {
        if let Some((index, _)) = self
            .lines
            .get(&Target::Operation)
            .into_iter()
            .flatten()
            .find(|(_, line)| !line.trim().is_empty())
        {
            diagnostics.push(
                Diagnostic::new(
                    Code::InvalidRouteYaml,
                    "`@openapi operation` needs a `@send` or `@receive` in the block; the YAML is ignored",
                )
                .at(self.at(*index)),
            );
        }
    }

    /// Merges the YAML of `target` into `value`: the YAML wins, and a value
    /// it changes is reported as a note at its line. YAML that is invalid
    /// or not a mapping is reported and ignored.
    fn merge_into(&self, target: &Target, value: &mut Value, diagnostics: &mut Diagnostics) {
        let Some(lines) = self.lines.get(target) else {
            return;
        };
        let text: Vec<&str> = lines.iter().map(|(_, line)| *line).collect();
        if text.iter().all(|line| line.trim().is_empty()) {
            return;
        }
        let first = lines.first().map(|(index, _)| *index).unwrap_or_default();
        match yaml::parse(&text.join("\n")) {
            Ok(Value::Null) => {}
            Ok(parsed) if !parsed.is_object() => diagnostics.push(
                Diagnostic::new(
                    Code::InvalidRouteYaml,
                    "embedded AsyncAPI YAML must be a mapping and was ignored",
                )
                .at(self.at(first)),
            ),
            Ok(mut parsed) => {
                // A reserved key is reported at the line that writes it.
                for removed in reserved::strip(&mut parsed, Keep::Markers) {
                    let index = reserved::line_of(&text.join("\n"), &removed.key)
                        .and_then(|i| lines.get(i))
                        .map_or(first, |(index, _)| *index);
                    diagnostics.push(removed.finding().at(self.at(index)));
                }
                // The channel's summary and description are not the
                // block's, but YAML that sets other ones changes what the
                // block says.
                let mut pointers: Vec<String> = Vec::new();
                if *target == Target::Channel {
                    for (key, dsl) in &self.prose {
                        if parsed.get(key).is_some_and(|yaml| yaml != dsl) {
                            pointers.push(format!("/{key}"));
                        }
                    }
                }
                pointers.extend(merge(value, parsed, Policy::Document));
                let mut found: Vec<(usize, String)> = pointers
                    .into_iter()
                    .map(|pointer| {
                        let index = conflict_line(&text, &pointer)
                            .and_then(|i| lines.get(i))
                            .map_or(first, |(index, _)| *index);
                        (index, pointer)
                    })
                    .collect();
                found.sort();
                for (index, pointer) in found {
                    diagnostics.push(
                        Diagnostic::new(
                            Code::MergeConflict,
                            format!("embedded YAML overrides the DSL value at `{pointer}`"),
                        )
                        .with_severity(Severity::Note)
                        .at(self.at(index)),
                    );
                }
            }
            Err(error) => {
                let index = lines
                    .get(error.line.saturating_sub(1))
                    .map_or(first, |(index, _)| *index);
                diagnostics.push(
                    Diagnostic::new(
                        Code::InvalidRouteYaml,
                        format!(
                            "embedded AsyncAPI YAML is invalid and was ignored: {}",
                            error.message
                        ),
                    )
                    .at(self.at(index)),
                );
            }
        }
    }
}

/// The `@scope` of the innermost module defaults that have one; an invalid
/// `@scope` line is reported at the block.
fn default_scopes(source: ChannelSource<'_>, diagnostics: &mut Diagnostics) -> Vec<String> {
    let mut scopes: Option<Names> = None;
    let mut outer = false;
    for line in source.defaults {
        if line.trim() == "@route-defaults" {
            outer = scopes.is_some();
            continue;
        }
        let Some(rest) = scope::directive(line) else {
            continue;
        };
        match scope::names(rest) {
            Some(names) => {
                if std::mem::take(&mut outer) {
                    scopes = None;
                }
                scopes.get_or_insert_with(Names::default).extend(names);
            }
            None => diagnostics.push(
                Diagnostic::new(Code::InvalidDirective, SCOPE_SYNTAX).at(source.location.clone()),
            ),
        }
    }
    scopes.map(Names::into_vec).unwrap_or_default()
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

/// The names of the `{placeholders}` of a channel address, in order.
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
