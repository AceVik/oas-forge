//! `src/output/asyncapi.rs`: the AsyncAPI document.

use crate::common::{codes, spec, spec_of};
use oas_forge::diagnostics::Code;
use oas_forge::error::Error;
use oas_forge::output::OutputKind;
use serde_json::json;

const ROOT: &str =
    "//! @asyncapi\n//! asyncapi: 3.1.0\n//! info: { title: Events, version: '1' }\n";

#[test]
fn channels_need_an_asyncapi_root() {
    let (spec, _project) = spec_of(&[("lib.rs", "/// @channel a\n/// @send A: u8\nfn a() {}")]);
    assert!(matches!(
        spec.render(OutputKind::AsyncApi),
        Err(Error::NoAsyncApiRoot)
    ));
    let (spec, _project) = spec_of(&[("lib.rs", "/// @openapi\nstruct S;")]);
    assert!(matches!(
        spec.render(OutputKind::AsyncApi),
        Err(Error::NoAsyncApiRoot)
    ));
    // `asyncapi` without `info` is no root either.
    let (spec, _project) = spec_of(&[(
        "lib.rs",
        "//! @asyncapi\n//! asyncapi: 3.1.0\n\n/// @channel a\n/// @send A: u8\nfn a() {}",
    )]);
    assert!(matches!(
        spec.render(OutputKind::AsyncApi),
        Err(Error::NoAsyncApiRoot)
    ));
}

#[test]
fn schemas_used_twice_are_shared_once() {
    let (spec, _project) = spec_of(&[(
        "lib.rs",
        &format!(
            "{ROOT}\n/// @openapi\nstruct User {{ a: u8 }}\n/// @channel a\n/// @send A: $User\nfn a() {{}}\n/// @channel b\n/// @receive B: Vec<$User>\nfn b() {{}}"
        ),
    )]);
    let document = spec.render(OutputKind::AsyncApi).unwrap();
    assert_eq!(
        document["components"]["schemas"]
            .as_object()
            .unwrap()
            .keys()
            .collect::<Vec<_>>(),
        ["User"]
    );
}

#[test]
fn only_the_schemas_messages_use_are_shared() {
    let (spec, _project) = spec_of(&[(
        "lib.rs",
        &format!(
            "{ROOT}\n/// @openapi\nstruct User {{ address: Address }}\n/// @openapi\nstruct Address {{ zip: String }}\n/// @openapi\nstruct Unused {{ a: u8 }}\n/// @channel users\n/// @send Created: $User\nfn a() {{}}"
        ),
    )]);
    let (document, diagnostics) = spec.render_with_diagnostics(OutputKind::AsyncApi).unwrap();
    assert!(diagnostics.is_empty(), "{diagnostics:?}");
    let schemas: Vec<_> = document["components"]["schemas"]
        .as_object()
        .unwrap()
        .keys()
        .cloned()
        .collect();
    assert_eq!(schemas, ["Address", "User"]);
    assert!(
        spec::validate(&document).is_empty(),
        "{:?}",
        spec::validate(&document)
    );
    assert!(
        spec.document()["components"]["schemas"]
            .get("Unused")
            .is_some()
    );
    assert!(
        spec.document().get("channels").is_none(),
        "AsyncAPI blocks stay out of OpenAPI"
    );
}

#[test]
fn own_schemas_win_and_unknown_references_stay() {
    let (spec, _project) = spec_of(&[(
        "lib.rs",
        &format!(
            "{ROOT}//! components: {{ schemas: {{ User: {{ type: string }} }} }}\n\n/// @openapi\nstruct User {{ a: Other }}\n/// @openapi\nstruct Other {{ b: u8 }}\n/// @channel users\n/// @send Created: $User\n/// @send Raw: {{ $ref: '#/components/schemas/Ghost' }}\nfn a() {{}}"
        ),
    )]);
    let document = spec.render(OutputKind::AsyncApi).unwrap();
    assert_eq!(
        document["components"]["schemas"]["User"],
        json!({ "type": "string" })
    );
    assert!(
        document["components"]["schemas"].get("Other").is_some(),
        "dependencies of a shadowed schema are still copied"
    );
    assert_eq!(codes(spec.diagnostics()), [Code::DanglingReference]);
}

#[test]
fn asyncapi_files_and_multiple_roots() {
    let (spec, _project) = spec_of(&[
        (
            "events.yaml",
            "asyncapi: 3.1.0\ninfo: { title: E, version: '1' }\nchannels: { a: { address: a } }",
        ),
        ("lib.rs", "/// @openapi\nstruct S;"),
    ]);
    assert_eq!(
        spec.render(OutputKind::AsyncApi).unwrap()["channels"]["a"]["address"],
        "a"
    );

    let project = crate::common::Project::new();
    project
        .file(
            "src/a.yaml",
            "asyncapi: 3.1.0\ninfo: { title: A, version: '1' }",
        )
        .file("src/b.rs", ROOT);
    let result = oas_forge::Generator::new()
        .input(project.path("src"))
        .build();
    assert!(
        matches!(result, Err(Error::MultipleRootsFound { .. })),
        "{result:?}"
    );
}

#[test]
fn schemas_follow_the_asyncapi_dialect() {
    let (spec, _project) = spec_of(&[(
        "lib.rs",
        &format!(
            "{ROOT}\n/// @openapi\nstruct P {{ t: (u8, String), #[serde(flatten)] m: Option<M>, #[serde(skip_serializing)] w: u8 }}\n/// @openapi\n#[serde(deny_unknown_fields)]\nstruct M {{ #[serde(flatten)] x: X }}\n/// @openapi\n#[serde(tag = \"k\")]\nenum X {{ A {{ a: u8 }} }}\n/// @channel p\n/// @send P: $P\nfn a() {{}}"
        ),
    )]);
    let (document, diagnostics) = spec.render_with_diagnostics(OutputKind::AsyncApi).unwrap();
    let schemas = &document["components"]["schemas"];
    assert!(schemas["P"]["properties"]["t"]["items"].is_array());
    assert_eq!(schemas["P"]["properties"]["t"]["additionalItems"], false);
    assert_eq!(schemas["X"]["discriminator"], "k");
    assert!(schemas["M"].get("unevaluatedProperties").is_none());
    assert_eq!(codes(&diagnostics), [Code::LossyLowering]);
    assert!(
        spec::validate(&document).is_empty(),
        "{:?}",
        spec::validate(&document)
    );
}

/// Reference pointers escape `/` as `~1` (RFC 6901).
#[test]
fn shared_schemas_are_found_by_escaped_references() {
    let (spec, _project) = spec_of(&[(
        "lib.rs",
        &format!(
            "{ROOT}//! channels:\n//!   a:\n//!     messages:\n//!       m:\n//!         payload: {{ $ref: '#/components/schemas/v1~1User' }}\n\n/// @openapi\n/// components:\n///   schemas:\n///     v1/User: {{ type: object }}\nfn docs() {{}}"
        ),
    )]);
    let document = spec.render(OutputKind::AsyncApi).unwrap();
    assert_eq!(
        document["components"]["schemas"]["v1/User"],
        json!({ "type": "object" })
    );
}

/// A WebSocket channel whose YAML refers to schemas with smart references.
const WEBSOCKET: &str = r#"
/// Pending work.
/// @openapi
struct Pending { jobs: Vec<Job> }

/// A job.
/// @openapi
struct Job { id: u64 }

/// A filter.
/// @openapi
#[derive(Deserialize)]
enum Filter { All, Mine }

/// Live updates
/// @channel live
/// @ws-query filter: $Filter "What to watch"
/// @ws-header Authorization: String
/// @send Update: application/json
/// @openapi message Update
/// payload: { $ref: $Pending }
/// @openapi
/// bindings: { ws: { method: GET } }
fn live() {}
"#;

#[test]
fn smart_references_in_channel_yaml_resolve_and_share_their_schemas() {
    let (spec, _project) = spec_of(&[("lib.rs", &format!("{ROOT}{WEBSOCKET}"))]);
    assert!(spec.diagnostics().is_empty(), "{:#?}", spec.diagnostics());
    let document = spec.render(OutputKind::AsyncApi).unwrap();
    assert_eq!(
        document["components"]["messages"]["Update"],
        json!({
            "name": "Update",
            "contentType": "application/json",
            "payload": { "$ref": "#/components/schemas/Pending" },
        })
    );
    assert_eq!(
        document["channels"]["live"]["bindings"]["ws"]["query"]["properties"]["filter"],
        json!({ "$ref": "#/components/schemas/Filter", "description": "What to watch" })
    );
    let schemas = document["components"]["schemas"].as_object().unwrap();
    assert_eq!(
        schemas.keys().collect::<Vec<_>>(),
        ["Filter", "Job", "Pending"]
    );
    assert!(
        spec::validate(&document).is_empty(),
        "{:?}",
        spec::validate(&document)
    );
}

#[test]
fn schemas_of_channel_yaml_survive_the_pruning_of_a_scope() {
    let project = crate::common::Project::new();
    project.file("src/lib.rs", &format!("{ROOT}{WEBSOCKET}"));
    let spec = oas_forge::Generator::new()
        .input(project.path("src"))
        .without_package()
        .scope("public")
        .build()
        .unwrap();
    let document = spec.render(OutputKind::AsyncApi).unwrap();
    let schemas = document["components"]["schemas"].as_object().unwrap();
    assert_eq!(
        schemas.keys().collect::<Vec<_>>(),
        ["Filter", "Job", "Pending"]
    );
    // The OpenAPI document of the scope has no operation that uses them.
    assert!(spec.document().get("components").is_none());
}

const PROTO: &str = "syntax = \"proto3\";\n\npackage baylee.v1;\n\n// A frame on the socket.\nmessage Envelope {\n  oneof body {\n    string text = 1;\n    bytes data = 2;\n  }\n}\n";

#[test]
fn proto_payloads_are_valid_multi_format_schemas() {
    let project = crate::common::Project::new();
    project
        .file(
            "src/lib.rs",
            &format!(
                "{ROOT}\n/// Frames\n/// @channel frames\n/// @send Envelope: proto(baylee.v1.Envelope)\nfn frames() {{}}\n"
            ),
        )
        .file("src/proto/baylee.proto", PROTO);
    let spec = oas_forge::Generator::new()
        .input(project.path("src"))
        .package(oas_forge::Package::new("shop", "1.0.0"))
        .build()
        .unwrap();
    assert_eq!(codes(spec.diagnostics()), [Code::GeneratedRoot]);
    let (document, findings) = spec.render_with_diagnostics(OutputKind::AsyncApi).unwrap();
    assert!(findings.is_empty(), "{findings:#?}");
    assert_eq!(
        document["components"]["messages"]["Envelope"]["payload"],
        json!({ "schemaFormat": "application/vnd.google.protobuf;version=3", "schema": PROTO })
    );
    assert!(document["components"].get("schemas").is_none());
    assert!(
        spec::validate(&document).is_empty(),
        "{:?}",
        spec::validate(&document)
    );
    // The OpenAPI documents know nothing of it.
    let openapi = spec.render(OutputKind::Full).unwrap();
    assert!(!openapi.to_string().contains("protobuf"), "{openapi:#}");
}

#[test]
fn broken_proto_files_and_unknown_messages_are_reported() {
    let project = crate::common::Project::new();
    project
        .file(
            "src/lib.rs",
            &format!("{ROOT}\n/// @channel frames\n/// @send Envelope: proto(baylee.v1.Envelope)\nfn frames() {{}}\n"),
        )
        .file("src/baylee.proto", "package baylee.v1;\nmessage Envelope {\n");
    let spec = oas_forge::Generator::new()
        .input(project.path("src"))
        .without_package()
        .build()
        .unwrap();
    assert_eq!(
        codes(spec.diagnostics()),
        [Code::InvalidProto, Code::UnknownProtoMessage]
    );
    let finding = spec.diagnostics().iter().next().unwrap();
    assert!(finding.to_string().contains("baylee.proto:2"), "{finding}");
}

/// Message headers are schemas too: they are lowered like payloads, in
/// channel messages, component messages and message traits.
#[test]
fn message_headers_follow_the_asyncapi_dialect() {
    let headers = "{ type: object, properties: { trace: { type: array, prefixItems: [{ type: string }] } }, $defs: { Id: { type: string } }, unevaluatedProperties: false }";
    let (spec, _project) = spec_of(&[(
        "lib.rs",
        &format!(
            "{ROOT}//! channels:\n//!   a:\n//!     address: a\n//!     messages:\n//!       m:\n//!         headers: {headers}\n//!         payload: {{ type: string }}\n//! components:\n//!   messages:\n//!     M:\n//!       headers: {headers}\n//!   messageTraits:\n//!     T:\n//!       headers: {headers}\n"
        ),
    )]);
    let (document, diagnostics) = spec.render_with_diagnostics(OutputKind::AsyncApi).unwrap();
    let lowered = json!({
        "type": "object",
        "properties": { "trace": { "type": "array", "items": [{ "type": "string" }] } },
        "definitions": { "Id": { "type": "string" } },
    });
    assert_eq!(
        document["channels"]["a"]["messages"]["m"]["headers"],
        lowered
    );
    assert_eq!(document["components"]["messages"]["M"]["headers"], lowered);
    assert_eq!(
        document["components"]["messageTraits"]["T"]["headers"],
        lowered
    );
    assert_eq!(codes(&diagnostics), [Code::LossyLowering]);
    assert!(
        spec::validate(&document).is_empty(),
        "{:?}",
        spec::validate(&document)
    );
}
