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
