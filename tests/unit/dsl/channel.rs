//! `src/dsl/channel.rs`: the AsyncAPI channel DSL.

use crate::common::{codes, here, lines};
use oas_forge::diagnostics::{Code, Diagnostics, Severity};
use oas_forge::dsl::{ChannelSource, compile_channel, is_channel};
use serde_json::{Value, json};

fn compile(src: &[&str], operation_id: Option<&str>) -> (Option<Value>, Diagnostics) {
    let lines = lines(src);
    let location = here();
    let mut diagnostics = Diagnostics::new();
    let compiled = compile_channel(
        ChannelSource::new(&lines, &location).operation_id(operation_id),
        &mut diagnostics,
    );
    (compiled.map(|c| c.document), diagnostics)
}

#[test]
fn channel_blocks_are_recognized() {
    assert!(is_channel(&lines(&["Docs", "  @channel a/b"])));
    assert!(!is_channel(&lines(&[
        "@channels are nice",
        "@route GET /x"
    ])));
}

#[test]
fn a_channel_with_one_action() {
    let (document, diagnostics) = compile(
        &[
            "Signed up.",
            "Details.",
            "@channel user/{id}/signedup",
            "@send SignedUp: $User \"New user\"",
            "@tag Users, Audit",
        ],
        Some("publish"),
    );
    assert!(diagnostics.is_empty(), "{diagnostics:?}");
    let document = document.unwrap();
    assert_eq!(
        document["channels"]["user_id_signedup"],
        json!({
            "address": "user/{id}/signedup",
            "parameters": { "id": {} },
            "messages": { "SignedUp": { "$ref": "#/components/messages/SignedUp" } },
        })
    );
    assert_eq!(
        document["operations"]["publish"],
        json!({
            "action": "send",
            "channel": { "$ref": "#/channels/user_id_signedup" },
            "messages": [{ "$ref": "#/channels/user_id_signedup/messages/SignedUp" }],
            "summary": "Signed up.",
            "description": "Details.",
            "tags": [{ "name": "Users" }, { "name": "Audit" }],
        })
    );
    assert_eq!(
        document["components"]["messages"]["SignedUp"],
        json!({ "name": "SignedUp", "payload": { "$ref": "$User" }, "description": "New user" })
    );
}

#[test]
fn both_actions_get_their_own_operation() {
    let (document, _) = compile(
        &[
            "@channel jobs queue/jobs",
            "@receive Job: String",
            "@send Done:",
            "@summary Jobs",
        ],
        Some("worker"),
    );
    let document = document.unwrap();
    let operations: Vec<_> = document["operations"]
        .as_object()
        .unwrap()
        .keys()
        .cloned()
        .collect();
    assert_eq!(operations, ["worker_send", "worker_receive"]);
    assert!(
        document["components"]["messages"]["Done"]
            .get("payload")
            .is_none()
    );
    assert_eq!(document["operations"]["worker_send"]["summary"], "Jobs");

    let (document, _) = compile(
        &["@channel ping", "@description Pings.", "@send Ping: u8"],
        None,
    );
    assert!(document.unwrap()["operations"]["send_ping"]["description"] == "Pings.");
}

#[test]
fn channels_without_messages_have_no_operation() {
    let (document, _) = compile(&["@channel ///weird///"], None);
    let document = document.unwrap();
    assert!(document.get("operations").is_none());
    assert!(document["channels"].get("weird").is_some(), "{document}");
    let (document, _) = compile(&["@channel ///"], None);
    assert!(document.unwrap()["channels"].get("channel").is_some());
}

#[test]
fn problems_are_reported() {
    let (document, diagnostics) = compile(&["@channel"], None);
    assert!(document.is_none());
    assert_eq!(codes(&diagnostics), [Code::InvalidRoute]);
    let (document, diagnostics) = compile(&["Just text"], None);
    assert!(document.is_none());
    assert_eq!(codes(&diagnostics), [Code::InvalidRoute]);
    let (_, diagnostics) = compile(&["@channel bad/id a/b"], None);
    assert_eq!(codes(&diagnostics), [Code::InvalidRoute]);
    let (_, diagnostics) = compile(
        &[
            "@channel a",
            "@send NoColon",
            "@send bad name: u8",
            "@frobnicate",
        ],
        None,
    );
    assert_eq!(
        codes(&diagnostics),
        [
            Code::InvalidDirective,
            Code::InvalidDirective,
            Code::UnknownDirective
        ]
    );
}

#[test]
fn a_payload_that_is_no_type_is_reported_and_skipped() {
    let (document, diagnostics) = compile(
        &[
            "@channel orders",
            "@send Broken: { type: object",
            "@send Fine: $Order",
        ],
        Some("publish"),
    );
    assert_eq!(codes(&diagnostics), [Code::InvalidDirective]);
    let document = document.unwrap();
    assert!(document["components"]["messages"].get("Broken").is_none());
    assert!(document["components"]["messages"]["Fine"].is_object());
}

#[test]
fn a_block_without_channel_line_is_rejected_once() {
    let (document, diagnostics) = compile(&["@send A: $A"], None);
    assert!(document.is_none());
    assert_eq!(codes(&diagnostics), [Code::InvalidRoute]);
    let (document, diagnostics) = compile(&["@channel"], None);
    assert!(document.is_none());
    assert_eq!(codes(&diagnostics), [Code::InvalidRoute]);
}

mod scopes {
    use super::*;
    use oas_forge::dsl::CompiledChannel;

    fn compiled(src: &[&str], defaults: &[&str]) -> (Option<CompiledChannel>, Diagnostics) {
        let lines = lines(src);
        let defaults = lines_of(defaults);
        let location = here();
        let mut diagnostics = Diagnostics::new();
        let compiled = compile_channel(
            ChannelSource::new(&lines, &location)
                .line_numbers(&[1, 2, 3])
                .defaults(&defaults),
            &mut diagnostics,
        );
        (compiled, diagnostics)
    }

    fn lines_of(src: &[&str]) -> Vec<String> {
        src.iter().map(|l| l.to_string()).collect()
    }

    fn scopes(src: &[&str], defaults: &[&str]) -> Vec<String> {
        let (channel, diagnostics) = compiled(src, defaults);
        assert!(diagnostics.is_empty(), "{diagnostics:#?}");
        channel.expect("valid channel").scopes
    }

    #[test]
    fn a_channel_names_its_scopes() {
        assert!(scopes(&["@channel a"], &[]).is_empty());
        assert_eq!(
            scopes(&["@channel a", "@scope admin, ops", "@scope admin"], &[]),
            ["admin", "ops"]
        );
        let (channel, _) = compiled(&["@channel a", "@scope admin"], &[]);
        let document = channel.expect("valid channel").document;
        assert_eq!(document["channels"]["a"], json!({ "address": "a" }));
    }

    #[test]
    fn the_innermost_module_scope_applies_unless_the_block_has_one() {
        let defaults = [
            "@route-defaults",
            "@scope public",
            "@tag Outer",
            "@route-defaults",
            "@scope admin",
            "@scope ops",
        ];
        assert_eq!(scopes(&["@channel a"], &defaults), ["admin", "ops"]);
        assert_eq!(scopes(&["@channel a"], &defaults[..3]), ["public"]);
        let inner_without = ["@route-defaults", "@scope public", "@route-defaults"];
        assert_eq!(scopes(&["@channel a"], &inner_without), ["public"]);
        assert_eq!(
            scopes(&["@channel a", "@scope internal"], &defaults),
            ["internal"]
        );
    }

    #[test]
    fn invalid_scope_lines_are_reported() {
        let (channel, diagnostics) = compiled(&["@channel a", "@scope a b"], &[]);
        assert!(channel.expect("valid channel").scopes.is_empty());
        assert_eq!(codes(&diagnostics), [Code::InvalidDirective]);
        assert_eq!(
            diagnostics
                .iter()
                .next()
                .and_then(|d| d.location())
                .map(|l| l.line()),
            Some(2)
        );

        let (channel, diagnostics) =
            compiled(&["@channel a"], &["@route-defaults", "@scope", "@scope ok"]);
        assert_eq!(channel.expect("valid channel").scopes, ["ok"]);
        assert_eq!(codes(&diagnostics), [Code::InvalidDirective]);
        assert_eq!(
            diagnostics.iter().next().and_then(|d| d.location()),
            Some(&here())
        );
    }
}

/// Compiles `src` with source line numbers 1, 2, … and returns the document
/// and the findings.
fn numbered(src: &[&str]) -> (Option<Value>, Diagnostics) {
    let lines = lines(src);
    let numbers: Vec<usize> = (1..=src.len()).collect();
    let location = here();
    let mut diagnostics = Diagnostics::new();
    let compiled = compile_channel(
        ChannelSource::new(&lines, &location)
            .line_numbers(&numbers)
            .operation_id(Some("op")),
        &mut diagnostics,
    );
    (compiled.map(|c| c.document), diagnostics)
}

/// The findings as `line: code message`.
fn findings(diagnostics: &Diagnostics) -> Vec<String> {
    diagnostics
        .iter()
        .map(|d| {
            let line = d.location().map_or(0, |l| l.line());
            format!("{line}: {} {}", d.code(), d.message())
        })
        .collect()
}

mod websocket {
    use super::*;

    #[test]
    fn query_and_header_parameters_form_the_ws_binding() {
        let (document, diagnostics) = numbered(&[
            "@channel live",
            r#"@ws-query token: String "Session token""#,
            "@ws-query since: Option<u64> example=5",
            "@ws-query room: String default=lobby enum=[lobby, ops]",
            r#"@ws-header X-Client: String deprecated "Client name""#,
            "@send Tick: u64",
        ]);
        assert!(diagnostics.is_empty(), "{diagnostics:#?}");
        assert_eq!(
            document.unwrap()["channels"]["live"],
            json!({
                "address": "live",
                "messages": { "Tick": { "$ref": "#/components/messages/Tick" } },
                "bindings": {
                    "ws": {
                        "query": {
                            "type": "object",
                            "properties": {
                                "token": { "type": "string", "description": "Session token" },
                                "since": {
                                    "type": "integer",
                                    "format": "int64",
                                    "minimum": 0,
                                    "examples": [5],
                                },
                                "room": { "type": "string", "default": "lobby", "enum": ["lobby", "ops"] },
                            },
                            "required": ["token"],
                        },
                        "headers": {
                            "type": "object",
                            "properties": {
                                "X-Client": {
                                    "type": "string",
                                    "description": "Client name",
                                    "deprecated": true,
                                },
                            },
                            "required": ["X-Client"],
                        },
                        "bindingVersion": "0.1.0",
                    }
                },
            })
        );
    }

    #[test]
    fn headers_alone_and_optional_parameters() {
        let (document, _) = numbered(&["@channel a", "@ws-header X-Trace: Option<String>"]);
        assert_eq!(
            document.unwrap()["channels"]["a"]["bindings"],
            json!({ "ws": {
                "headers": { "type": "object", "properties": { "X-Trace": { "type": "string" } } },
                "bindingVersion": "0.1.0",
            } })
        );
    }

    #[test]
    fn a_later_declaration_of_a_name_replaces_the_earlier_one() {
        let (document, _) = numbered(&[
            "@channel a",
            "@ws-query token: u8",
            "@ws-query other: Option<u8>",
            "@ws-query token: Option<String>",
        ]);
        assert_eq!(
            document.unwrap()["channels"]["a"]["bindings"]["ws"]["query"],
            json!({
                "type": "object",
                "properties": { "token": { "type": "string" }, "other": { "type": "integer", "format": "int32", "minimum": 0, "maximum": 255 } },
            })
        );
    }

    #[test]
    fn parameter_only_and_unknown_flags_are_reported_and_ignored() {
        let (document, diagnostics) = numbered(&[
            "@channel a",
            "@ws-query q: String explode style=form allow-reserved allow-empty bogus=1",
        ]);
        assert_eq!(
            findings(&diagnostics),
            [
                "2: OF2004 `bogus=1` is not a parameter flag and was ignored",
                "2: OF2004 `explode` does not apply to `@ws-query` and was ignored",
                "2: OF2004 `style` does not apply to `@ws-query` and was ignored",
                "2: OF2004 `allowReserved` does not apply to `@ws-query` and was ignored",
                "2: OF2004 `allowEmptyValue` does not apply to `@ws-query` and was ignored",
            ]
        );
        assert_eq!(
            document.unwrap()["channels"]["a"]["bindings"]["ws"]["query"]["properties"],
            json!({ "q": { "type": "string" } })
        );
    }

    #[test]
    fn malformed_parameters_are_reported_and_skipped() {
        let (document, diagnostics) =
            numbered(&["@channel a", "@ws-header token", "@ws-query : u8"]);
        assert_eq!(
            findings(&diagnostics),
            [
                "2: OF2004 `@ws-header` needs `name: [Type] [flags] [\"description\"]`",
                "3: OF2004 `@ws-query` needs `name: [Type] [flags] [\"description\"]`",
            ]
        );
        assert!(document.unwrap()["channels"]["a"].get("bindings").is_none());
    }
}

mod media_types {
    use super::*;

    #[test]
    fn the_first_media_type_is_the_content_type() {
        let (document, diagnostics) = numbered(&[
            "@channel a",
            r#"@send Frame: $Frame application/x-protobuf application/octet-stream "A frame""#,
            "@receive Ping: text/plain",
            "@receive Pong: { type: string }",
        ]);
        assert!(diagnostics.is_empty(), "{diagnostics:#?}");
        let messages = &document.unwrap()["components"]["messages"];
        assert_eq!(
            messages["Frame"],
            json!({
                "name": "Frame",
                "contentType": "application/x-protobuf",
                "payload": { "$ref": "$Frame" },
                "description": "A frame",
            })
        );
        assert_eq!(
            messages["Ping"],
            json!({ "name": "Ping", "contentType": "text/plain" })
        );
        assert_eq!(
            messages["Pong"],
            json!({ "name": "Pong", "payload": { "type": "string" } })
        );
    }
}

mod embedded_yaml {
    use super::*;

    #[test]
    fn yaml_goes_to_the_channel_a_message_the_operations_or_the_document() {
        let (document, diagnostics) = numbered(&[
            "Live updates",
            "@channel live",
            "@ws-query token: String",
            "@send Tick: u64",
            "@receive Hello: { type: object }",
            "bindings:",
            "  ws:",
            "    method: GET",
            "",
            "x-internal: true",
            "@openapi message Tick",
            "title: A tick",
            "headers: { type: object }",
            "@openapi message Hello",
            "summary: A greeting",
            "@openapi operation",
            "security: [{ $ref: '#/components/securitySchemes/token' }]",
            "@openapi document",
            "components:",
            "  securitySchemes:",
            "    token: { type: httpApiKey, in: query, name: token }",
            "@openapi channel",
            "description: Every update",
            "@openapi",
            "title: Live",
        ]);
        assert!(diagnostics.is_empty(), "{diagnostics:#?}");
        let document = document.unwrap();
        let channel = &document["channels"]["live"];
        assert_eq!(channel["bindings"]["ws"]["method"], "GET");
        assert_eq!(channel["bindings"]["ws"]["bindingVersion"], "0.1.0");
        assert!(channel["bindings"]["ws"]["query"].is_object());
        assert_eq!(channel["x-internal"], true);
        assert_eq!(channel["description"], "Every update");
        assert_eq!(channel["title"], "Live");
        let messages = &document["components"]["messages"];
        assert_eq!(
            messages["Tick"],
            json!({
                "name": "Tick",
                "payload": { "type": "integer", "format": "int64", "minimum": 0 },
                "title": "A tick",
                "headers": { "type": "object" },
            })
        );
        assert_eq!(messages["Hello"]["summary"], "A greeting");
        for operation in ["op_send", "op_receive"] {
            assert_eq!(
                document["operations"][operation]["security"],
                json!([{ "$ref": "#/components/securitySchemes/token" }]),
                "{operation}"
            );
            assert_eq!(document["operations"][operation]["summary"], "Live updates");
        }
        assert_eq!(
            document["components"]["securitySchemes"]["token"]["type"],
            "httpApiKey"
        );
    }

    #[test]
    fn yaml_wins_with_a_note_at_its_line() {
        let (document, diagnostics) = numbered(&[
            "@channel live",
            "@ws-query token: String",
            "bindings:",
            "  ws:",
            "    bindingVersion: 0.2.0",
            "@openapi",
            "address: other/live",
            "@openapi document",
            "channels: { live: { title: Doc } }",
        ]);
        assert_eq!(
            findings(&diagnostics),
            [
                "5: OF4001 embedded YAML overrides the DSL value at `/bindings/ws/bindingVersion`",
                "7: OF4001 embedded YAML overrides the DSL value at `/address`",
            ]
        );
        let notes = diagnostics
            .iter()
            .filter(|d| d.severity() == Severity::Note);
        assert_eq!(notes.count(), 2);
        let document = document.unwrap();
        assert_eq!(document["channels"]["live"]["address"], "other/live");
        assert_eq!(document["channels"]["live"]["title"], "Doc");
        assert_eq!(
            document["channels"]["live"]["bindings"]["ws"]["bindingVersion"],
            "0.2.0"
        );
    }

    #[test]
    fn document_yaml_that_changes_the_dsl_is_noted_at_its_line() {
        let (document, diagnostics) = numbered(&[
            "@channel live",
            "@send A: u8",
            "@openapi document",
            "channels:",
            "  live:",
            "    address: elsewhere",
            "    messages: { B: { $ref: '#/components/messages/B' } }",
        ]);
        assert_eq!(
            findings(&diagnostics),
            ["6: OF4001 embedded YAML overrides the DSL value at `/channels/live/address`"]
        );
        let channel = &document.unwrap()["channels"]["live"];
        assert_eq!(channel["address"], "elsewhere");
        assert_eq!(
            channel["messages"].as_object().map(|m| m.len()),
            Some(2),
            "maps merge"
        );
    }

    #[test]
    fn invalid_yaml_is_reported_and_ignored() {
        let (document, diagnostics) = numbered(&[
            "@channel live",
            "@send Tick: u8",
            "bindings:",
            "  ws: [",
            "@openapi message Tick",
            "- a list",
        ]);
        let findings = findings(&diagnostics);
        assert_eq!(findings.len(), 2, "{findings:#?}");
        assert!(
            findings[0].starts_with("4: OF2005 embedded AsyncAPI YAML is invalid and was ignored"),
            "{findings:#?}"
        );
        assert_eq!(
            findings[1],
            "6: OF2005 embedded AsyncAPI YAML must be a mapping and was ignored"
        );
        let document = document.unwrap();
        assert!(document["channels"]["live"].get("bindings").is_none());
        assert_eq!(
            document["components"]["messages"]["Tick"],
            json!({ "name": "Tick", "payload": { "type": "integer", "format": "int32", "minimum": 0, "maximum": 255 } })
        );
    }

    #[test]
    fn yaml_for_operations_or_messages_the_block_lacks_is_reported_and_ignored() {
        let (document, diagnostics) = numbered(&[
            "@channel live",
            "@openapi message Nope",
            "x-foo: 1",
            "@openapi operation",
            "x-a: 1",
        ]);
        assert_eq!(
            findings(&diagnostics),
            [
                "2: OF2004 `@openapi message Nope` names no `@send` or `@receive` message of this block; its YAML is ignored",
                "5: OF2005 `@openapi operation` needs a `@send` or `@receive` in the block; the YAML is ignored",
            ]
        );
        // No message is left behind.
        assert!(document.unwrap().get("components").is_none());
    }

    #[test]
    fn invalid_targets_are_reported_and_their_yaml_ignored() {
        for (line, target) in [
            ("@openapi message", "message"),
            ("@openapi message bad/name", "message bad/name"),
            ("@openapi path", "path"),
            ("@openapi message A B", "message A B"),
            ("@openapi channel extra", "channel extra"),
        ] {
            let (document, diagnostics) = numbered(&[
                "Summary",
                "@channel live",
                line,
                "title: Lost",
                "more: text",
            ]);
            assert_eq!(
                findings(&diagnostics),
                [format!(
                    "3: OF2004 `@openapi {target}` is no target of a channel block (use channel, operation, document or message Name); the YAML is ignored"
                )],
            );
            let document = document.unwrap();
            assert!(
                document["channels"]["live"].get("title").is_none(),
                "{line}"
            );
            assert!(document.get("operations").is_none());
        }
    }

    #[test]
    fn prose_is_no_yaml_and_directives_end_a_section() {
        let (document, diagnostics) = numbered(&[
            "Summary",
            "@channel live",
            "@send A: u8",
            "Note: the servers are fast",
            "servers are fast: yes",
            "bindings: {}",
            "@tag T",
            "More text",
        ]);
        assert!(diagnostics.is_empty(), "{diagnostics:#?}");
        let document = document.unwrap();
        assert_eq!(
            document["operations"]["op"]["description"],
            "Note: the servers are fast\nservers are fast: yes\nMore text"
        );
        assert_eq!(document["channels"]["live"]["bindings"], json!({}));
        // Blank YAML sections change nothing.
        let (document, diagnostics) =
            numbered(&["@channel a", "@openapi", "", "@openapi operation"]);
        assert!(diagnostics.is_empty(), "{diagnostics:#?}");
        assert_eq!(
            document.unwrap()["channels"]["a"],
            json!({ "address": "a" })
        );
        let (_, diagnostics) = numbered(&["@channel a", "@openapi", "~"]);
        assert!(diagnostics.is_empty(), "{diagnostics:#?}");
    }
}

mod protobuf {
    use super::*;
    use oas_forge::proto::ProtoIndex;
    use oas_forge::source::{SourceFile, SourceKind};

    const CHAT: &str =
        "syntax = \"proto3\";\npackage chat.v1;\nmessage Envelope { bytes body = 1; }\n";

    fn compile(src: &[&str]) -> (Option<Value>, Diagnostics) {
        let mut protos = ProtoIndex::new();
        let mut diagnostics = Diagnostics::new();
        protos.add(
            &SourceFile::new("chat.proto", SourceKind::Proto, CHAT),
            &mut diagnostics,
        );
        let lines = lines(src);
        let numbers: Vec<usize> = (1..=src.len()).collect();
        let location = here();
        let compiled = compile_channel(
            ChannelSource::new(&lines, &location)
                .line_numbers(&numbers)
                .protos(&protos),
            &mut diagnostics,
        );
        (compiled.map(|c| c.document), diagnostics)
    }

    #[test]
    fn a_proto_message_is_a_multi_format_payload() {
        let (document, diagnostics) = compile(&[
            "@channel chat",
            r#"@send Envelope: proto(chat.v1.Envelope) "Every frame""#,
            "@receive Raw: proto( chat.v1.Envelope ) application/octet-stream",
        ]);
        assert!(diagnostics.is_empty(), "{diagnostics:#?}");
        let messages = &document.unwrap()["components"]["messages"];
        assert_eq!(
            messages["Envelope"],
            json!({
                "name": "Envelope",
                "contentType": "application/x-protobuf",
                "payload": {
                    "schemaFormat": "application/vnd.google.protobuf;version=3",
                    "schema": CHAT,
                },
                "description": "Every frame",
                "x-protobuf-message": "chat.v1.Envelope",
            })
        );
        // A media type wins over the protobuf default.
        assert_eq!(messages["Raw"]["contentType"], "application/octet-stream");
        assert_eq!(messages["Raw"]["x-protobuf-message"], "chat.v1.Envelope");
    }

    #[test]
    fn unknown_proto_messages_are_reported_and_left_out() {
        let (document, diagnostics) = compile(&[
            "@channel chat",
            "@send Envelope: proto(Envelope)",
            "@send Other: proto(chat.v1.Other)",
            "@send Ok: u8",
        ]);
        assert_eq!(
            codes(&diagnostics),
            [Code::UnknownProtoMessage, Code::UnknownProtoMessage]
        );
        let first = diagnostics.iter().next().unwrap();
        assert_eq!(first.location().map(|l| l.line()), Some(2));
        assert!(
            first
                .message()
                .starts_with("`proto(Envelope)` names no message"),
            "{}",
            first.message()
        );
        let document = document.unwrap();
        let names: Vec<&String> = document["components"]["messages"]
            .as_object()
            .unwrap()
            .keys()
            .collect();
        assert_eq!(names, ["Ok"]);
    }

    #[test]
    fn without_an_index_every_proto_message_is_unknown() {
        let lines = lines(&["@channel chat", "@send Envelope: proto(chat.v1.Envelope)"]);
        let location = here();
        let mut diagnostics = Diagnostics::new();
        compile_channel(ChannelSource::new(&lines, &location), &mut diagnostics);
        assert_eq!(codes(&diagnostics), [Code::UnknownProtoMessage]);
    }
}

#[test]
fn fenced_yaml_after_openapi_in_a_channel_block() {
    let fenced = compile(
        &[
            "@channel pings",
            "@send Ping: u8",
            "@openapi message Ping",
            "```yaml",
            "title: A ping",
            "```",
            "Pings the server.",
        ],
        None,
    );
    let plain = compile(
        &[
            "Pings the server.",
            "@channel pings",
            "@send Ping: u8",
            "@openapi message Ping",
            "title: A ping",
        ],
        None,
    );
    assert_eq!(fenced.0, plain.0);
    assert!(fenced.1.is_empty(), "{:#?}", fenced.1);
}

#[test]
fn only_a_fence_right_after_openapi_opens_fenced_yaml() {
    // The fence is no YAML fence here: it is part of the description.
    let (document, _) = compile(
        &[
            "@channel pings",
            "@send Ping: u8",
            "@openapi message Ping",
            "title: A ping",
            "@description Example:",
            "```yaml",
            "a: 1",
            "```",
        ],
        None,
    );
    let document = document.unwrap();
    assert_eq!(
        document["components"]["messages"]["Ping"]["title"],
        "A ping"
    );
    assert_eq!(
        document["operations"]["send_pings"]["description"],
        "Example:\n```yaml\na: 1\n```"
    );
}

#[test]
fn a_fence_inside_open_yaml_is_yaml_text() {
    let (_, diagnostics) = compile(
        &[
            "@channel pings",
            "@send Ping: u8",
            "@openapi message Ping",
            "title: A ping",
            "```",
            "x-a: 1",
            "```",
        ],
        None,
    );
    // Not a fenced section: the backticks are (invalid) YAML of the open one.
    assert_eq!(diagnostics.len(), 1, "{diagnostics:#?}");
}

/// Where embedded YAML ends and prose goes on (the same rules as in route
/// blocks).
mod yaml_and_prose {
    use super::*;

    #[test]
    fn description_lines_are_never_yaml() {
        let (document, diagnostics) = numbered(&[
            "@description Prose here.",
            "x-c: [after-description",
            "summary: inside description",
            "@channel lobby /lobby/ws",
            "@summary The lobby feed",
            "@send Feed: u8",
        ]);
        assert!(diagnostics.is_empty(), "{diagnostics:#?}");
        let document = document.unwrap();
        assert_eq!(
            document["operations"]["op"]["description"],
            "Prose here.\nx-c: [after-description\nsummary: inside description"
        );
        assert_eq!(
            document["channels"]["lobby"],
            json!({
                "address": "/lobby/ws",
                "messages": { "Feed": { "$ref": "#/components/messages/Feed" } },
            })
        );
    }

    #[test]
    fn a_markdown_list_after_yaml_and_a_blank_line_is_prose() {
        let (document, diagnostics) = numbered(&[
            " Live",
            " @channel live",
            " @send A: u8",
            " bindings:",
            "   ws:",
            "",
            "     method: GET",
            "",
            " - first point",
            " - second point",
            "",
            " x-after: 1",
            "",
            " Closing words: a colon in prose.",
        ]);
        assert!(diagnostics.is_empty(), "{diagnostics:#?}");
        let document = document.unwrap();
        assert_eq!(
            document["channels"]["live"]["bindings"],
            json!({ "ws": { "method": "GET" } })
        );
        assert_eq!(document["channels"]["live"]["x-after"], 1);
        assert_eq!(
            document["operations"]["op"]["description"],
            "- first point\n- second point\n\nClosing words: a colon in prose."
        );
    }

    #[test]
    fn explicit_yaml_is_not_ended_by_prose() {
        let (document, diagnostics) =
            numbered(&["@channel live", "@openapi", "x-a: 1", "", "- not prose"]);
        // Explicit YAML stays YAML: a sequence after a mapping is invalid.
        assert_eq!(codes(&diagnostics), [Code::InvalidRouteYaml]);
        assert!(document.unwrap()["channels"]["live"].get("x-a").is_none());
    }
}

mod channel_yaml_notes {
    use super::*;

    #[test]
    fn channel_yaml_that_changes_what_directives_set_is_noted_at_its_line() {
        let (document, diagnostics) = numbered(&[
            "The lobby",
            "Who is waiting.",
            "@channel lobby /lobby/ws",
            "@summary The lobby feed",
            "@send Feed: u8",
            "summary: Another summary",
            "description: Another description",
            "address: /elsewhere",
            "title: Lobby",
        ]);
        assert_eq!(
            findings(&diagnostics),
            [
                "6: OF4001 embedded YAML overrides the DSL value at `/summary`",
                "7: OF4001 embedded YAML overrides the DSL value at `/description`",
                "8: OF4001 embedded YAML overrides the DSL value at `/address`",
            ]
        );
        assert!(diagnostics.iter().all(|d| d.severity() == Severity::Note));
        let document = document.unwrap();
        let channel = &document["channels"]["lobby"];
        assert_eq!(channel["summary"], "Another summary");
        assert_eq!(channel["address"], "/elsewhere");
        // The operations keep what the block says.
        assert_eq!(document["operations"]["op"]["summary"], "The lobby feed");
        assert_eq!(
            document["operations"]["op"]["description"],
            "The lobby\nWho is waiting."
        );
    }

    #[test]
    fn equal_values_and_a_block_without_summary_are_not_noted() {
        let (_, diagnostics) = numbered(&[
            "The lobby",
            "@channel lobby",
            "summary: The lobby",
            "description: Anything",
        ]);
        assert!(diagnostics.is_empty(), "{diagnostics:#?}");
        let (_, diagnostics) = numbered(&["@channel lobby", "@openapi", "summary: S"]);
        assert!(diagnostics.is_empty(), "{diagnostics:#?}");
        // Message YAML is compared with the message, not with the block.
        let (_, diagnostics) = numbered(&[
            "The lobby",
            "@channel lobby",
            "@send A: u8",
            "@openapi message A",
            "summary: Other",
        ]);
        assert!(diagnostics.is_empty(), "{diagnostics:#?}");
    }
}

mod message_names {
    use super::*;

    #[test]
    fn one_name_with_other_content_in_one_channel_is_reported() {
        let (document, diagnostics) = numbered(&[
            "Seat",
            "@channel seat /games/{id}/ws",
            r#"@send HelloAck: u8 "seat ack""#,
            r#"@receive HelloAck: String "different payload, same name""#,
        ]);
        assert_eq!(
            findings(&diagnostics),
            [
                "4: OF4001 message `HelloAck` is defined again in this channel with other content; the later definition is used (give the messages distinct names)"
            ]
        );
        let document = document.unwrap();
        assert_eq!(
            document["components"]["messages"]["HelloAck"]["description"],
            "different payload, same name"
        );
    }

    #[test]
    fn one_message_sent_and_received_is_fine_and_referenced_once() {
        let (document, diagnostics) = numbered(&[
            "@channel echo",
            "@send Ping: u8",
            "@receive Ping: u8",
            "@send Ping: u8",
        ]);
        assert!(diagnostics.is_empty(), "{diagnostics:#?}");
        let document = document.unwrap();
        assert_eq!(
            document["operations"]["op_send"]["messages"],
            json!([{ "$ref": "#/channels/echo/messages/Ping" }])
        );
        assert_eq!(
            document["operations"]["op_receive"]["messages"],
            json!([{ "$ref": "#/channels/echo/messages/Ping" }])
        );
    }

    /// A definition that differs from any earlier one of its name is
    /// reported, also when it equals one before the last.
    #[test]
    fn a_definition_unlike_any_earlier_one_is_reported() {
        let (document, diagnostics) = numbered(&[
            "@channel echo",
            "@send Ping: u8",
            "@send Ping: String",
            "@send Ping: u8",
        ]);
        let found: Vec<String> = findings(&diagnostics)
            .into_iter()
            .map(|f| {
                f.split_once(' ')
                    .map_or(f.clone(), |(line, _)| line.to_string())
            })
            .collect();
        assert_eq!(found, ["3:", "4:"]);
        let document = document.unwrap();
        assert_eq!(
            document["components"]["messages"]["Ping"]["payload"],
            json!({ "type": "integer", "format": "int32", "minimum": 0, "maximum": 255 })
        );
    }
}

/// Large blocks: the messages, YAML lines and handshake parameters of a
/// block were scanned for each new one, so tens of thousands took minutes.
mod large_blocks {
    use super::*;
    use std::time::{Duration, Instant};

    /// Compiles `block` (operation id `op`) in less than `seconds`.
    fn compiled(block: Vec<String>, seconds: u64) -> (Value, Diagnostics) {
        let location = here();
        let mut diagnostics = Diagnostics::new();
        let started = Instant::now();
        let compiled = compile_channel(
            ChannelSource::new(&block, &location).operation_id(Some("op")),
            &mut diagnostics,
        );
        assert!(
            started.elapsed() < Duration::from_secs(seconds),
            "{:?}",
            started.elapsed()
        );
        (compiled.expect("a valid channel").document, diagnostics)
    }

    /// 20 000 `@send` lines of one name (and content) took minutes: each was
    /// compared with every earlier message of its name.
    #[test]
    fn many_messages_of_one_name() {
        let mut block = vec!["@channel chat".to_string()];
        block.extend(std::iter::repeat_n("@send A: String".to_string(), 20_000));
        let (document, diagnostics) = compiled(block, 30);
        assert!(diagnostics.is_empty(), "{diagnostics:#?}");
        assert_eq!(
            document["components"]["messages"],
            json!({ "A": { "name": "A", "payload": { "type": "string" } } })
        );
        assert_eq!(
            document["operations"]["op"]["messages"],
            json!([{ "$ref": "#/channels/chat/messages/A" }])
        );
    }

    /// 40 000 `@send` lines of distinct names took over a minute: the
    /// references of the operation were checked against all before.
    #[test]
    fn many_distinct_messages() {
        let mut block = vec!["@channel chat".to_string()];
        block.extend((0..40_000).map(|i| format!("@send M{i}: String")));
        let (document, diagnostics) = compiled(block, 30);
        assert!(diagnostics.is_empty(), "{diagnostics:#?}");
        let messages = document["components"]["messages"].as_object().unwrap();
        assert_eq!(messages.len(), 40_000);
        let refs = document["operations"]["op"]["messages"].as_array().unwrap();
        assert_eq!(refs.len(), 40_000);
        assert_eq!(
            refs[39_999],
            json!({ "$ref": "#/channels/chat/messages/M39999" })
        );
    }

    /// 40 000 messages, each with its `@openapi message Name` YAML, took over
    /// a minute: the YAML lines were filtered again for each message.
    #[test]
    fn many_message_sections() {
        let mut block = vec!["@channel chat".to_string()];
        block.extend((0..40_000).map(|i| format!("@send M{i}: String")));
        for i in 0..40_000 {
            block.push(format!("@openapi message M{i}"));
            block.push(format!("title: T{i}"));
        }
        let (document, diagnostics) = compiled(block, 30);
        assert!(diagnostics.is_empty(), "{diagnostics:#?}");
        let messages = document["components"]["messages"].as_object().unwrap();
        assert_eq!(messages.len(), 40_000);
        assert_eq!(messages["M0"]["title"], "T0");
        assert_eq!(messages["M39999"]["title"], "T39999");
    }

    /// 120 000 `@ws-query` lines took a minute: each name was looked for in
    /// the list of those before.
    #[test]
    fn many_handshake_parameters() {
        let mut block = vec!["@channel chat".to_string(), "@send A: String".to_string()];
        block.extend((0..120_000).map(|i| format!("@ws-query q{i}: String")));
        block.push("@ws-query q7: u32".to_string());
        let (document, diagnostics) = compiled(block, 30);
        assert!(diagnostics.is_empty(), "{diagnostics:#?}");
        let query = &document["channels"]["chat"]["bindings"]["ws"]["query"];
        let names: Vec<&String> = query["properties"].as_object().unwrap().keys().collect();
        assert_eq!(names.len(), 120_000);
        assert_eq!(names[7], "q7");
        assert_eq!(query["properties"]["q7"]["type"], "integer");
    }
}
