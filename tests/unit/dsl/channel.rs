//! `src/dsl/channel.rs`: the AsyncAPI channel DSL.

use crate::common::{codes, here, lines};
use oas_forge::diagnostics::{Code, Diagnostics};
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
