//! `src/dsl/prose.rs`: the summary and description of route, webhook and
//! channel blocks: plain lines, `@summary`, `@description` and `@internal`.

use crate::common::{here, lines};
use oas_forge::Format;
use oas_forge::diagnostics::Diagnostics;
use oas_forge::dsl::{self, ChannelSource, RouteSource};
use serde_json::{Value, json};

/// Doc lines as the extraction reads them: one space after `///`.
fn doc(src: &[&str]) -> Vec<String> {
    src.iter()
        .map(|line| {
            if line.is_empty() {
                String::new()
            } else {
                format!(" {line}")
            }
        })
        .collect()
}

/// The operation of a route block (its only path and method).
fn compile_route(src: &[&str], defaults: &[&str]) -> (Value, Diagnostics) {
    let block = doc(src);
    let defaults = lines(defaults);
    let location = here();
    let mut diagnostics = Diagnostics::new();
    let route = dsl::compile(
        RouteSource::new(&block, &location).defaults(&defaults),
        &mut diagnostics,
    )
    .unwrap_or_else(|| panic!("rejected: {diagnostics:#?}"));
    let operation = route.document["paths"][&route.path][&route.method].clone();
    (operation, diagnostics)
}

/// The summary and description of a route block.
fn text(src: &[&str]) -> (Value, Value) {
    let (operation, diagnostics) = compile_route(src, &[]);
    assert!(diagnostics.is_empty(), "{diagnostics:#?}");
    (
        operation["summary"].clone(),
        operation["description"].clone(),
    )
}

#[test]
fn without_directives_the_first_plain_line_is_the_summary() {
    assert_eq!(
        text(&[
            "Get a deck",
            "",
            "Details.",
            "  - indented",
            "@route GET /x"
        ]),
        (json!("Get a deck"), json!("Details.\n  - indented"))
    );
}

#[test]
fn description_replaces_the_free_text() {
    assert_eq!(
        text(&[
            "`POST /decks` — creates a deck.",
            "",
            "Developer notes: we store the version first because of #312;",
            "see `store::persist` for the locking trade-off.",
            "@route POST /decks",
            "@summary Create a deck",
            "@description Creates a deck for the signed-in player.",
            "The name must be unique per player.",
            "@return 201: String",
        ]),
        (
            json!("Create a deck"),
            json!("Creates a deck for the signed-in player.\nThe name must be unique per player.")
        )
    );
}

#[test]
fn description_continues_until_the_next_directive() {
    assert_eq!(
        text(&[
            "@route GET /x",
            "@description A",
            "B",
            "",
            "responses: prose here",
            "@tag T",
            "Get x",
            "Free text after the summary is left out.",
        ]),
        (json!("Get x"), json!("A\nB\n\nresponses: prose here"))
    );
    // Several `@description` lines add up; an empty one hides the free text.
    assert_eq!(
        text(&[
            "Get x",
            "Free.",
            "@route GET /x",
            "@description A",
            "@description B"
        ]),
        (json!("Get x"), json!("A\nB"))
    );
    let (summary, description) = text(&["Get x", "Free.", "@route GET /x", "@description"]);
    assert_eq!((summary, description), (json!("Get x"), Value::Null));
}

#[test]
fn a_multi_line_description_keeps_its_relative_indentation() {
    assert_eq!(
        text(&[
            "@route GET /x",
            "@description Steps:",
            "  1. Create",
            "     the deck",
            "",
            "Done.",
        ])
        .1,
        json!("Steps:\n  1. Create\n     the deck\n\nDone.")
    );
}

#[test]
fn an_explicit_summary_leaves_every_plain_line_to_the_description() {
    // No plain line is the summary next to `@summary`, before or after it.
    assert_eq!(
        text(&[
            "Rust summary",
            "",
            "A box.",
            "@route GET /x",
            "@summary API summary"
        ]),
        (json!("API summary"), json!("Rust summary\n\nA box."))
    );
    assert_eq!(
        text(&["@summary API summary", "Details.", "@route GET /x"]),
        (json!("API summary"), json!("Details."))
    );
    // `@description` still replaces them, `@internal` lines stay out.
    assert_eq!(
        text(&[
            "Rust summary",
            "@internal",
            "Notes.",
            "@route GET /x",
            "@summary API summary",
            "Public.",
        ]),
        (json!("API summary"), json!("Rust summary\nPublic."))
    );
    assert_eq!(
        text(&[
            "Rust summary",
            "@route GET /x",
            "@summary S",
            "@description D"
        ]),
        (json!("S"), json!("D"))
    );
}

#[test]
fn a_multi_line_description_renders_without_leading_spaces() {
    let (operation, diagnostics) = compile_route(
        &[
            "Service info",
            "@route GET /info",
            "@description Returns the version and the build.",
            "The build is a git hash.",
            "",
            "- one",
            "  - nested",
        ],
        &[],
    );
    assert!(diagnostics.is_empty(), "{diagnostics:#?}");
    let description = operation["description"].as_str().unwrap();
    assert_eq!(
        description,
        "Returns the version and the build.\nThe build is a git hash.\n\n- one\n  - nested"
    );
    let yaml = Format::Yaml.render(&json!({ "description": description }));
    assert_eq!(
        yaml,
        "description: |-\n  Returns the version and the build.\n  The build is a git hash.\n\n  - one\n    - nested\n"
    );
    assert!(!yaml.contains("|2"), "{yaml}");
}

#[test]
fn internal_lines_are_left_out() {
    let (operation, diagnostics) = compile_route(
        &[
            "Get a deck",
            "@internal",
            "Developer notes:",
            "responses: not YAML here",
            "",
            "  - nor this",
            "@route GET /x",
            "Public details.",
        ],
        &[],
    );
    assert!(diagnostics.is_empty(), "{diagnostics:#?}");
    assert_eq!(operation["summary"], "Get a deck");
    assert_eq!(operation["description"], "Public details.");
    assert_eq!(
        operation["responses"],
        json!({ "default": { "description": "Default response" } })
    );
}

#[test]
fn internal_may_come_first_or_after_the_route() {
    assert_eq!(
        text(&["@internal", "notes", "@route GET /x", "Get x", "Details."]),
        (json!("Get x"), json!("Details."))
    );
    assert_eq!(
        text(&[
            "Get x",
            "@route GET /x",
            "@internal",
            "notes",
            "",
            "more notes",
            "@tag T",
            "Details."
        ]),
        (json!("Get x"), json!("Details."))
    );
    // Inside a `@description`, `@internal` ends it.
    assert_eq!(
        text(&[
            "@route GET /x",
            "@description Public.",
            "@internal",
            "Private.",
            "@deprecated"
        ]),
        (Value::Null, json!("Public."))
    );
}

#[test]
fn module_defaults_accept_internal() {
    let (operation, diagnostics) = compile_route(
        &["@route GET /x"],
        &[
            "@route-defaults",
            "@internal",
            "responses: why the module is here",
            "@tag T",
        ],
    );
    assert!(diagnostics.is_empty(), "{diagnostics:#?}");
    assert_eq!(operation["tags"], json!(["T"]));
    assert!(operation["responses"].get("default").is_some());
}

/// The operation of a channel block with one `@send`.
fn channel(src: &[&str]) -> Value {
    let block = doc(src);
    let location = here();
    let mut diagnostics = Diagnostics::new();
    let compiled = dsl::compile_channel(
        ChannelSource::new(&block, &location).operation_id(Some("op")),
        &mut diagnostics,
    )
    .unwrap_or_else(|| panic!("rejected: {diagnostics:#?}"));
    assert!(diagnostics.is_empty(), "{diagnostics:#?}");
    compiled.document["operations"]["op"].clone()
}

#[test]
fn channels_follow_the_same_rules() {
    let op = channel(&[
        "Pings the server.",
        "",
        "Developer notes.",
        "@channel ping",
        "@summary Ping",
        "@description Sends a ping",
        "  twice.",
        "@internal",
        "Uses UDP.",
        "@send Ping: u8",
    ]);
    assert_eq!(op["summary"], "Ping");
    assert_eq!(op["description"], "Sends a ping\n  twice.");
    let op = channel(&[
        "@internal",
        "Notes.",
        "@channel ping",
        "Pings.",
        "",
        "Details",
        "  indented.",
        "@send Ping: u8",
    ]);
    assert_eq!(op["summary"], "Pings.");
    assert_eq!(op["description"], "Details\n  indented.");
    // `@description` continues on the following plain lines.
    let op = channel(&[
        "@channel ping",
        "@description Sends",
        "a ping.",
        "@send Ping: u8",
    ]);
    assert!(op.get("summary").is_none(), "{op}");
    assert_eq!(op["description"], "Sends\na ping.");
}
