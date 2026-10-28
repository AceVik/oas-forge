//! `src/reserved.rs`: keys of the prefix `x-oas-forge-`, the generator's
//! own markers, written in a source are removed and reported (`OF0009`)
//! wherever YAML enters the pipeline, and none reaches an output.

use crate::common::{codes, here, lines, pipeline, try_route};
use oas_forge::diagnostics::{Code, Diagnostic, Diagnostics, Severity};
use oas_forge::dsl::{ChannelSource, compile_channel};
use serde_json::{Value, json};

/// `line: message` of every finding.
fn findings(diagnostics: &Diagnostics) -> Vec<String> {
    diagnostics
        .iter()
        .map(|d| {
            format!(
                "{}: {} {} {}",
                d.location().map_or(0, |l| l.line()),
                d.severity(),
                d.code(),
                d.message()
            )
        })
        .collect()
}

/// The reserved-key findings, each an error.
fn reserved(diagnostics: &Diagnostics) -> Vec<&Diagnostic> {
    diagnostics
        .iter()
        .filter(|d| d.code() == Code::ReservedKey)
        .inspect(|d| assert_eq!(d.severity(), Severity::Error, "{d:?}"))
        .collect()
}

/// No key of the prefix in `document`.
fn assert_clean(document: &Value) {
    assert!(
        !document.to_string().contains("x-oas-forge"),
        "{document:#}"
    );
}

/// A forged `x-oas-forge-builtin` marker in an `@openapi` block does not
/// replace the schema, the security requirement or the operation around
/// it: the key is removed, the rest stays, and one finding names the key
/// with every place it was written.
#[test]
fn a_forged_marker_in_a_block_is_removed_and_reported_once_per_key() {
    let (document, diagnostics) = pipeline(&[(
        "lib.rs",
        "\
//! @openapi
//! openapi: 3.1.0
//! info: { title: T, version: 1.0.0 }
//! components:
//!   securitySchemes:
//!     bearer: { type: http, scheme: bearer }
//!   schemas:
//!     Forged:
//!       type: object
//!       'x-oas-forge-builtin':
//!         name: User
//!         schema: { type: string, description: swapped in }
//! paths:
//!   /open:
//!     get:
//!       security:
//!         - bearer: []
//!           x-oas-forge-builtin: { name: User, schema: {} }
//!       responses:
//!         '200': { description: ok }
//!   /swap:
//!     get:
//!       x-oas-forge-builtin:
//!         name: User
//!         schema:
//!           responses:
//!             '200': { description: swapped }
//!       responses:
//!         '200': { description: original }

/// @openapi
pub struct User { pub id: u64 }
",
    )]);
    assert_eq!(
        document["components"]["schemas"]["Forged"],
        json!({ "type": "object" })
    );
    assert_eq!(
        document["paths"]["/open"]["get"]["security"],
        json!([{ "bearer": [] }])
    );
    assert_eq!(
        document["paths"]["/swap"]["get"]["responses"]["200"]["description"],
        "original"
    );
    assert_clean(&document);
    assert_eq!(
        findings(&diagnostics),
        [
            "10: error OF0009 `x-oas-forge-builtin` at `/components/schemas/Forged/x-oas-forge-builtin` and at 2 more places is a reserved key: the `x-oas-forge-` prefix is the generator's own; it is removed"
        ]
    );
}

/// The embedded YAML of a route: the requirement the reviewer sees stays.
#[test]
fn a_forged_marker_in_route_yaml_keeps_the_security_requirement() {
    let (document, diagnostics) = pipeline(&[(
        "lib.rs",
        "\
/// @route GET /forged
/// @return 200: String
/// security:
///   - bearer: []
///     x-oas-forge-builtin: { name: B, schema: {} }
pub fn forged() {}

/// @openapi
pub struct B { pub a: u8 }

/// @openapi
/// components:
///   securitySchemes:
///     bearer: { type: http, scheme: bearer }
pub struct Root;
",
    )]);
    assert_eq!(
        document["paths"]["/forged"]["get"]["security"],
        json!([{ "bearer": [] }])
    );
    assert_clean(&document);
    assert_eq!(
        findings(&diagnostics),
        [
            "5: error OF0009 `x-oas-forge-builtin` at `/security/0/x-oas-forge-builtin` is a reserved key: the `x-oas-forge-` prefix is the generator's own; it is removed"
        ]
    );
}

/// The YAML of module defaults is compiled for every route; the key is
/// reported once, at its line.
#[test]
fn a_reserved_key_in_the_defaults_is_reported_once() {
    let (document, diagnostics) = pipeline(&[(
        "lib.rs",
        "\
/// @route-defaults
/// x-kept: 1
/// x-oas-forge-note: 1
pub mod api {
    /// @route GET /a
    /// @return 204:
    pub fn a() {}

    /// @route GET /b
    /// @return 204:
    pub fn b() {}
}
",
    )]);
    assert_eq!(document["paths"]["/a"]["get"]["x-kept"], 1);
    assert_eq!(document["paths"]["/b"]["get"]["x-kept"], 1);
    assert_clean(&document);
    assert_eq!(
        findings(&diagnostics),
        [
            "3: error OF0009 `x-oas-forge-note` at `/x-oas-forge-note` is a reserved key: the `x-oas-forge-` prefix is the generator's own; it is removed"
        ]
    );
}

/// An `@openapi` override of a field: a forged marker on a plain field
/// does not swap its type, and a dangling reference under an arbitrary
/// reserved key is gone with the key (so the lints have nothing to hide).
#[test]
fn a_reserved_key_in_an_override_is_removed() {
    let (document, diagnostics) = pipeline(&[(
        "lib.rs",
        "\
/// @openapi
pub struct User {
    /// @openapi
    /// x-oas-forge-builtin:
    ///   name: User
    ///   schema: { type: string }
    pub swapped: u64,
    /// @openapi
    /// x-oas-forge-anything: { $ref: '#/components/schemas/Nope' }
    /// x-kept: 1
    pub leaky: u64,
}
",
    )]);
    let properties = &document["components"]["schemas"]["User"]["properties"];
    let u64 = json!({ "type": "integer", "format": "int64", "minimum": 0 });
    assert_eq!(properties["swapped"], u64);
    assert_eq!(properties["leaky"]["x-kept"], 1);
    assert_eq!(properties["leaky"]["type"], "integer");
    assert_clean(&document);
    assert_eq!(
        findings(&diagnostics),
        [
            "4: error OF0009 `x-oas-forge-builtin` at `/x-oas-forge-builtin` is a reserved key: the `x-oas-forge-` prefix is the generator's own; it is removed",
            "9: error OF0009 `x-oas-forge-anything` at `/x-oas-forge-anything` is a reserved key: the `x-oas-forge-` prefix is the generator's own; it is removed",
        ]
    );
}

/// An `@example` value is data the references stage still walks.
#[test]
fn a_reserved_key_in_an_example_is_removed() {
    let (document, diagnostics) = pipeline(&[(
        "lib.rs",
        "\
/// @openapi
pub struct E {
    /// @example { x-oas-forge-builtin: { name: E, schema: { type: string } }, kept: 1 }
    /// @example 2
    pub v: u64,
}
",
    )]);
    assert_eq!(
        document["components"]["schemas"]["E"]["properties"]["v"]["examples"],
        json!([{ "kept": 1 }, 2])
    );
    assert_clean(&document);
    assert_eq!(
        findings(&diagnostics),
        [
            "3: error OF0009 `x-oas-forge-builtin` at `/x-oas-forge-builtin` is a reserved key: the `x-oas-forge-` prefix is the generator's own; it is removed"
        ]
    );
}

/// An included file carries the template wire format in `components`
/// (read, then removed); any other key of the prefix is reserved.
#[test]
fn an_input_file_keeps_the_wire_format_and_loses_other_reserved_keys() {
    let (document, diagnostics) = pipeline(&[
        (
            "shared.yaml",
            "\
components:
  x-oas-forge-format: 1
  x-oas-forge-templates:
    Page:
      params: [T]
      body: \"items: { $ref: $T }\"
  schemas:
    S:
      type: object
      x-oas-forge-builtin: { name: S, schema: { type: string } }
paths:
  /p:
    x-oas-forge-expand: 1
    get:
      x-oas-forge-expand: 2
      responses: { '200': { description: ok } }
",
        ),
        (
            "api.rs",
            "/// @route GET /x\n/// @return 200: $Page<S>\nfn x() {}\n",
        ),
    ]);
    assert_eq!(
        document["components"]["schemas"]["S"],
        json!({ "type": "object" })
    );
    assert_eq!(
        document["components"]["schemas"]["Page_S"],
        json!({ "items": { "$ref": "#/components/schemas/S" } }),
        "the template was imported"
    );
    assert_eq!(
        document["paths"]["/p"]["get"]["responses"]["200"]["description"],
        "ok"
    );
    assert_clean(&document);
    assert_eq!(
        findings(&diagnostics),
        [
            "10: error OF0009 `x-oas-forge-builtin` at `/components/schemas/S/x-oas-forge-builtin` is a reserved key: the `x-oas-forge-` prefix is the generator's own; it is removed",
            "13: error OF0009 `x-oas-forge-expand` at `/paths/~1p/x-oas-forge-expand` and at one more place is a reserved key: the `x-oas-forge-` prefix is the generator's own; it is removed",
        ]
    );
}

/// The wire format belongs to included files, not to blocks (an
/// `@asyncapi` block included).
#[test]
fn the_wire_format_in_a_block_is_reserved() {
    let (document, diagnostics) = pipeline(&[(
        "lib.rs",
        "\
//! @openapi
//! openapi: 3.1.0
//! info: { title: T, version: 1.0.0 }
//! components:
//!   x-oas-forge-format: 1
//!
//! @asyncapi
//! asyncapi: 3.0.0
//! info: { title: A, version: 1.0.0 }
//! components:
//!   x-oas-forge-fragments: {}
",
    )]);
    assert_clean(&document);
    assert_eq!(
        findings(&diagnostics),
        [
            "5: error OF0009 `x-oas-forge-format` at `/components/x-oas-forge-format` is a reserved key: the `x-oas-forge-` prefix is the generator's own; it is removed",
            "11: error OF0009 `x-oas-forge-fragments` at `/components/x-oas-forge-fragments` is a reserved key: the `x-oas-forge-` prefix is the generator's own; it is removed",
        ]
    );
}

/// A fragment body is text until it is used: `@insert` pastes it into
/// the block (reported at the `@insert` line), `@extend` parses it on
/// its own (reported at the block, naming the fragment).
#[test]
fn a_reserved_key_in_a_fragment_is_reported_where_the_fragment_is_used() {
    let (document, diagnostics) = pipeline(&[(
        "lib.rs",
        "\
//! @openapi-fragment Forged
//! x-oas-forge-builtin: { name: S, schema: { type: string } }
//! x-frag: 1
//!
//! @openapi
//! components:
//!   schemas:
//!     Inserted:
//!       type: object
//!       @insert Forged
//!     Extended:
//!       type: object
//!       @extend Forged
//!     S: { type: object }
",
    )]);
    let schemas = &document["components"]["schemas"];
    assert_eq!(
        schemas["Inserted"],
        json!({ "type": "object", "x-frag": 1 }),
        "{document:#}\n{diagnostics:#?}"
    );
    assert_eq!(
        schemas["Extended"],
        json!({ "type": "object", "x-frag": 1 })
    );
    assert_clean(&document);
    assert_eq!(
        findings(&diagnostics),
        [
            "10: error OF0009 `x-oas-forge-builtin` at `/components/schemas/Inserted/x-oas-forge-builtin` is a reserved key: the `x-oas-forge-` prefix is the generator's own; it is removed",
            "6: error OF0009 fragment `Forged`: `x-oas-forge-builtin` at `/x-oas-forge-builtin` is a reserved key: the `x-oas-forge-` prefix is the generator's own; it is removed",
        ]
    );
}

/// Lines an `@insert` pastes shift the lines after them in the expanded
/// text: a key in the pasted lines is reported at the `@insert` line, one
/// after them at its own line.
#[test]
fn a_reserved_key_after_an_insert_is_reported_at_its_source_line() {
    let (document, diagnostics) = pipeline(&[(
        "lib.rs",
        "\
//! @openapi-fragment Paging
//! x-page: 1
//! x-size: 2
//! x-oas-forge-builtin: { name: S, schema: { type: string } }
//!
//! @openapi
//! components:
//!   schemas:
//!     A:
//!       type: object
//!       @insert Paging
//!     B:
//!       type: object
//!       x-oas-forge-unresolved: forged
//!     S: { type: object }
",
    )]);
    let schemas = &document["components"]["schemas"];
    assert_eq!(
        schemas["A"],
        json!({ "type": "object", "x-page": 1, "x-size": 2 }),
        "{document:#}\n{diagnostics:#?}"
    );
    assert_eq!(schemas["B"], json!({ "type": "object" }));
    assert_clean(&document);
    assert_eq!(
        findings(&diagnostics),
        [
            "11: error OF0009 `x-oas-forge-builtin` at `/components/schemas/A/x-oas-forge-builtin` is a reserved key: the `x-oas-forge-` prefix is the generator's own; it is removed",
            "14: error OF0009 `x-oas-forge-unresolved` at `/components/schemas/B/x-oas-forge-unresolved` is a reserved key: the `x-oas-forge-` prefix is the generator's own; it is removed",
        ]
    );
}

/// A reserved key is reported at its line also with a space before the
/// colon, quoted or in a flow mapping.
#[test]
fn a_reserved_key_is_reported_at_its_line_however_it_is_written() {
    let (_, diagnostics) = pipeline(&[(
        "lib.rs",
        "\
//! @openapi
//! components:
//!   schemas:
//!     A:
//!       x-oas-forge-unresolved : forged
//!     B: { type: object, x-oas-forge-pattern: p }
//!     C:
//!       'x-oas-forge-type-default': 1
",
    )]);
    let lines: Vec<usize> = reserved(&diagnostics)
        .iter()
        .filter_map(|d| d.location().map(|l| l.line()))
        .collect();
    assert_eq!(lines, [5, 6, 8], "{diagnostics:#?}");
}

/// The inline schema of a directive and the value of a parameter flag
/// are rejected with the directive's own code: the directive is ignored.
#[test]
fn an_inline_schema_or_flag_value_with_a_reserved_key_is_ignored() {
    let (document, diagnostics) = try_route(&[
        "@route GET /x",
        "@return 200: { x-oas-forge-builtin: { name: S, schema: { type: string } } }",
        "@query-param q: String default={x-oas-forge-builtin: {name: S, schema: {}}} deprecated",
        "@query-param e: String enum=[`{x-oas-forge-builtin: 1}`]",
        "@query-param ok: String default={x-plain: 1}",
        "@return 201: { type: object, properties: { a: { x-oas-forge-builtin: 1 } } }",
        "@return 202: { oneOf: [{ type: string }, { x-oas-forge-builtin: 1 }] }",
    ]);
    let operation = &document.expect("compiled")["paths"]["/x"]["get"];
    assert!(operation["responses"].get("200").is_none(), "{operation:#}");
    assert!(operation["responses"].get("201").is_none(), "{operation:#}");
    assert!(operation["responses"].get("202").is_none(), "{operation:#}");
    let parameters = operation["parameters"].as_array().expect("parameters");
    assert_eq!(parameters.len(), 3);
    assert!(parameters[0]["schema"].get("default").is_none());
    assert_eq!(parameters[0]["deprecated"], true);
    assert!(parameters[1]["schema"].get("enum").is_none());
    assert_eq!(parameters[2]["schema"]["default"], json!({ "x-plain": 1 }));
    assert!(!operation.to_string().contains("x-oas-forge"));
    assert_eq!(
        codes(&diagnostics),
        [
            Code::InvalidDirective,
            Code::InvalidDirective,
            Code::InvalidDirective,
            Code::InvalidDirective,
            Code::InvalidDirective
        ],
        "{diagnostics:#?}"
    );
    let messages: Vec<&str> = diagnostics.iter().map(|d| d.message()).collect();
    assert!(
        messages[0].contains("the inline schema `{ x-oas-forge-builtin: { name: S, schema: { type: string } } }` writes the reserved key `x-oas-forge-builtin`"),
        "{messages:?}"
    );
    assert!(
        messages[1].contains("`default={x-oas-forge-builtin: {name: S, schema: {}}}` writes the reserved key `x-oas-forge-builtin`"),
        "{messages:?}"
    );
    assert!(
        messages[2].contains(
            "`enum=[`{x-oas-forge-builtin: 1}`]` writes the reserved key `x-oas-forge-builtin`"
        ),
        "{messages:?}"
    );
    assert!(
        messages[3].contains("the inline schema `{ type: object, properties: { a: { x-oas-forge-builtin: 1 } } }` writes the reserved key `x-oas-forge-builtin`"),
        "{messages:?}"
    );
}

/// The embedded YAML of a channel block.
#[test]
fn a_reserved_key_in_channel_yaml_is_removed() {
    let lines = lines(&[
        "@channel live",
        "@send Tick: u8",
        "@openapi",
        "x-kept: 1",
        "x-oas-forge-builtin: { name: S, schema: {} }",
    ]);
    let mut diagnostics = Diagnostics::new();
    let numbers: Vec<usize> = (1..=lines.len()).collect();
    let compiled = compile_channel(
        ChannelSource::new(&lines, &here()).line_numbers(&numbers),
        &mut diagnostics,
    )
    .expect("compiled");
    let channel = &compiled.document["channels"]["live"];
    assert_eq!(channel["x-kept"], 1);
    assert!(channel.get("x-oas-forge-builtin").is_none(), "{channel:#}");
    assert_eq!(
        findings(&diagnostics),
        [
            "5: error OF0009 `x-oas-forge-builtin` at `/x-oas-forge-builtin` is a reserved key: the `x-oas-forge-` prefix is the generator's own; it is removed"
        ]
    );
}

/// An `@extend` marker written by hand is the one exception: the lexical
/// stage writes the same key into the text, so it cannot be told apart,
/// and it does nothing an `@extend` line would not.
#[test]
fn a_hand_written_extend_marker_is_an_extend() {
    let (document, diagnostics) = pipeline(&[(
        "lib.rs",
        "\
//! @openapi-fragment Base
//! x-base: 1

/// @openapi
/// components:
///   schemas:
///     S:
///       type: object
///       x-oas-forge-extend-9: Base
pub struct Y;
",
    )]);
    assert_eq!(
        document["components"]["schemas"]["S"],
        json!({ "type": "object", "x-base": 1 })
    );
    assert!(diagnostics.is_empty(), "{diagnostics:#?}");
}

/// The body of an imported template is the generator's and keeps its
/// markers; one that no stage resolves (here a marker nested in a
/// marker's alternative) is removed from the finished document, with a
/// finding at the origin of the schema.
#[test]
fn a_marker_an_imported_template_leaves_behind_is_swept_with_a_finding() {
    let (document, diagnostics) = pipeline(&[
        (
            "shared.yaml",
            "\
components:
  x-oas-forge-format: 1
  x-oas-forge-templates:
    Wrap:
      params: [T]
      body: |
        type: object
        properties:
          t: { $ref: $T }
        x-oas-forge-builtin:
          name: S
          schema:
            type: string
            x-oas-forge-builtin: { name: S, schema: { type: integer } }
",
        ),
        (
            "api.rs",
            "/// @openapi\npub struct S { pub a: u8 }\n\n/// @route GET /x\n/// @return 200: $Wrap<S>\nfn x() {}\n",
        ),
    ]);
    assert_eq!(
        document["components"]["schemas"]["Wrap_S"],
        json!({ "type": "string" }),
        "the alternative of the documented `S` was chosen"
    );
    assert_clean(&document);
    let found = reserved(&diagnostics);
    assert_eq!(found.len(), 1, "{diagnostics:#?}");
    assert_eq!(
        found[0].message(),
        "the generator's key `x-oas-forge-builtin` was left at `/components/schemas/Wrap_S/x-oas-forge-builtin` (a marker of an imported template that no stage resolved, or a bug of the generator); it is removed from the document"
    );
    assert!(found[0].location().is_some(), "{found:?}");
}

/// Each removed key was looked up among the keys removed before, so many
/// distinct reserved keys took time in the square of their number.
#[test]
fn many_distinct_reserved_keys_are_reported_quickly() {
    let keys: String = (0..40_000)
        .map(|i| format!("x-oas-forge-k{i}: 1\n"))
        .collect();
    let started = std::time::Instant::now();
    let (_, diagnostics) = pipeline(&[("a.yaml", &keys)]);
    assert!(
        started.elapsed() < std::time::Duration::from_secs(30),
        "{:?}",
        started.elapsed()
    );
    assert_eq!(reserved(&diagnostics).len(), 40_000);
}
