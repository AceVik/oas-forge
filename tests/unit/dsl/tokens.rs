//! `src/dsl/tokens.rs`: quoted descriptions with escaped quotes (`\"`) and
//! backslashes (`\\`), in every directive that takes one.

use crate::common::{here, lines, operation, try_route};
use oas_forge::diagnostics::Diagnostics;
use oas_forge::dsl::{self, ChannelSource};
use serde_json::json;

/// A description with both escapes and Markdown, as written in a directive.
const WRITTEN: &str = r#""not `{\"enabled\": true}` but C:\\temp""#;
/// The same description as text.
const TEXT: &str = r#"not `{"enabled": true}` but C:\temp"#;

fn op(src: &[&str]) -> serde_json::Value {
    let (document, diagnostics) = try_route(src);
    assert!(diagnostics.is_empty(), "{diagnostics:#?}");
    let document = document.unwrap();
    let (_, item) = document["paths"]
        .as_object()
        .unwrap()
        .iter()
        .next()
        .unwrap();
    let (_, operation) = item.as_object().unwrap().iter().next().unwrap();
    operation.clone()
}

#[test]
fn responses_keep_escaped_quotes_in_their_description() {
    let op = op(&[
        "@route POST /x",
        &format!("@return 400: $ErrorBody {WRITTEN}"),
        &format!("@return 201: {{}} {WRITTEN}"),
        &format!("@problem 409 {WRITTEN}"),
        &format!("@return-header 201 X-Note: String {WRITTEN}"),
        &format!("@body $Note {WRITTEN}"),
    ]);
    assert_eq!(op["responses"]["400"]["description"], TEXT);
    assert_eq!(
        op["responses"]["400"]["content"]["application/json"]["schema"],
        json!({ "$ref": "$ErrorBody" })
    );
    assert_eq!(op["responses"]["201"]["description"], TEXT);
    assert_eq!(
        op["responses"]["201"]["content"]["application/json"]["schema"],
        json!({})
    );
    assert_eq!(op["responses"]["409"]["description"], TEXT);
    assert_eq!(
        op["responses"]["201"]["headers"]["X-Note"]["description"],
        TEXT
    );
    assert_eq!(op["requestBody"]["description"], TEXT);
}

#[test]
fn parameters_keep_escaped_quotes() {
    let op = op(&[
        &format!(r#"@route GET /x/{{id: u64 {WRITTEN}}}"#),
        &format!("@query-param q: String {WRITTEN}"),
        r#"@query-param s: String example="say \"hi\"" "Search""#,
    ]);
    let params = op["parameters"].as_array().unwrap();
    assert_eq!(params[0]["description"], TEXT);
    assert_eq!(params[1]["description"], TEXT);
    assert_eq!(params[2]["example"], r#"say "hi""#);
    assert_eq!(params[2]["description"], "Search");
}

#[test]
fn servers_and_documentation_keep_escaped_quotes() {
    let op = op(&[
        "@route GET /x",
        &format!("@server https://eu.example.com {WRITTEN}"),
        &format!("@external-docs https://docs.example.com {WRITTEN}"),
    ]);
    assert_eq!(op["servers"][0]["description"], TEXT);
    assert_eq!(op["servers"][0]["url"], "https://eu.example.com");
    assert_eq!(op["externalDocs"]["description"], TEXT);
}

#[test]
fn messages_keep_escaped_quotes() {
    let block = lines(&[
        "@channel ping",
        &format!("@send Ping: u8 {WRITTEN}"),
        &format!("@receive Pong: u8 {WRITTEN}"),
    ]);
    let location = here();
    let mut diagnostics = Diagnostics::new();
    let channel =
        dsl::compile_channel(ChannelSource::new(&block, &location), &mut diagnostics).unwrap();
    assert!(diagnostics.is_empty(), "{diagnostics:#?}");
    let messages = &channel.document["components"]["messages"];
    assert_eq!(messages["Ping"]["description"], TEXT);
    assert_eq!(messages["Pong"]["description"], TEXT);
}

#[test]
fn a_description_without_escapes_is_unchanged() {
    let op = operation(
        &["@route GET /x", r#"@return 200: String "Plain \d text""#],
        "/x",
        "get",
    );
    // A backslash before anything but `"` or `\` stays.
    assert_eq!(op["responses"]["200"]["description"], r"Plain \d text");
}

#[test]
fn an_unbalanced_quote_is_no_description() {
    // As before: a trailing quote without opening quote is dropped.
    let op = operation(&["@route GET /x", r#"@return 200: String""#], "/x", "get");
    assert_eq!(op["responses"]["200"]["description"], "OK");
    assert_eq!(
        op["responses"]["200"]["content"]["application/json"]["schema"],
        json!({ "type": "string" })
    );
}
