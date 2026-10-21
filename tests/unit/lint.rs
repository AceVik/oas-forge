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

    /// `properties` maps names to schemas: a property may be called like a
    /// keyword whose value is data elsewhere.
    #[test]
    fn properties_named_like_data_keywords_are_schemas() {
        let (_, diagnostics) = pipeline(&[(
            "lib.rs",
            "/// @openapi\n/// components:\n///   schemas:\n///     S:\n///       type: object\n///       properties:\n///         example: { $ref: '#/components/schemas/Missing' }\nfn docs() {}",
        )]);
        assert_eq!(codes(&diagnostics), [Code::DanglingReference]);
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

/// Findings point to the annotation that defined the operation, component
/// or field they concern: the first snippet that defined it.
mod locations {
    use super::*;
    use oas_forge::diagnostics::Location;

    /// Every lint once, in a project of several files.
    fn findings() -> (Project, Vec<(Code, String, Option<Location>)>) {
        let project = Project::new();
        project
            .file(
                "src/root.rs",
                "//! @openapi\n//! openapi: 3.2.0\n//! info: { title: T, version: '1' }\n//! security: [{ rootScheme: [] }]\n",
            )
            .file(
                "src/a.rs",
                "/// @route GET /users/{id: u64}\n/// @operation-id same\n/// @security opScheme\n/// @return 200: { $ref: '#/components/schemas/Ghost' }\nfn a() {}\n",
            )
            .file(
                "src/b.rs",
                "/// @openapi\n/// paths:\n///   /items/{item}:\n///     get:\n///       operationId: same\n///       responses: { '200': { description: OK } }\n///     x-note: { $ref: '#/components/schemas/Ghost' }\nfn b() {}\n",
            )
            .file(
                "src/c.rs",
                "/// @openapi-type Broken\n/// $ref: '#/components/schemas/Nowhere'\nfn broken() {}\n\n/// @openapi\nstruct Lonely { a: u8 }\n",
            )
            .file(
                "src/d.rs",
                "/// @openapi\n/// paths:\n///   /users/{id}:\n///     get:\n///       description: Extended later\n///   /orders:\n///     additionalOperations:\n///       LIST:\n///         operationId: same\n///         responses: { '200': { description: OK } }\nfn d() {}\n",
            );
        let spec = Generator::new()
            .input(project.path("src"))
            .strict_lints(true)
            .build()
            .unwrap();
        let findings = spec
            .diagnostics()
            .iter()
            .map(|d| (d.code(), d.message().to_string(), d.location().cloned()))
            .collect();
        (project, findings)
    }

    #[test]
    fn every_lint_finding_has_the_location_of_its_part() {
        let (project, findings) = findings();
        let at = |file: &str, line: usize| Some(Location::new(project.path(file), line));
        let expected = [
            (
                Code::DanglingReference,
                "`$ref: #/components/schemas/Nowhere` points to a component that does not exist",
                at("src/c.rs", 2),
            ),
            // Also referenced in `b.rs`, later.
            (
                Code::DanglingReference,
                "`$ref: #/components/schemas/Ghost` points to a component that does not exist",
                at("src/a.rs", 1),
            ),
            // The first duplicate.
            (
                Code::DuplicateOperationId,
                "operationId `same` is used by 3 operations",
                at("src/b.rs", 2),
            ),
            (
                Code::UndefinedSecurityScheme,
                "security scheme `rootScheme` is required but not defined in `components.securitySchemes`",
                at("src/root.rs", 2),
            ),
            // The route defines the operation, `d.rs` extends it.
            (
                Code::UndefinedSecurityScheme,
                "security scheme `opScheme` is required but not defined in `components.securitySchemes`",
                at("src/a.rs", 1),
            ),
            (
                Code::PathParameterMismatch,
                "`{item}` of `/items/{item}` has no path parameter",
                at("src/b.rs", 2),
            ),
            (
                Code::UnusedComponent,
                "`#/components/schemas/Lonely` is never referenced",
                at("src/c.rs", 5),
            ),
            (
                Code::UnusedComponent,
                "`#/components/schemas/Broken` is never referenced",
                at("src/c.rs", 2),
            ),
            (
                Code::MissingDescription,
                "operation get /items/{item} has no description",
                at("src/b.rs", 2),
            ),
            (
                Code::MissingDescription,
                "operation LIST /orders has no description",
                at("src/d.rs", 2),
            ),
            (
                Code::MissingDescription,
                "schema `Lonely` has no description",
                at("src/c.rs", 5),
            ),
            (
                Code::MissingDescription,
                "schema `Broken` has no description",
                at("src/c.rs", 2),
            ),
        ]
        .map(|(code, message, location)| (code, message.to_string(), location));
        assert_eq!(findings, expected);
    }

    /// The `@channel` that refers to a missing component, not the first
    /// channel of the document.
    #[test]
    fn asyncapi_findings_point_to_their_channel() {
        let (_, diagnostics) = pipeline(&[(
            "events.rs",
            "//! @asyncapi\n//! asyncapi: 3.0.0\n//! info: { title: Events, version: '1' }\n\n/// @channel pings\n/// @send Ping: { type: string }\nfn pings() {}\n\n/// @channel events\n/// @send Event: { $ref: '#/components/schemas/Absent' }\nfn events() {}\n",
        )]);
        let finding = diagnostics.iter().next().unwrap();
        assert_eq!(finding.code(), Code::DanglingReference);
        assert_eq!(finding.location(), Some(&Location::new("events.rs", 9)));
    }

    /// Operations and webhooks of path items defined in a file, parameter
    /// mismatches of `additionalOperations` and references in arrays.
    #[test]
    fn nested_parts_inherit_the_location_of_their_part() {
        let (_, diagnostics) = pipeline(&[
            (
                "root.yaml",
                "openapi: 3.2.0\ninfo: { title: T, version: '1' }\nwebhooks:\n  ping:\n    post:\n      operationId: twice\n      responses: { '200': { description: OK } }\n",
            ),
            (
                "z.rs",
                "/// @openapi\n/// webhooks:\n///   pong:\n///     post:\n///       operationId: twice\n///       responses: { '200': { description: OK } }\n/// paths:\n///   /a/{x}:\n///     additionalOperations:\n///       PURGE:\n///         parameters: [{ $ref: '#/components/parameters/Gone' }]\n///         responses: { '200': { description: OK } }\nfn z() {}\n",
            ),
        ]);
        let located: Vec<(Code, Option<Location>)> = diagnostics
            .iter()
            .map(|d| (d.code(), d.location().cloned()))
            .collect();
        assert_eq!(
            located,
            [
                (Code::DanglingReference, Some(Location::new("z.rs", 2))),
                (Code::DuplicateOperationId, Some(Location::new("z.rs", 2))),
                (Code::PathParameterMismatch, Some(Location::new("z.rs", 2))),
            ]
        );
    }

    /// Top-level fields count as parts too, also when the root is generated
    /// from the package.
    #[test]
    fn top_level_fields_of_any_snippet_are_located() {
        let project = Project::new();
        project.file(
            "src/lib.rs",
            "/// @openapi\n/// security: [{ missing: [] }]\nfn docs() {}\n",
        );
        let spec = Generator::new()
            .input(project.path("src"))
            .package(oas_forge::Package::new("shop", "1.0.0"))
            .build()
            .unwrap();
        let finding = spec
            .diagnostics()
            .iter()
            .find(|d| d.code() == Code::UndefinedSecurityScheme)
            .unwrap();
        assert_eq!(
            finding.location(),
            Some(&Location::new(project.path("src/lib.rs"), 2))
        );
    }
}

mod channel_parameters {
    use super::*;

    const ASYNCAPI: &str =
        "//! @asyncapi\n//! asyncapi: 3.0.0\n//! info: { title: Events, version: '1' }\n\n";

    /// The findings and their lines of a build of `code` after an AsyncAPI
    /// root.
    fn findings(code: &str) -> Vec<String> {
        let project = Project::new();
        project.file("src/lib.rs", &format!("{ASYNCAPI}{code}"));
        let spec = Generator::new()
            .input(project.path("src"))
            .without_package()
            .build()
            .unwrap();
        spec.diagnostics()
            .iter()
            .map(|d| {
                let line = d.location().map_or(0, |l| l.line());
                format!("{line}: {} {}", d.code(), d.message())
            })
            .collect()
    }

    #[test]
    fn placeholders_get_parameters_from_the_address() {
        assert!(
            findings("/// @channel seat /games/{id}/ws\n/// @send A: u8\nfn seat() {}\n")
                .is_empty()
        );
    }

    #[test]
    fn yaml_parameters_must_match_the_address() {
        let found = findings(
            "/// @channel seat /games/{id}/ws\n/// @send A: u8\n/// parameters:\n///   game: { description: Unknown }\nfn seat() {}\n\
             /// @channel watch /games/{id}/watch\n/// @send B: u8\n/// address: /games/{game}/{seat}/watch\nfn watch() {}\n",
        );
        assert_eq!(
            found,
            [
                "12: OF4001 embedded YAML overrides the DSL value at `/address`",
                "5: OF5006 parameter `game` of channel `seat` does not appear in its address `/games/{id}/ws`",
                "10: OF5006 `{game}` of the address `/games/{game}/{seat}/watch` of channel `watch` has no parameter",
                "10: OF5006 `{seat}` of the address `/games/{game}/{seat}/watch` of channel `watch` has no parameter",
                "10: OF5006 parameter `id` of channel `watch` does not appear in its address `/games/{game}/{seat}/watch`",
            ]
        );
    }

    #[test]
    fn channels_in_yaml_are_checked_and_null_addresses_skipped() {
        let found = findings(
            "/// @asyncapi\n/// channels:\n///   a: { address: 'rooms/{room}' }\n///   b: { address: null, parameters: { x: {} } }\n///   c: { parameters: { y: {} } }\nfn yaml() {}\n",
        );
        assert_eq!(
            found,
            ["6: OF5006 `{room}` of the address `rooms/{room}` of channel `a` has no parameter"]
        );
    }
}
