//! `src/dsl/expand.rs`: struct expansion of parameters (`@query-params`,
//! `@header-params`, inferred `Query<T>`).

use crate::common::pipeline;
use serde_json::{Value, json};

/// The parameters of `GET /x` with the struct `Filter` below expanded by
/// `directive` (or by the handler's `Query<Filter>` with `None`).
fn expanded(directive: Option<&str>) -> Vec<Value> {
    let route = match directive {
        Some(directive) => format!("/// @route GET /x\n/// {directive}\nfn x() {{}}"),
        None => "/// @route GET /x\nasync fn x(Query(f): Query<Filter>) {}".to_string(),
    };
    let (document, diagnostics) = pipeline(&[(
        "api.rs",
        &format!(
            r#"
/// @openapi
pub struct Filter {{
    /// Old name of `q`.
    #[deprecated]
    pub search: Option<String>,
    /// Page size.
    /// @example 20
    pub limit: Option<u32>,
    /// @openapi deprecated: true
    /// @example "a"
    /// @example "b"
    pub sort: Option<String>,
    /// @openapi
    /// example: 7
    pub page: Option<u32>,
}}

{route}
"#
        ),
    )]);
    assert!(diagnostics.is_empty(), "{diagnostics:#?}");
    document["paths"]["/x"]["get"]["parameters"]
        .as_array()
        .unwrap()
        .clone()
}

#[test]
fn parameter_fields_are_lifted_out_of_the_schema() {
    for directive in [Some("@query-params $Filter"), None] {
        let parameters = expanded(directive);
        let by_name = |name: &str| {
            parameters
                .iter()
                .find(|p| p["name"] == name)
                .unwrap_or_else(|| panic!("no {name} in {parameters:#?}"))
                .clone()
        };
        assert_eq!(
            by_name("search"),
            json!({
                "name": "search",
                "in": "query",
                "required": false,
                "description": "Old name of `q`.",
                "deprecated": true,
                "schema": { "type": "string" },
            }),
            "{directive:?}"
        );
        assert_eq!(
            by_name("limit"),
            json!({
                "name": "limit",
                "in": "query",
                "required": false,
                "description": "Page size.",
                "example": 20,
                "schema": { "type": "integer", "format": "int64", "minimum": 0, "maximum": 4294967295_u64 },
            })
        );
        assert_eq!(
            by_name("sort"),
            json!({
                "name": "sort",
                "in": "query",
                "required": false,
                "deprecated": true,
                "examples": {
                    "example1": { "value": "a" },
                    "example2": { "value": "b" },
                },
                "schema": { "type": "string" },
            })
        );
        assert_eq!(by_name("page")["example"], 7);
        assert!(by_name("page")["schema"].get("example").is_none());
    }
}

#[test]
fn header_parameters_are_lifted_too() {
    let parameters = expanded(Some("@header-params $Filter"));
    let search = parameters.iter().find(|p| p["name"] == "search").unwrap();
    assert_eq!(search["in"], "header");
    assert_eq!(search["deprecated"], true);
    assert!(search["schema"].get("deprecated").is_none());
}
