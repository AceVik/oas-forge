//! Template transport: one crate exports fragments, another one includes them.

use crate::common::{Project, ROOT_DOC};
use oas_forge::Generator;
use serde_json::json;

#[test]
fn templates_and_fragments_travel_between_crates() {
    let shared = Project::new();
    shared.file(
        "src/lib.rs",
        r#"
//! @openapi-fragment Problem(status)
//! description: Problem {{status}}

/// @openapi<T>
/// type: object
/// properties:
///   items:
///     type: array
///     items: { $ref: $T }
pub struct Paginated<T> { pub total: u64 }

/// @openapi
pub struct Money { pub cents: i64 }
"#,
    );
    Generator::new()
        .input(shared.path("src"))
        .output_fragments(shared.path("shared.yaml"))
        .generate()
        .unwrap();

    let service = Project::new();
    service.file(
        "src/main.rs",
        &format!(
            r#"{ROOT_DOC}
/// @openapi
pub struct Invoice {{ pub amount: Money }}

/// @route GET /invoices
/// @return 200: $Paginated<Invoice> "Invoices"
fn list_invoices() {{}}

/// @openapi
/// paths:
///   /invoices/{{id}}:
///     get:
///       responses:
///         '500':
///           @insert Problem("internal")
fn invoice() {{}}
"#
        ),
    );
    Generator::new()
        .input(service.path("src"))
        .include(shared.path("shared.yaml"))
        .output(service.path("openapi.yaml"))
        .generate()
        .unwrap();

    let spec = service.read_yaml("openapi.yaml");
    let schemas = &spec["components"]["schemas"];

    assert_eq!(
        spec["paths"]["/invoices"]["get"]["responses"]["200"]["content"]["application/json"]["schema"],
        json!({ "$ref": "#/components/schemas/Paginated_Invoice" })
    );
    assert_eq!(
        schemas["Paginated_Invoice"]["properties"]["items"]["items"],
        json!({ "$ref": "#/components/schemas/Invoice" })
    );
    assert_eq!(
        schemas["Invoice"]["properties"]["amount"],
        json!({ "$ref": "#/components/schemas/Money" }),
        "schemas imported from the shared crate resolve as smart references"
    );
    assert!(schemas["Money"].is_object());
    assert_eq!(
        spec["paths"]["/invoices/{id}"]["get"]["responses"]["500"]["description"],
        "Problem internal"
    );
    assert!(!service.read("openapi.yaml").contains("x-oas-forge"));
}

/// What a workspace usually does: a model crate documents plain serde types
/// (including a generic one), an API crate uses them by name, by path, as
/// generic arguments and in handler signatures.
#[test]
fn reflected_types_generics_and_signatures_work_across_crates() {
    let models = Project::new();
    models.file(
        "src/lib.rs",
        r#"
use serde::{Deserialize, Serialize};

/// A user account.
/// @openapi
#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct User {
    pub id: u64,
    pub display_name: String,
    pub role: Role,
}

/// @openapi
#[derive(Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Role { Admin, Member }

/// One page of results.
/// @openapi
#[derive(Serialize)]
pub struct Page<T> {
    pub items: Vec<T>,
    pub total: u64,
}
"#,
    );
    Generator::new()
        .input(models.path("src"))
        .output_fragments(models.path("fragments.yaml"))
        .generate()
        .unwrap();

    let api = Project::new();
    api.file(
        "src/main.rs",
        &format!(
            r#"{ROOT_DOC}
use models::{{Page, User}};

/// @openapi
pub struct Team {{
    pub lead: models::User,
    pub members: Vec<User>,
}}

/// @route GET /users
/// @return 200: $Page<User>
pub async fn list_users() {{}}

/// @route GET /users/{{id}}
pub async fn get_user(Path(id): Path<u64>) -> Json<models::User> {{ todo!() }}

/// @route GET /teams
pub async fn teams() -> Json<Page<Team>> {{ todo!() }}
"#
        ),
    );
    let spec = Generator::new()
        .input(api.path("src"))
        .include(models.path("fragments.yaml"))
        .build()
        .unwrap();
    let document = spec.document();
    let schemas = &document["components"]["schemas"];
    let ok = |path: &str| {
        document["paths"][path]["get"]["responses"]["200"]["content"]["application/json"]["schema"]
            .clone()
    };

    assert!(
        spec.diagnostics().is_empty(),
        "{:?}",
        spec.diagnostics()
            .iter()
            .map(ToString::to_string)
            .collect::<Vec<_>>()
    );
    assert_eq!(
        ok("/users"),
        json!({ "$ref": "#/components/schemas/Page_User" })
    );
    assert_eq!(
        ok("/users/{id}"),
        json!({ "$ref": "#/components/schemas/User" })
    );
    assert_eq!(
        ok("/teams"),
        json!({ "$ref": "#/components/schemas/Page_Team" })
    );
    assert_eq!(
        schemas["Page_User"]["properties"]["items"]["items"],
        json!({ "$ref": "#/components/schemas/User" })
    );
    assert_eq!(
        schemas["Team"]["properties"]["lead"],
        json!({ "$ref": "#/components/schemas/User" }),
        "a path to another crate's type names its schema"
    );
    assert_eq!(
        schemas["User"]["properties"]["displayName"]["type"],
        "string"
    );
    assert_eq!(schemas["Role"]["enum"], json!(["admin", "member"]));
    assert!(crate::common::spec::is_valid(document));
}

/// Crates still built with oas-forge 0.1 publish fragments without a format
/// marker; 0.2 reads them (ADR 0005).
#[test]
fn fragments_written_by_oas_forge_0_1_are_read() {
    let shared = Project::new();
    shared.file(
        "fragments.yaml",
        r#"components:
  schemas:
    User:
      type: object
      properties:
        id:
          type: integer
          format: int64
      required:
      - id
  x-oas-forge-templates:
    Page:
      params:
      - T
      body: |
        type: object
        properties:
          items:
            type: array
            items:
              $ref: $T
        required:
        - items
  x-oas-forge-fragments:
    Paging:
      params: []
      body: |-
        - name: page
          in: query
          schema: { type: integer, minimum: 1 }
"#,
    );
    let api = Project::new();
    api.file(
        "src/main.rs",
        &format!(
            r#"{ROOT_DOC}
/// @route GET /users
/// parameters:
///   - @insert Paging
/// @return 200: $Page<User>
pub async fn list_users() {{}}
"#
        ),
    );
    let spec = Generator::new()
        .input(api.path("src"))
        .include(shared.path("fragments.yaml"))
        .build()
        .unwrap();
    let document = spec.document();
    let operation = &document["paths"]["/users"]["get"];

    assert!(spec.diagnostics().is_empty(), "{:?}", spec.diagnostics());
    assert_eq!(operation["parameters"][0]["name"], "page");
    assert_eq!(
        document["components"]["schemas"]["Page_User"]["properties"]["items"]["items"],
        json!({ "$ref": "#/components/schemas/User" })
    );
    assert!(crate::common::spec::is_valid(document));
}
