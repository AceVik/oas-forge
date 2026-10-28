//! `src/dsl/expand.rs`: struct expansion of parameters (`@query-params`,
//! `@header-params`, inferred `Query<T>`).

use crate::common::pipeline;
use oas_forge::diagnostics::Code;
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

#[test]
fn examples_stay_in_the_schema_next_to_an_example_or_when_not_a_list() {
    let (document, diagnostics) = pipeline(&[(
        "api.rs",
        r#"
/// @openapi
pub struct Filter {
    /// @example 1
    /// @openapi
    /// example: 2
    pub both: Option<u32>,
    /// @openapi
    /// examples: not a list
    pub odd: Option<u32>,
}

/// @route GET /x
/// @query-params $Filter
fn x() {}
"#,
    )]);
    assert!(
        diagnostics.iter().all(|d| !d.is_error()),
        "{diagnostics:#?}"
    );
    let parameters = document["paths"]["/x"]["get"]["parameters"]
        .as_array()
        .unwrap();
    let both = parameters.iter().find(|p| p["name"] == "both").unwrap();
    assert_eq!(both["example"], 2);
    assert!(both.get("examples").is_none());
    assert_eq!(both["schema"]["examples"], json!([1]));
    let odd = parameters.iter().find(|p| p["name"] == "odd").unwrap();
    assert!(odd.get("examples").is_none() && odd.get("example").is_none());
    assert_eq!(odd["schema"]["examples"], "not a list");
}

/// `name:required` of the parameters of `GET /a` after expanding `$Root`
/// (defined by `schemas`, file-level blocks) as query parameters.
fn expanded_root(schemas: &str) -> Vec<String> {
    let source = format!("{schemas}\n/// @route GET /a\n/// @query-params $Root\nfn a() {{}}");
    let (document, _) = pipeline(&[("api.rs", &source)]);
    document["paths"]["/a"]["get"]["parameters"]
        .as_array()
        .unwrap_or_else(|| panic!("no parameters: {document:#}"))
        .iter()
        .map(|p| format!("{}:{}", p["name"].as_str().unwrap(), p["required"]))
        .collect()
}

/// A file-level `@openapi-type` block.
fn named_schema(name: &str, yaml: &str) -> String {
    format!("//! @openapi-type {name}\n//! {yaml}\n")
}

#[test]
fn every_required_property_is_a_required_parameter() {
    let (document, diagnostics) = pipeline(&[(
        "api.rs",
        "/// @openapi\nstruct Window { from: u32, to: u32, step: Option<u32> }\n/// @route GET /a\n/// @query-params $Window\nfn a() {}",
    )]);
    assert!(diagnostics.is_empty(), "{diagnostics:?}");
    let required: Vec<String> = document["paths"]["/a"]["get"]["parameters"]
        .as_array()
        .unwrap()
        .iter()
        .map(|p| format!("{}:{}", p["name"].as_str().unwrap(), p["required"]))
        .collect();
    assert_eq!(required, ["from:true", "to:true", "step:false"]);
}

/// `allOf` parts nest one level each: the properties of the part at depth 8
/// are still parameters, those deeper down are not.
#[test]
fn nested_parts_are_followed_eight_levels_deep() {
    let mut part = String::from("{ properties: { p9: { type: string } } }");
    for level in (0..9).rev() {
        part = format!("{{ properties: {{ p{level}: {{ type: string }} }}, allOf: [{part}] }}");
    }
    let names = expanded_root(&named_schema("Root", &part));
    let expected: Vec<String> = (0..9).map(|level| format!("p{level}:false")).collect();
    assert_eq!(names, expected);
}

/// A reference is one level too: `$ref` and `allOf` part make two per
/// flattened struct.
#[test]
fn references_count_towards_the_depth() {
    let mut schemas = named_schema(
        "Root",
        "{ properties: { r0: { type: string } }, allOf: [{ $ref: $R1 }] }",
    );
    for level in 1..6 {
        schemas.push_str(&named_schema(
            &format!("R{level}"),
            &format!(
                "{{ properties: {{ r{level}: {{ type: string }} }}, allOf: [{{ $ref: $R{} }}] }}",
                level + 1
            ),
        ));
    }
    schemas.push_str(&named_schema(
        "R6",
        "{ properties: { r6: { type: string } } }",
    ));
    // `R4` is reached at depth 8; the part that refers to `R5` is too deep.
    assert_eq!(
        expanded_root(&schemas),
        ["r0:false", "r1:false", "r2:false", "r3:false", "r4:false"]
    );
}

/// The alternatives of an optional flattened struct (`anyOf: [schema, {}]`)
/// are one level deeper, like a part.
#[test]
fn optional_alternatives_count_towards_the_depth() {
    let mut part = String::from("{ properties: { o9: { type: string } } }");
    for level in (1..9).rev() {
        part = format!(
            "{{ properties: {{ o{level}: {{ type: string }} }}, allOf: [{{ anyOf: [{part}, {{ type: 'null' }}] }}] }}"
        );
    }
    let root = format!(
        "{{ properties: {{ o0: {{ type: string }} }}, required: [o0], allOf: [{{ anyOf: [{part}, {{}}] }}] }}"
    );
    let mut expected = vec!["o0:true".to_string()];
    expected.extend((1..9).map(|level| format!("o{level}:false")));
    assert_eq!(expanded_root(&named_schema("Root", &root)), expected);
}

/// Only an optional value (`anyOf` with `{}` or `null`) is flattened from
/// its alternatives; the alternatives of a real choice are not parameters.
#[test]
fn alternatives_of_a_choice_are_not_expanded() {
    let root = "{ properties: { own: { type: string } }, allOf: [{ anyOf: [{ properties: { x: { type: string } } }, { properties: { y: { type: string } } }] }] }";
    assert_eq!(expanded_root(&named_schema("Root", root)), ["own:false"]);
}

/// Each expanded parameter and required field was checked against the ones
/// before by scanning: a struct with 10 000 fields took a minute and a half.
#[test]
fn large_structs_expand_quickly() {
    let fields: String = (0..10_000).map(|i| format!("pub f{i}: u64, ")).collect();
    let text = format!(
        "/// @openapi\npub struct Big {{ {fields} pub o: Option<u8> }}\n/// @route GET /x\n/// @query-params $Big\n/// @param query f3: String\nfn x() {{}}"
    );
    let started = std::time::Instant::now();
    let (document, diagnostics) = pipeline(&[("api.rs", &text)]);
    assert!(
        started.elapsed() < std::time::Duration::from_secs(30),
        "{:?}",
        started.elapsed()
    );
    assert!(diagnostics.is_empty(), "{diagnostics:#?}");
    let parameters = document["paths"]["/x"]["get"]["parameters"]
        .as_array()
        .unwrap();
    assert_eq!(parameters.len(), 10_001);
    // The explicit parameter wins; the optional field is not required.
    let f3: Vec<&Value> = parameters.iter().filter(|p| p["name"] == "f3").collect();
    assert_eq!(f3.len(), 1);
    assert_eq!(f3[0]["schema"], json!({ "type": "string" }));
    let o = parameters.iter().find(|p| p["name"] == "o").unwrap();
    assert_eq!(o["required"], false);
    let last = parameters.iter().find(|p| p["name"] == "f9999").unwrap();
    assert_eq!(last["required"], true);
}

/// A schema whose `allOf` refers to itself many times was walked
/// exponentially (60 parts took two minutes); each reference is followed
/// once.
#[test]
fn self_references_in_all_of_are_followed_once() {
    let parts = vec!["{ $ref: '#/components/schemas/A' }"; 60].join(", ");
    let started = std::time::Instant::now();
    let (document, _) = pipeline(&[
        (
            "a.yaml",
            &format!(
                "components: {{ schemas: {{ A: {{ type: object, required: [p], properties: {{ p: {{ type: string }} }}, allOf: [{parts}] }} }} }}"
            ),
        ),
        (
            "api.rs",
            "/// @route GET /x\n/// @query-params $A\nfn x() {}",
        ),
    ]);
    assert!(
        started.elapsed() < std::time::Duration::from_secs(10),
        "{:?}",
        started.elapsed()
    );
    assert_eq!(
        document["paths"]["/x"]["get"]["parameters"],
        json!([{ "name": "p", "in": "query", "required": true, "schema": { "type": "string" } }])
    );
}

/// Once the budget is used up, the fields of the struct were still copied
/// and serialized for every later operation: 200 routes expanding a struct
/// with 20 000 fields took a minute. An expansion that cannot fit is
/// reported without copying a field.
#[test]
fn expansions_over_the_budget_are_reported_without_copying_fields() {
    let fields: String = (0..20_000).map(|i| format!("pub f{i}: u64, ")).collect();
    let routes: String = (0..200)
        .map(|i| format!("/// @route GET /r{i}\n/// @query-params $Big\nfn r{i}() {{}}\n"))
        .collect();
    let text = format!("/// @openapi\npub struct Big {{ {fields} }}\n{routes}");
    let started = std::time::Instant::now();
    let (document, diagnostics) = pipeline(&[("api.rs", &text)]);
    assert!(
        started.elapsed() < std::time::Duration::from_secs(30),
        "{:?}",
        started.elapsed()
    );
    let expanded = (0..200)
        .filter(|i| {
            document["paths"][format!("/r{i}")]["get"]["parameters"]
                .as_array()
                .is_some_and(|p| p.len() == 20_000)
        })
        .count();
    assert_eq!(expanded, 2, "two copies of 20 000 fields fit in 16 MiB");
    let limits = diagnostics
        .iter()
        .filter(|d| d.code() == Code::ExpansionLimit)
        .count();
    assert_eq!(limits, 198, "{:?}", diagnostics.iter().next());
    // The routes that fit come first; the others have no parameters.
    assert!(document["paths"]["/r1"]["get"]["parameters"].is_array());
    assert!(document["paths"]["/r2"]["get"].get("parameters").is_none());
}

/// Every operation copies the fields of the struct it expands: the copies
/// of a document share 16 MiB (each parameter costs 256 more than its
/// text). 40 000 fields for 20 routes took 4.8 GB.
#[test]
fn struct_expansions_of_a_document_share_a_budget() {
    for (fields, routes, limited) in [(1_000, 20, 0), (40_000, 3, 2)] {
        let fields: String = (0..fields)
            .map(|i| format!("p{i}: {{ type: string }}, "))
            .collect();
        let routes: String = (0..routes)
            .map(|i| format!("/// @route GET /r{i}\n/// @query-params $Big\nfn r{i}() {{}}\n"))
            .collect();
        let (_, diagnostics) = pipeline(&[
            (
                "big.yaml",
                &format!(
                    "components: {{ schemas: {{ Big: {{ type: object, properties: {{ {fields} }} }} }} }}"
                ),
            ),
            ("api.rs", &routes),
        ]);
        let found = diagnostics
            .iter()
            .filter(|d| d.code() == Code::ExpansionLimit)
            .count();
        assert_eq!(found, limited, "{:?}", diagnostics.iter().next());
    }
}
