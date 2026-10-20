//! The full pipeline (extract → macros → fragments → DSL → generics →
//! references → merge) on a realistic multi-file project.

use crate::common::Project;
use oas_forge::Generator;
use serde_json::{Value, json};

fn generate(project: &Project) -> Value {
    Generator::new()
        .input(project.path("src"))
        .output(project.path("openapi.yaml"))
        .generate()
        .expect("generation should succeed");
    project.read_yaml("openapi.yaml")
}

fn api_project() -> Project {
    let project = Project::new();
    project
        .file(
            "src/lib.rs",
            r#"
//! @openapi
//! openapi: 3.1.0
//! info:
//!   title: Shop API
//!   version: '{{CARGO_PKG_VERSION}}'
//!   description: Prices are in \$ (USD)

//! @openapi-fragment CommonError(code)
//! description: Error {{code}}
//! content:
//!   application/json:
//!     schema:
//!       $ref: $ErrorModel

//! @openapi-fragment NotFoundBase
//! responses:
//!   '404':
//!     description: Not Found
"#,
        )
        .file(
            "src/models.rs",
            r#"
/// @openapi<T>
/// type: object
/// properties:
///   data:
///     $ref: $T
struct Wrapper<T>(T);

/// Error payload
/// @openapi
#[serde(rename_all = "camelCase")]
pub struct ErrorModel {
    pub error_code: u16,
    pub message: Option<String>,
}

/// @openapi
pub struct User {
    pub id: Uuid,
    #[validate(email)]
    pub email: String,
}
"#,
        )
        .file(
            "src/routes.rs",
            r#"
/// Fetch a user
/// @route GET /users/{id: Uuid "User ID"}
/// @tag Users
/// @return 200: $Wrapper<User> "The user"
/// @return 404: "Unknown user"
/// @security bearer
pub fn get_user() {}

/// @openapi
/// paths:
///   /legacy:
///     get:
///       parameters:
///         @insert QueryParam
///       @extend NotFoundBase
///       responses:
///         '200':
///           description: OK
///         '400':
///           @insert CommonError("Bad Request")
///         '401':
///           @insert CommonError
pub fn legacy() {}
"#,
        );
    project
}

#[test]
fn root_document_is_finalized() {
    let spec = generate(&api_project());
    assert_eq!(spec["openapi"], "3.1.0");
    assert_eq!(
        spec["info"],
        json!({
            "title": "Shop API",
            "version": std::env::var("CARGO_PKG_VERSION").unwrap_or_else(|_| "0.0.0".into()),
            "description": "Prices are in $ (USD)"
        })
    );
}

#[test]
fn reflected_models_are_registered() {
    let schemas = &generate(&api_project())["components"]["schemas"];
    assert_eq!(
        schemas["ErrorModel"],
        json!({
            "type": "object",
            "description": "Error payload",
            "properties": {
                "errorCode": { "type": "integer", "format": "int32" },
                "message": { "type": "string" }
            },
            "required": ["errorCode"]
        })
    );
    assert_eq!(schemas["User"]["properties"]["email"]["format"], "email");
}

#[test]
fn dsl_route_is_compiled_with_resolved_generics() {
    let spec = generate(&api_project());
    let op = &spec["paths"]["/users/{id}"]["get"];

    assert_eq!(op["operationId"], "get_user");
    assert_eq!(op["summary"], "Fetch a user");
    assert_eq!(op["tags"], json!(["Users"]));
    assert_eq!(op["security"], json!([{ "bearer": [] }]));
    assert_eq!(op["parameters"][0]["schema"]["format"], "uuid");
    assert_eq!(
        op["responses"]["200"]["content"]["application/json"]["schema"],
        json!({ "$ref": "#/components/schemas/Wrapper_User" })
    );
    assert!(op["responses"]["404"].get("content").is_none());

    assert_eq!(
        spec["components"]["schemas"]["Wrapper_User"],
        json!({
            "type": "object",
            "properties": { "data": { "$ref": "#/components/schemas/User" } }
        })
    );
}

#[test]
fn legacy_block_expands_fragments_and_macros() {
    let spec = generate(&api_project());
    let op = &spec["paths"]["/legacy"]["get"];

    assert_eq!(
        op["parameters"],
        json!([{ "$ref": "#/components/parameters/QueryParam" }])
    );
    assert_eq!(op["responses"]["200"]["description"], "OK");
    assert_eq!(op["responses"]["404"]["description"], "Not Found");
    assert_eq!(
        op["responses"]["400"],
        json!({
            "description": "Error Bad Request",
            "content": { "application/json": { "schema": {
                "$ref": "#/components/schemas/ErrorModel"
            }}}
        })
    );
    assert_eq!(
        op["responses"]["401"]["description"], "Error {{code}}",
        "missing fragment arguments keep their placeholder"
    );
}

#[test]
fn output_contains_no_nulls_or_unresolved_markers() {
    let project = api_project();
    generate(&project);
    let text = project.read("openapi.yaml");

    for marker in [
        "null",
        "@insert",
        "@extend",
        "x-openapi-extend",
        "$Wrapper",
        "\\$",
    ] {
        assert!(!text.contains(marker), "`{marker}` leaked into:\n{text}");
    }
}

#[test]
fn non_ascii_documentation_survives_the_pipeline() {
    // Regression: multi-byte characters before a `$Ref` broke the monomorphizer.
    let project = Project::new();
    project.file(
        "src/lib.rs",
        r#"
/// @openapi
pub struct Adresse { pub ort: String }

/// Kundendaten für die Größenberechnung – „ÄÖÜ“
/// @openapi
pub struct Kunde {
    /// Wohnort des Kunden
    pub adresse: Adresse,
}
"#,
    );

    Generator::new()
        .input(project.path("src"))
        .output_schemas(project.path("schemas.yaml"))
        .generate()
        .expect("non-ASCII docs must not break generation");

    let kunde = &project.read_yaml("schemas.yaml")["Kunde"];
    assert_eq!(
        kunde["description"],
        "Kundendaten für die Größenberechnung – „ÄÖÜ“"
    );
    assert_eq!(
        kunde["properties"]["adresse"],
        json!({ "$ref": "#/components/schemas/Adresse", "description": "Wohnort des Kunden" })
    );
}

/// Regressions: a stray snippet used to replace the whole merged specification.
mod no_silent_data_loss {
    use super::*;
    use oas_forge::error::Error;

    fn schemas_and_paths(project: &Project) -> (Value, Value) {
        Generator::new()
            .input(project.path("src"))
            .output_schemas(project.path("schemas.yaml"))
            .output_paths(project.path("paths.yaml"))
            .generate()
            .expect("generation should succeed");
        (
            project.read_yaml("schemas.yaml"),
            project.read_yaml("paths.yaml"),
        )
    }

    #[test]
    fn empty_openapi_block_keeps_everything_else() {
        let project = Project::new();
        project.file(
            "src/lib.rs",
            "/// @openapi\npub struct Keep { pub id: i32 }\n\n/// @openapi\nfn empty_block() {}\n",
        );

        let (schemas, _) = schemas_and_paths(&project);

        assert!(schemas["Keep"].is_object(), "{schemas:#}");
    }

    #[test]
    fn summary_line_above_openapi_block_keeps_everything_else() {
        let project = Project::new();
        project.file(
            "src/lib.rs",
            r#"
/// @openapi
pub struct Keep { pub id: i32 }

/// Health check
/// @openapi
/// paths:
///   /health:
///     get:
///       description: ok
fn health() {}
"#,
        );

        let (schemas, paths) = schemas_and_paths(&project);

        assert!(schemas["Keep"].is_object(), "{schemas:#}");
        assert_eq!(paths["/health"]["get"]["description"], "ok");
    }

    #[test]
    fn plain_text_block_is_reported_with_its_location() {
        let project = Project::new();
        project.file(
            "src/lib.rs",
            "\n\n/// @openapi\n/// This endpoint is documented elsewhere.\nfn odd() {}\n",
        );

        let result = Generator::new()
            .input(project.path("src"))
            .output_schemas(project.path("schemas.yaml"))
            .generate();

        match result {
            Err(error @ Error::SourceMapped { .. }) => {
                let message = error.to_string();
                assert!(
                    message.starts_with(&format!(
                        "YAML error in {}:3: an OpenAPI snippet must be a mapping at the top level, found a string",
                        project.path("src/lib.rs").display()
                    )),
                    "{message}"
                );
                assert!(
                    message.ends_with("    03 | This endpoint is documented elsewhere."),
                    "{message}"
                );
            }
            other => panic!("expected a located error, got {other:?}"),
        }
    }
}
