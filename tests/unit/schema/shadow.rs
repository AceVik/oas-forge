//! `src/schema/shadow.rs`: documented schemas replacing built-in names.
//!
//! Which schema is chosen is tested with the structs
//! (`extract::structs::builtin_names`) and the route DSL (`generator`);
//! here: what the alternatives cost, and that what the stages do to a
//! schema before the choice reaches the alternative.

use crate::common::{extract, pipeline, schema};
use serde_json::{Value, json};

/// The `Job` schema of a pipeline over `code`, which must be clean.
fn job(code: &str) -> Value {
    let (document, diagnostics) = pipeline(&[("lib.rs", code)]);
    assert!(diagnostics.is_empty(), "{diagnostics:?}");
    let text = document.to_string();
    assert!(!text.contains("x-oas-forge-builtin"), "{text}");
    document["components"]["schemas"]["Job"].clone()
}

/// A type naming six built-in types bare next to a large type: the
/// alternatives cost one reference per name, not a copy of the whole type
/// per subset of the names (63 copies of each field's schema took 2 GB
/// and 5 s for 1000 such fields).
#[test]
fn alternatives_cost_one_reference_per_name() {
    let nested = format!("{}u8{}", "Vec<".repeat(20), ">".repeat(20));
    let ty = format!("(Duration, SystemTime, Uuid, Url, Decimal, Ulid, {nested})");
    let fields: String = (0..200).map(|i| format!("    pub f{i}: {ty},\n")).collect();
    let code = format!("/// @openapi\npub struct Job {{\n{fields}}}");
    let extracted = schema(&extract(&code), "Job").to_string();
    let output = job(&code).to_string();
    assert!(
        extracted.len() < 2 * output.len(),
        "{} bytes extracted for {} bytes of output",
        extracted.len(),
        output.len()
    );
}

/// An `@openapi` override of the items of a `Vec<Duration>` field reaches
/// the alternative of the items.
#[test]
fn an_override_of_the_items_reaches_the_alternative() {
    let job = job(
        "/// @openapi\npub struct Duration { pub minutes: u32 }\n/// @openapi\npub struct Job {\n    /// @openapi\n    /// items: { description: One step }\n    pub steps: Vec<Duration>,\n}",
    );
    assert_eq!(
        job["properties"]["steps"]["items"],
        json!({ "$ref": "#/components/schemas/Duration", "description": "One step" })
    );
}

/// A garde `inner` rule on a `Vec<Duration>` field constrains the
/// alternative of the items.
#[test]
fn inner_constraints_reach_the_alternative() {
    let job = job(
        "/// @openapi\npub struct Duration { pub minutes: u32 }\n/// @openapi\npub struct Job {\n    #[garde(inner(range(min = 1)))]\n    pub steps: Vec<Duration>,\n}",
    );
    assert_eq!(
        job["properties"]["steps"]["items"],
        json!({ "$ref": "#/components/schemas/Duration", "minimum": 1 })
    );
}

/// A built-in name written bare in the target of a type alias of an
/// `Option` follows the documented types like one written in the field.
#[test]
fn a_name_in_the_target_of_an_option_alias_follows_documented_types() {
    let job = job(
        "/// @openapi\npub struct Duration { pub minutes: u32 }\npub type MaybeDuration = Option<Duration>;\n/// @openapi\npub struct Job { pub timeout: MaybeDuration }",
    );
    assert_eq!(
        job["properties"]["timeout"],
        json!({ "anyOf": [{ "$ref": "#/components/schemas/Duration" }, { "type": "null" }] })
    );
}

/// A second `@return` of a code merges into the first, later values
/// winning, alternative included: the documented schema of the second type
/// is chosen, and no alternative leaks into the output.
#[test]
fn a_repeated_return_brings_its_own_alternative() {
    let route = "/// Op\n/// @route GET /p\n/// @return 200: Uuid \"a\"\n/// @return 200: Duration \"b\"\npub async fn op() {}";
    let schema = |code: &str| {
        let (document, _) = pipeline(&[("lib.rs", code)]);
        let text = document.to_string();
        assert!(!text.contains("x-oas-forge-builtin"), "{text}");
        document["paths"]["/p"]["get"]["responses"]["200"]["content"]["application/json"]["schema"]
            .clone()
    };
    assert_eq!(
        schema(&format!(
            "/// @openapi\npub struct Duration {{ pub minutes: u32 }}\n{route}"
        )),
        json!({ "$ref": "#/components/schemas/Duration" })
    );
    assert_eq!(schema(route)["type"], "object");
}

/// A `serde(with)` module replaces the type's schema, also the alternative:
/// the field keeps the module's schema although `DateTime` is documented.
#[test]
fn a_serde_with_module_leaves_no_alternative() {
    let code = "/// @openapi\npub struct DateTime { pub epoch: u64 }\n/// @openapi\npub struct Job {\n    #[serde(with = \"chrono::serde::ts_seconds\")]\n    pub at: DateTime,\n}";
    let at = json!({ "type": "integer", "format": "int64" });
    assert_eq!(schema(&extract(code), "Job")["properties"]["at"], at);
    assert_eq!(job(code)["properties"]["at"], at);
}
