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
            "@openapi message Extra",
            "payload: { type: string }",
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
        assert_eq!(
            messages["Extra"],
            json!({ "payload": { "type": "string" } })
        );
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
    fn document_yaml_that_changes_the_dsl_is_noted_at_the_block() {
        let (_, diagnostics) = numbered(&[
            "@channel live",
            "@openapi document",
            "channels: { live: { address: elsewhere } }",
        ]);
        assert_eq!(
            findings(&diagnostics),
            ["1: OF4001 embedded YAML overrides the DSL value at `/channels/live/address`"]
        );
    }

    #[test]
    fn invalid_yaml_is_reported_and_ignored() {
        let (document, diagnostics) = numbered(&[
            "@channel live",
            "bindings:",
            "  ws: [",
            "@openapi message Tick",
            "- a list",
            "@openapi operation",
            "x-a: 1",
        ]);
        let findings = findings(&diagnostics);
        assert_eq!(findings.len(), 3, "{findings:#?}");
        assert!(
            findings[0].starts_with("3: OF2005 embedded AsyncAPI YAML is invalid and was ignored"),
            "{findings:#?}"
        );
        assert_eq!(
            findings[1],
            "5: OF2005 embedded AsyncAPI YAML must be a mapping and was ignored"
        );
        assert_eq!(
            findings[2],
            "7: OF2005 `@openapi operation` needs a `@send` or `@receive` in the block; the YAML is ignored"
        );
        let document = document.unwrap();
        assert!(document["channels"]["live"].get("bindings").is_none());
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
            "servers are fast: yes",
            "bindings: {}",
            "@tag T",
            "More text",
        ]);
        assert!(diagnostics.is_empty(), "{diagnostics:#?}");
        let document = document.unwrap();
        assert_eq!(
            document["operations"]["op"]["description"],
            "servers are fast: yes\nMore text"
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
