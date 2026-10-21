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
