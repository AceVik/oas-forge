//! `src/lint.rs`: checks of the merged document (run by every build).

use crate::common::{Project, codes, pipeline};
use oas_forge::Generator;
use oas_forge::diagnostics::{Code, Diagnostics};

const ROOT: &str =
    "/// @openapi\n/// openapi: 3.1.0\n/// info: { title: T, version: '1' }\nfn root() {}\n";

/// The findings of a build with strict lints.
fn strict(code: &str) -> Diagnostics {
    let project = Project::new();
    project.file("src/lib.rs", &format!("{ROOT}{code}"));
    Generator::new()
        .input(project.path("src"))
        .strict_lints(true)
        .build()
        .unwrap()
        .diagnostics()
        .clone()
}

fn messages(diagnostics: &Diagnostics) -> Vec<String> {
    diagnostics
        .iter()
        .map(|d| d.message().to_string())
        .collect()
}

mod security_schemes {
    use super::*;

    #[test]
    fn undefined_schemes_are_reported_once() {
        let (_, diagnostics) = pipeline(&[(
            "lib.rs",
            r#"
            /// @openapi
            /// security: [{ token: [] }]
            /// components:
            ///   securitySchemes:
            ///     defined: { type: http, scheme: bearer }
            fn docs() {}

            /// @route GET /a
            /// @security token
            /// @security defined
            fn a() {}

            /// @route GET /b
            /// @security token + missing
            fn b() {}

            /// @route GET /c
            /// @security none
            fn c() {}
            "#,
        )]);
        assert_eq!(
            codes(&diagnostics),
            [Code::UndefinedSecurityScheme, Code::UndefinedSecurityScheme]
        );
        let messages = messages(&diagnostics);
        assert!(messages[0].contains("`token`"), "{messages:?}");
        assert!(messages[1].contains("`missing`"), "{messages:?}");
    }

    #[test]
    fn without_any_schemes_every_requirement_is_undefined() {
        let (_, diagnostics) =
            pipeline(&[("lib.rs", "/// @route GET /a\n/// @security key\nfn a() {}")]);
        assert_eq!(codes(&diagnostics), [Code::UndefinedSecurityScheme]);
    }
}

mod path_parameters {
    use super::*;

    /// Reference pointers escape `/` as `~1` (RFC 6901).
    #[test]
    fn referenced_parameters_with_escaped_names_are_found() {
        let (_, diagnostics) = pipeline(&[(
            "lib.rs",
            r#"
            /// @openapi
            /// components:
            ///   parameters:
            ///     'user/id': { name: id, in: path, required: true, schema: { type: string } }
            /// paths:
            ///   /users/{id}:
            ///     get:
            ///       parameters:
            ///         - $ref: '#/components/parameters/user~1id'
            ///       responses: { '200': { description: OK } }
            fn docs() {}
            "#,
        )]);
        assert!(
            !codes(&diagnostics).contains(&Code::PathParameterMismatch),
            "{diagnostics:?}"
        );
    }

    #[test]
    fn placeholders_and_parameters_must_match() {
        let (_, diagnostics) = pipeline(&[(
            "lib.rs",
            r#"
            /// @openapi
            /// paths:
            ///   /users/{id}:
            ///     get:
            ///       parameters:
            ///         - { name: other, in: path, required: true, schema: { type: string } }
            ///         - { name: id, in: query, schema: { type: string } }
            ///       responses: { '200': { description: OK } }
            fn docs() {}
            "#,
        )]);
        assert_eq!(
            codes(&diagnostics),
            [Code::PathParameterMismatch, Code::PathParameterMismatch]
        );
        let messages = messages(&diagnostics);
        assert!(
            messages[0].contains("`{id}` of `/users/{id}` has no path parameter"),
            "{messages:?}"
        );
        assert!(
            messages[1].contains("path parameter `other` does not appear"),
            "{messages:?}"
        );
    }

    #[test]
    fn shared_and_referenced_parameters_count() {
        let (_, diagnostics) = pipeline(&[(
            "lib.rs",
            r##"
            /// @openapi
            /// openapi: 3.2.0
            /// info: { title: T, version: '1' }
            /// paths:
            ///   /orgs/{org}/users/{id}:
            ///     parameters:
            ///       - { name: org, in: path, required: true, schema: { type: string } }
            ///     get:
            ///       parameters: [{ $ref: "#/components/parameters/Id" }]
            ///       responses: { '200': { description: OK } }
            ///     additionalOperations:
            ///       LINK:
            ///         parameters: [{ $ref: "#/components/parameters/Id" }]
            ///         responses: { '200': { description: OK } }
            ///     x-not-an-operation: {}
            /// components:
            ///   parameters:
            ///     Id: { name: id, in: path, required: true, schema: { type: string } }
            fn docs() {}
            "##,
        )]);
        assert!(diagnostics.is_empty(), "{diagnostics:?}");
    }

    #[test]
    fn every_operation_is_checked_on_its_own() {
        let (_, diagnostics) = pipeline(&[(
            "lib.rs",
            r##"
            /// @openapi
            /// paths:
            ///   /a/{x}:
            ///     get:
            ///       parameters: [{ name: x, in: path, required: true, schema: { type: string } }]
            ///       responses: { '200': { description: OK } }
            ///     delete:
            ///       parameters: [{ $ref: "#/components/parameters/Unknown" }, { $ref: "elsewhere.yaml#/X" }]
            ///       responses: { '204': { description: Gone } }
            fn docs() {}
            "##,
        )]);
        assert_eq!(
            codes(&diagnostics),
            [Code::DanglingReference, Code::PathParameterMismatch]
        );
    }
}

mod strict_lints {
    use super::*;

    #[test]
    fn documents_without_components_have_nothing_unused() {
        let diagnostics = strict("/// Lists.\n/// @route GET /a\nfn a() {}");
        assert!(diagnostics.is_empty(), "{diagnostics:?}");
    }

    #[test]
    fn malformed_sections_are_skipped() {
        let diagnostics = strict(
            r#"
            /// @openapi
            /// paths:
            ///   /a: ~
            ///   /b: { get: 5 }
            /// components:
            ///   schemas: 5
            fn docs() {}
            "#,
        );
        assert!(diagnostics.is_empty(), "{diagnostics:?}");
    }

    #[test]
    fn shared_targets_are_visited_once() {
        let diagnostics = strict(
            r#"
            /// Shared.
            /// @openapi
            struct Shared { a: u8 }
            /// Left.
            /// @openapi
            struct Left { s: Shared }
            /// Right.
            /// @openapi
            struct Right { s: Shared }

            /// Both.
            /// @route GET /a
            /// @return 200: $Left
            /// @return 201: $Right
            fn a() {}
            "#,
        );
        assert!(diagnostics.is_empty(), "{diagnostics:?}");
    }

    #[test]
    fn are_off_by_default() {
        let (_, diagnostics) = pipeline(&[(
            "lib.rs",
            "/// @openapi\nstruct Unused { a: u8 }\n/// @route GET /a\nfn a() {}",
        )]);
        assert!(diagnostics.is_empty(), "{diagnostics:?}");
    }

    #[test]
    fn report_unused_components() {
        let diagnostics = strict(
            r##"
            /// The user.
            /// @openapi
            struct User { a: u8 }
            /// Never used.
            /// @openapi
            struct Unused { next: Option<Box<Unused>> }

            /// Lists users.
            /// @route GET /users
            /// @return 200: $User
            fn users() {}

            /// @openapi
            /// components:
            ///   securitySchemes:
            ///     never: { type: http, scheme: basic }
            ///   responses:
            ///     Orphan: { description: Nobody }
            ///   x-custom:
            ///     whatever: {}
            fn docs() {}
            "##,
        );
        assert_eq!(
            codes(&diagnostics),
            [Code::UnusedComponent, Code::UnusedComponent]
        );
        let messages = messages(&diagnostics);
        assert!(
            messages[0].contains("#/components/schemas/Unused"),
            "{messages:?}"
        );
        assert!(
            messages[1].contains("#/components/responses/Orphan"),
            "{messages:?}"
        );
    }

    #[test]
    fn discriminator_mappings_count_as_uses() {
        let diagnostics = strict(
            r#"
            /// A shape.
            /// @openapi
            #[serde(tag = "kind")]
            enum Shape { Circle { r: u8 }, Square { s: u8 } }

            /// Gets a shape.
            /// @route GET /shape
            /// @return 200: $Shape
            fn shape() {}
            "#,
        );
        assert!(
            !codes(&diagnostics).contains(&Code::UnusedComponent),
            "{diagnostics:?}"
        );
    }

    #[test]
    fn components_are_used_when_reachable_from_outside_components() {
        let diagnostics = strict(
            r##"
            /// @openapi
            /// paths:
            ///   /a:
            ///     get:
            ///       summary: A
            ///       responses:
            ///         '200':
            ///           description: OK
            ///           content:
            ///             application/json:
            ///               schema: { $ref: "#/components/schemas/Outer/properties/inner" }
            /// components:
            ///   schemas:
            ///     Outer:
            ///       description: Reached through a nested pointer.
            ///       properties: { inner: { $ref: "#/components/schemas/Inner" } }
            ///     Inner: { description: Reached through Outer. }
            ///     Ping: { description: Cycle, properties: { p: { $ref: "#/components/schemas/Pong" } } }
            ///     Pong: { description: Cycle, properties: { p: { $ref: "#/components/schemas/Ping" } } }
            fn docs() {}
            "##,
        );
        assert_eq!(
            messages(&diagnostics),
            [
                "`#/components/schemas/Ping` is never referenced",
                "`#/components/schemas/Pong` is never referenced",
            ]
        );
    }

    #[test]
    fn mapping_values_may_be_schema_names() {
        let (_, diagnostics) = pipeline(&[(
            "lib.rs",
            r##"
            /// @openapi
            /// components:
            ///   schemas:
            ///     Pet:
            ///       oneOf: [{ $ref: "#/components/schemas/Cat" }]
            ///       discriminator: { propertyName: kind, mapping: { cat: Cat, dog: Dog } }
            ///     Cat: { type: object }
            fn docs() {}
            "##,
        )]);
        assert_eq!(codes(&diagnostics), [Code::DanglingReference]);
        assert!(messages(&diagnostics)[0].contains("#/components/schemas/Dog"));
    }

    #[test]
    fn report_missing_descriptions() {
        let diagnostics = strict(
            r#"
            /// @openapi
            struct Plain { a: u8 }

            /// @route GET /a
            /// @return 200: $Plain
            fn a() {}

            /// Has a summary.
            /// @route GET /b
            fn b() {}
            "#,
        );
        assert_eq!(
            codes(&diagnostics),
            [Code::MissingDescription, Code::MissingDescription]
        );
        let messages = messages(&diagnostics);
        assert!(messages[0].contains("operation get /a"), "{messages:?}");
        assert!(messages[1].contains("schema `Plain`"), "{messages:?}");
    }
}

#[test]
fn asyncapi_documents_are_linted_too() {
    let (_, diagnostics) = pipeline(&[(
        "lib.rs",
        r##"
        /// @asyncapi
        /// asyncapi: 3.0.0
        /// info: { title: Events, version: '1' }
        /// channels:
        ///   a: { messages: { m: { $ref: "#/components/messages/Missing" } } }
        fn events() {}
        "##,
    )]);
    assert_eq!(codes(&diagnostics), [Code::DanglingReference]);
}

mod references {
    use super::*;

    #[test]
    fn properties_named_mapping_are_not_discriminator_mappings() {
        let (_, diagnostics) = pipeline(&[(
            "lib.rs",
            "/// @openapi\nstruct Config { mapping: String, discriminator: String }",
        )]);
        assert!(diagnostics.is_empty(), "{diagnostics:?}");
    }

    #[test]
    fn data_values_are_not_searched_for_references() {
        let (_, diagnostics) = pipeline(&[(
            "lib.rs",
            r##"
            /// @openapi
            /// paths:
            ///   /a:
            ///     get:
            ///       responses:
            ///         '200':
            ///           description: OK
            ///           content:
            ///             application/json:
            ///               example: { $ref: "#/components/schemas/Missing" }
            ///               examples:
            ///                 one: { value: { mapping: { a: B } } }
            /// components:
            ///   schemas:
            ///     Doc:
            ///       type: object
            ///       default: { $ref: "#/components/schemas/Missing" }
            ///       enum: [{ $ref: "#/components/schemas/Missing" }]
            ///       const: { mapping: { a: Missing } }
            ///       examples: [{ discriminator: { mapping: { a: Missing } } }]
            fn docs() {}
            "##,
        )]);
        assert!(diagnostics.is_empty(), "{diagnostics:?}");
    }

    #[test]
    fn discriminator_mappings_are_still_checked() {
        let (_, diagnostics) = pipeline(&[(
            "lib.rs",
            r##"
            /// @openapi
            /// components:
            ///   schemas:
            ///     Pet:
            ///       oneOf: [{ $ref: "#/components/schemas/Cat" }]
            ///       discriminator: { propertyName: kind, mapping: { cat: Cat, dog: "#/components/schemas/Dog" } }
            ///     Cat: { type: object, properties: { mapping: { type: string } } }
            fn docs() {}
            "##,
        )]);
        assert_eq!(codes(&diagnostics), [Code::DanglingReference]);
        assert!(messages(&diagnostics)[0].contains("#/components/schemas/Dog"));
    }
}

#[test]
fn external_references_and_mappings_without_targets_are_not_components() {
    let diagnostics = strict(
        r##"
        /// @openapi
        /// paths:
        ///   /a:
        ///     get:
        ///       summary: A
        ///       responses:
        ///         '200':
        ///           description: OK
        ///           content:
        ///             application/json:
        ///               schema: { $ref: "shared.yaml#/components/schemas/Remote" }
        ///         '201':
        ///           description: Created
        ///           content:
        ///             application/json:
        ///               schema: { $ref: "#/components/schemas/Shape" }
        /// components:
        ///   schemas:
        ///     Shape:
        ///       description: Without a mapping.
        ///       discriminator: { propertyName: kind }
        fn docs() {}
        "##,
    );
    assert!(diagnostics.is_empty(), "{diagnostics:?}");
}
