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
fn an_escaped_final_quote_is_no_description() {
    let (_, diagnostics) = try_route(&["@route GET /x", r#"@return 200: String "open\""#]);
    assert_eq!(diagnostics.len(), 1, "{diagnostics:#?}");
}

#[test]
fn backticks_after_an_escaped_quote_stay_in_the_description() {
    let op = op(&["@route GET /x", r#"@return 200: String "say \" `hi` \"""#]);
    assert_eq!(op["responses"]["200"]["description"], r#"say " `hi` ""#);
}

#[test]
fn a_backslash_outside_quotes_escapes_nothing() {
    // A pattern ends at whitespace, also after a backslash: `b` is no flag.
    let (_, diagnostics) = try_route(&["@route GET /x", r"@query-param q: String pattern=a\ b"]);
    assert_eq!(diagnostics.len(), 1, "{diagnostics:#?}");
    assert!(
        diagnostics.iter().next().unwrap().message().contains("`b`"),
        "{diagnostics:#?}"
    );
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

/// Markdown backticks around directive arguments (for `clippy::doc_markdown`
/// and rustdoc): the document is the same as without them.
mod backticks {
    use super::*;
    use oas_forge::dsl::RouteSource;

    /// Compiles both blocks (with the same defaults) and checks that they give
    /// the same document without findings.
    fn same(with: &[&str], without: &[&str], defaults: (&[&str], &[&str])) {
        let compile = |src: &[&str], defaults: &[&str]| {
            let block = lines(src);
            let defaults = lines(defaults);
            let location = here();
            let mut diagnostics = Diagnostics::new();
            let route = dsl::compile(
                RouteSource::new(&block, &location)
                    .operation_id(Some("op"))
                    .defaults(&defaults),
                &mut diagnostics,
            );
            assert!(diagnostics.is_empty(), "{src:?}: {diagnostics:#?}");
            route
                .unwrap_or_else(|| panic!("rejected: {src:?}"))
                .document
        };
        assert_eq!(compile(with, defaults.0), compile(without, defaults.1));
    }

    #[test]
    fn route_directives_accept_backticked_arguments() {
        same(
            &[
                "@route GET `/decks/{id: u64 \"Deck id\"}`",
                "@return 404: `$ErrorBody` \"No such deck\"",
                "@return 200: `Vec<$Deck>` `application/json`",
                "@body `$ChatReport`",
                "@query-params `$PoolQuery`",
                "@query-param limit: `u32` default=20 \"Page size\"",
                "@param header `X-Trace`: `String`",
                "@return-header 200 `X-Rate`: `u32` \"Limit\"",
                "@problem `409` \"Taken\"",
                "@tag `Decks`, `Admin`",
                "@security `bearer` + `api_key`",
                "@server `https://eu.example.com` \"EU\"",
                "@external-docs `https://docs.example.com`",
                "@operation-id `getDeck`",
                "@openapi `path`",
                "x-scope: path",
            ],
            &[
                "@route GET /decks/{id: u64 \"Deck id\"}",
                "@return 404: $ErrorBody \"No such deck\"",
                "@return 200: Vec<$Deck> application/json",
                "@body $ChatReport",
                "@query-params $PoolQuery",
                "@query-param limit: u32 default=20 \"Page size\"",
                "@param header X-Trace: String",
                "@return-header 200 X-Rate: u32 \"Limit\"",
                "@problem 409 \"Taken\"",
                "@tag Decks, Admin",
                "@security bearer + api_key",
                "@server https://eu.example.com \"EU\"",
                "@external-docs https://docs.example.com",
                "@operation-id getDeck",
                "@openapi path",
                "x-scope: path",
            ],
            (&[], &[]),
        );
    }

    #[test]
    fn webhooks_and_module_defaults_accept_backticked_arguments() {
        same(
            &["@webhook `POST` `deckDeleted`", "@body `$Deck`"],
            &["@webhook POST deckDeleted", "@body $Deck"],
            (&[], &[]),
        );
        same(
            &["@route GET `/users`"],
            &["@route GET /users"],
            (
                &["@route-defaults", "@prefix `/api`", "@tag `Api`"],
                &["@route-defaults", "@prefix /api", "@tag Api"],
            ),
        );
    }

    #[test]
    fn markdown_in_descriptions_and_text_directives_is_kept() {
        let op = op(&[
            "@route GET /x",
            "@summary Get a `Deck`",
            "@description Returns the `Deck` and `its cards`.",
            "@return 200: `$Deck` \"A `Deck` with `cards`\"",
            "@return 204: \"`Nothing`\"",
        ]);
        assert_eq!(op["summary"], "Get a `Deck`");
        assert_eq!(op["description"], "Returns the `Deck` and `its cards`.");
        assert_eq!(
            op["responses"]["200"]["description"],
            "A `Deck` with `cards`"
        );
        assert_eq!(op["responses"]["204"]["description"], "`Nothing`");
    }

    #[test]
    fn backticks_that_do_not_wrap_a_whole_argument_stay() {
        // Not a whole argument: the type is still invalid.
        let (_, diagnostics) = try_route(&["@route GET /x", "@return 200: Vec<`$Deck`>"]);
        assert_eq!(diagnostics.len(), 1, "{diagnostics:#?}");
        // An unclosed or doubled backtick is no wrapping either.
        for bad in ["@return 200: `$Deck", "@return 200: ``$Deck``"] {
            let (_, diagnostics) = try_route(&["@route GET /x", bad]);
            assert_eq!(diagnostics.len(), 1, "{bad}: {diagnostics:#?}");
        }
    }

    #[test]
    fn channel_directives_accept_backticked_arguments() {
        let compile = |src: &[&str]| {
            let block = lines(src);
            let location = here();
            let mut diagnostics = Diagnostics::new();
            let channel =
                dsl::compile_channel(ChannelSource::new(&block, &location), &mut diagnostics)
                    .unwrap();
            assert!(diagnostics.is_empty(), "{diagnostics:#?}");
            channel.document
        };
        assert_eq!(
            compile(&[
                "@channel `pings` `ping/{id}`",
                "@send `Ping`: `u8` \"A `ping`\"",
                "@receive `Pong`: `$Pong`",
                "@tag `Health`",
            ]),
            compile(&[
                "@channel pings ping/{id}",
                "@send Ping: u8 \"A `ping`\"",
                "@receive Pong: $Pong",
                "@tag Health",
            ])
        );
    }
}
