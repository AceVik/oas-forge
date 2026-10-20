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
