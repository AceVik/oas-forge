//! `src/output/swagger2.rs`: the Swagger 2.0 export.

use crate::common::{spec, spec_of};
use oas_forge::output::OutputKind;
use serde_json::{Value, json};

fn swagger(files: &[(&str, &str)]) -> (Value, Vec<String>) {
    let (spec, _project) = spec_of(files);
    let (document, diagnostics) = spec.render_with_diagnostics(OutputKind::Swagger20).unwrap();
    let errors = spec::validate(&document);
    assert!(errors.is_empty(), "{errors:?}\n{document:#}");
    (
        document,
        diagnostics
            .iter()
            .map(|d| d.message().to_string())
            .collect(),
    )
}

fn root(extra: &str) -> String {
    format!("openapi: 3.1.0\ninfo: {{ title: T, version: '1', summary: S }}\n{extra}")
}

#[test]
fn servers_become_host_base_path_and_schemes() {
    let (document, messages) = swagger(&[(
        "root.yaml",
        &root(
            "servers:\n  - url: https://{region}.example.com:8443/api/v1/\n    variables: { region: { default: eu } }\n  - url: http://other",
        ),
    )]);
    assert_eq!(document["host"], "eu.example.com:8443");
    assert_eq!(document["basePath"], "/api/v1");
    assert_eq!(document["schemes"], json!(["https"]));
    assert!(document["info"].get("summary").is_none());
    assert!(
        messages.iter().any(|m| m.contains("multiple servers")),
        "{messages:?}"
    );

    let (document, _) = swagger(&[("root.yaml", &root("servers: [{ url: /relative }]"))]);
    assert_eq!(document["basePath"], "/relative");
    assert!(document.get("host").is_none());
    let (document, _) = swagger(&[(
        "root.yaml",
        &root("servers: [{ url: 'ftp://files.example.com' }]"),
    )]);
    assert_eq!(document["host"], "files.example.com");
    assert!(document.get("schemes").is_none());
    // A variable without a default, or no host at all, gives no `host`.
    for url in ["'https://{tenant}.example.com/v1'", "'https:///v1'"] {
        let (document, _) =
            swagger(&[("root.yaml", &root(&format!("servers: [{{ url: {url} }}]")))]);
        assert!(document.get("host").is_none(), "{url}: {document}");
        assert_eq!(document["basePath"], "/v1", "{url}");
    }
}

#[test]
fn parameters_are_inline_and_cookies_dropped() {
    let (document, messages) = swagger(&[
        ("root.yaml", &root("")),
        (
            "api.rs",
            r#"
/// @openapi
struct Filter { a: u8 }
/// @route GET /items/{id: u32}
/// @param query tags: Vec<String> "Tags"
/// @param query csv: Vec<u8> explode=false
/// @param query pipes: Vec<u8> style=pipeDelimited
/// @param query spaces: Vec<u8> style=spaceDelimited
/// @param header X-Ids: Vec<u8>
/// @param query filter: $Filter deprecated example=x
/// @param query opt: Option<String> allow-empty
/// @param cookie session: String
/// @return 200:
fn list() {}
"#,
        ),
    ]);
    let parameters = document["paths"]["/items/{id}"]["get"]["parameters"]
        .as_array()
        .unwrap()
        .clone();
    let by_name = |name: &str| {
        parameters
            .iter()
            .find(|p| p["name"] == name)
            .unwrap()
            .clone()
    };
    assert_eq!(
        by_name("id"),
        json!({ "name": "id", "in": "path", "required": true, "type": "integer", "format": "int64", "minimum": 0, "maximum": 4294967295u32 })
    );
    assert_eq!(by_name("tags")["collectionFormat"], "multi");
    assert_eq!(by_name("tags")["items"], json!({ "type": "string" }));
    assert_eq!(by_name("csv")["collectionFormat"], "csv");
    assert_eq!(by_name("pipes")["collectionFormat"], "pipes");
    assert_eq!(by_name("spaces")["collectionFormat"], "ssv");
    assert_eq!(by_name("X-Ids")["collectionFormat"], "csv");
    let filter = by_name("filter");
    assert_eq!(
        (
            &filter["type"],
            &filter["x-deprecated"],
            &filter["x-example"]
        ),
        (&json!("string"), &json!(true), &json!("x"))
    );
    assert_eq!(by_name("opt")["allowEmptyValue"], true);
    assert!(parameters.iter().all(|p| p["in"] != "cookie"));
    assert!(
        messages.iter().any(|m| m.contains("cookie parameters")),
        "{messages:?}"
    );
    assert!(
        messages.iter().any(|m| m.contains("objects or references")),
        "{messages:?}"
    );
}

#[test]
fn bodies_become_body_or_form_parameters() {
    let (document, _) = swagger(&[
        (
            "root.yaml",
            &root(
                "components:\n  requestBodies:\n    Shared: { content: { application/json: { schema: { type: string } } }, required: true }",
            ),
        ),
        (
            "api.rs",
            r#"
/// @openapi
struct Upload {
    /// The file.
    /// @openapi format: binary
    file: String,
    note: Option<String>,
}
/// @route POST /json
/// @body $Upload application/json text/plain "The upload"
fn json_body() {}
/// @route POST /form
/// @body $Upload multipart/form-data
fn form() {}
/// @route POST /urlencoded
/// @body $Upload application/x-www-form-urlencoded
fn urlencoded() {}
/// @route POST /shared
/// requestBody: { $ref: '#/components/requestBodies/Shared' }
fn shared() {}
"#,
        ),
    ]);
    let post = |path: &str| document["paths"][path]["post"].clone();
    assert_eq!(
        post("/json")["consumes"],
        json!(["application/json", "text/plain"])
    );
    assert_eq!(
        post("/json")["parameters"],
        json!([{ "name": "body", "in": "body", "required": true, "schema": { "$ref": "#/definitions/Upload" }, "description": "The upload" }])
    );
    let form = post("/form")["parameters"].clone();
    assert_eq!(
        form[0],
        json!({ "name": "file", "in": "formData", "required": true, "description": "The file.", "type": "file" })
    );
    assert_eq!(form[1]["required"], false);
    assert_eq!(
        post("/urlencoded")["parameters"][0]["type"],
        "string",
        "files only in multipart"
    );
    assert_eq!(
        post("/shared")["parameters"][0]["schema"],
        json!({ "type": "string" })
    );
}

#[test]
fn responses_headers_ranges_and_media_types() {
    let (document, messages) = swagger(&[
        (
            "root.yaml",
            &root("components:\n  responses:\n    NotFound: { description: Missing }"),
        ),
        (
            "api.rs",
            r#"
/// @route GET /x
/// @return 2XX: String text/plain application/problem+json "Ok"
/// @return-header 2XX X-Rate: u32 "Rate"
/// @return-header 2XX X-Obj: $Thing
/// responses:
///   '404': { $ref: '#/components/responses/NotFound' }
///   '500': { description: Boom, links: { a: { operationId: x } }, x-extra: 1 }
fn x() {}
"#,
        ),
    ]);
    let operation = &document["paths"]["/x"]["get"];
    assert_eq!(
        operation["produces"],
        json!(["text/plain", "application/problem+json"])
    );
    let ok = &operation["responses"]["200"];
    assert_eq!(ok["description"], "Ok");
    assert_eq!(
        ok["schema"],
        json!({ "type": "string" }),
        "the JSON media type wins"
    );
    assert_eq!(ok["headers"]["X-Rate"]["type"], "integer");
    assert_eq!(ok["headers"]["X-Obj"]["type"], "string");
    assert_eq!(
        operation["responses"]["404"],
        json!({ "$ref": "#/responses/NotFound" })
    );
    assert_eq!(
        operation["responses"]["500"],
        json!({ "description": "Boom", "x-extra": 1 })
    );
    assert_eq!(
        document["responses"]["NotFound"],
        json!({ "description": "Missing" })
    );
    for expected in ["status code ranges", "response links"] {
        assert!(
            messages.iter().any(|m| m.contains(expected)),
            "{expected}: {messages:?}"
        );
    }
}

#[test]
fn security_schemes_become_security_definitions() {
    let (document, messages) = swagger(&[(
        "root.yaml",
        &root(
            r#"components:
  securitySchemes:
    basic: { type: http, scheme: Basic, description: Login }
    bearer: { type: http, scheme: bearer }
    header: { type: apiKey, in: header, name: X-Key }
    cookie: { type: apiKey, in: cookie, name: sid }
    implicit: { type: oauth2, flows: { implicit: { authorizationUrl: 'https://a', scopes: { r: read } } } }
    password: { type: oauth2, flows: { password: { tokenUrl: 'https://t', scopes: {} } } }
    client: { type: oauth2, flows: { clientCredentials: { tokenUrl: 'https://t', scopes: {} } } }
    code: { type: oauth2, flows: { authorizationCode: { authorizationUrl: 'https://a', tokenUrl: 'https://t', scopes: {} }, implicit: { authorizationUrl: 'https://a', scopes: {} } } }
    oidc: { type: openIdConnect, openIdConnectUrl: 'https://o' }
    broken: { type: oauth2, flows: {} }
    noflows: { type: oauth2 }
    untyped: { description: no type }
security:
  - basic: []"#,
        ),
    )]);
    let definitions = &document["securityDefinitions"];
    assert_eq!(
        definitions["basic"],
        json!({ "type": "basic", "description": "Login" })
    );
    assert_eq!(
        definitions["bearer"],
        json!({ "type": "apiKey", "in": "header", "name": "Authorization" })
    );
    assert_eq!(
        definitions["header"],
        json!({ "type": "apiKey", "in": "header", "name": "X-Key" })
    );
    assert_eq!(definitions["implicit"]["flow"], "implicit");
    assert_eq!(definitions["password"]["flow"], "password");
    assert_eq!(definitions["client"]["flow"], "application");
    assert_eq!(
        definitions["code"]["flow"], "implicit",
        "the first flow in Swagger order"
    );
    for missing in ["cookie", "oidc", "broken", "noflows", "untyped"] {
        assert!(definitions.get(missing).is_none(), "{missing}");
    }
    assert_eq!(document["security"], json!([{ "basic": [] }]));
    // Schemes that are left out are reported, also malformed ones.
    for expected in [
        "OAuth2 schemes without a known flow",
        "OAuth2 schemes without `flows`",
        "security schemes without a `type`",
    ] {
        assert!(
            messages.iter().any(|m| m.contains(expected)),
            "{expected}: {messages:?}"
        );
    }
    assert_eq!(
        messages.len(),
        8,
        "cookie, bearer, OIDC, several flows, no known flow, no flows, no type, info.summary: {messages:?}"
    );
}

#[test]
fn schemas_follow_the_swagger_dialect() {
    let (document, _) = swagger(&[
        ("root.yaml", &root("")),
        (
            "lib.rs",
            "/// @openapi\nstruct S { a: Option<String>, #[serde(skip_serializing)] p: String, #[deprecated] d: u8 }\n/// @openapi\n#[serde(tag = \"t\")]\nenum E { A { x: u8 } }\n/// @openapi\n#[serde(untagged)]\nenum U { A(u8), B(String) }",
        ),
    ]);
    let definitions = &document["definitions"];
    assert_eq!(
        definitions["S"]["properties"]["a"],
        json!({ "type": "string", "x-nullable": true })
    );
    assert_eq!(definitions["S"]["properties"]["p"]["x-writeOnly"], true);
    assert_eq!(definitions["S"]["properties"]["d"]["x-deprecated"], true);
    assert_eq!(definitions["E"]["discriminator"], "t");
    assert!(definitions["E"]["x-oneOf"].is_array());
    assert!(definitions["U"]["x-anyOf"].is_array());
}

#[test]
fn unsupported_structure_is_reported() {
    let (document, messages) = swagger(&[
        (
            "root.yaml",
            &root(
                "tags: [{ name: A, summary: S }]\ncomponents:\n  links: { L: { operationId: x } }",
            ),
        ),
        (
            "api.rs",
            "/// @openapi\n/// paths:\n///   /p:\n///     summary: S\n///     parameters: [{ name: q, in: query, schema: { type: string } }]\n///     trace: { responses: { '200': { description: OK } } }\n///     get:\n///       servers: [{ url: 'https://x' }]\n///       callbacks: { c: {} }\n///       x-op: 1\n///       responses: { '200': { description: OK } }\n///     x-item: 1\nfn p() {}",
        ),
    ]);
    let item = &document["paths"]["/p"];
    assert!(item.get("summary").is_none() && item.get("trace").is_none());
    assert_eq!(item["parameters"][0]["type"], "string");
    assert_eq!(
        (&item["x-item"], &item["get"]["x-op"]),
        (&json!(1), &json!(1))
    );
    assert!(document["tags"][0].get("summary").is_none());
    for expected in [
        "`TRACE`",
        "path item",
        "callbacks",
        "operation `servers`",
        "`components.links`",
        "tag `summary`",
    ] {
        assert!(
            messages.iter().any(|m| m.contains(expected)),
            "{expected}: {messages:?}"
        );
    }
}

#[test]
fn a_license_identifier_that_repeats_the_name_is_dropped_silently() {
    let license =
        |text: &str| format!("openapi: 3.1.0\ninfo: {{ title: T, version: '1', license: {text} }}");
    let (document, messages) =
        swagger(&[("root.yaml", &license("{ name: MIT, identifier: MIT }"))]);
    assert_eq!(document["info"]["license"], json!({ "name": "MIT" }));
    assert!(messages.is_empty(), "{messages:?}");
    let (_, messages) = swagger(&[(
        "root.yaml",
        &license("{ name: MIT License, identifier: MIT }"),
    )]);
    assert!(
        messages.iter().any(|m| m.contains("`license.identifier`")),
        "{messages:?}"
    );
}

mod components_and_edge_cases {
    use super::*;

    #[test]
    fn reusable_parameters_responses_and_schemes_are_converted() {
        let (document, messages) = swagger(&[(
            "root.yaml",
            &root(
                r##"paths:
  /a:
    get:
      parameters:
        - $ref: "#/components/parameters/Limit"
        - { name: filter, in: query, content: { application/json: { schema: { type: string } } } }
        - { name: shape, in: query, schema: { type: object, description: Shape filter } }
      responses:
        "404": { $ref: "#/components/responses/NotFound" }
components:
  parameters:
    Limit: { name: limit, in: query, schema: { type: integer } }
    Session: { name: session, in: cookie, schema: { type: string } }
  responses:
    NotFound: { description: Not found }
  securitySchemes:
    key: { type: apiKey, in: header, name: X-Key }
    tls: { type: mutualTLS }
"##,
            ),
        )]);
        assert_eq!(
            document["parameters"],
            json!({ "Limit": { "name": "limit", "in": "query", "type": "integer" } })
        );
        assert_eq!(
            document["responses"],
            json!({ "NotFound": { "description": "Not found" } })
        );
        assert_eq!(
            document["securityDefinitions"],
            json!({ "key": { "type": "apiKey", "in": "header", "name": "X-Key" } })
        );
        let parameters = &document["paths"]["/a"]["get"]["parameters"];
        assert_eq!(parameters[0], json!({ "$ref": "#/parameters/Limit" }));
        assert_eq!(parameters[1]["type"], "string");
        assert_eq!(
            parameters[2],
            json!({ "name": "shape", "in": "query", "type": "string", "description": "Shape filter" })
        );
        assert!(
            messages.iter().any(|m| m.contains("cookie parameters")),
            "{messages:?}"
        );
    }

    #[test]
    fn path_item_references_and_bodies_without_content() {
        let (document, _) = swagger(&[(
            "root.yaml",
            &root(
                r##"servers: []
paths:
  /a:
    $ref: "#/components/pathItems/A"
  /b:
    post:
      requestBody: { description: Nothing to send }
      responses: { "204": { description: Done } }
"##,
            ),
        )]);
        assert_eq!(
            document["paths"]["/a"],
            json!({ "$ref": "#/components/pathItems/A" })
        );
        assert!(document["paths"]["/b"]["post"].get("parameters").is_none());
        assert!(document.get("host").is_none());
    }

    #[test]
    fn malformed_sections_pass_through() {
        let (spec, _project) = spec_of(&[(
            "root.yaml",
            "openapi: 3.1.0\ninfo: { title: T, version: '1' }\npaths:\n  /a: 5\n  /b: { get: 5 }\n  /c: { get: { parameters: 5, responses: { '200': 5 } } }\n  /d: { get: { parameters: [5], responses: {} } }\ncomponents: 5",
        )]);
        let document = spec.render(OutputKind::Swagger20).unwrap();
        assert_eq!(document["paths"]["/a"], 5);
        assert_eq!(document["paths"]["/b"]["get"], 5);
        assert_eq!(document["paths"]["/c"]["get"]["responses"]["200"], 5);
        assert_eq!(document["paths"]["/d"]["get"]["parameters"], json!([5]));
        assert!(document.get("definitions").is_none());

        let (spec, _project) = spec_of(&[(
            "root.yaml",
            "openapi: 3.1.0\ninfo: 5\ntags: [5]\npaths:\n  /a: { get: { responses: 5 } }\ncomponents: { parameters: 5, responses: 5, securitySchemes: 5 }",
        )]);
        let document = spec.render(OutputKind::Swagger20).unwrap();
        assert_eq!(document["info"], 5);
        assert_eq!(document["tags"], json!([5]));
        assert_eq!(document["parameters"], json!({}));
        assert_eq!(document["responses"], json!({}));
        assert_eq!(document["securityDefinitions"], json!({}));
    }
}

mod review_fixes {
    use super::*;

    #[test]
    fn produces_lists_each_media_type_once() {
        let (document, _) = swagger(&[
            ("root.yaml", &root("")),
            (
                "api.rs",
                r#"
/// @openapi
struct Item { a: u8 }
/// @route GET /x
/// @return 200: $Item
/// @problem 404
/// @return 409: $Item
/// @problem 410
fn x() {}
"#,
            ),
        ]);
        assert_eq!(
            document["paths"]["/x"]["get"]["produces"],
            json!(["application/json", "application/problem+json"])
        );
    }

    #[test]
    fn an_explicit_status_wins_over_a_range() {
        for lines in [
            "/// @return 200: String \"Exact\"\n/// @return 2XX: u8 \"Range\"",
            "/// @return 2XX: u8 \"Range\"\n/// @return 200: String \"Exact\"",
        ] {
            let (document, messages) = swagger(&[
                ("root.yaml", &root("")),
                (
                    "api.rs",
                    &format!("/// @route GET /x\n{lines}\nfn x() {{}}"),
                ),
            ]);
            let responses = &document["paths"]["/x"]["get"]["responses"];
            assert_eq!(responses["200"]["description"], "Exact", "{lines}");
            assert!(
                messages
                    .iter()
                    .any(|m| m.contains("status code ranges that collide")),
                "{messages:?}"
            );
        }
    }

    #[test]
    fn references_to_left_out_components_are_not_kept() {
        let (document, messages) = swagger(&[(
            "root.yaml",
            &root(
                r##"security:
  - oidc: []
  - key: []
paths:
  /a:
    get:
      security: [{ oidc: [] }]
      parameters:
        - $ref: "#/components/parameters/Session"
        - $ref: "#/components/parameters/Limit"
      responses:
        "200":
          description: OK
          headers:
            X-Rate: { $ref: "#/components/headers/Rate" }
  /b:
    get:
      security: [{ oidc: [], key: [] }, { key: [] }]
      responses: { "200": { description: OK } }
components:
  parameters:
    Session: { name: sid, in: cookie, schema: { type: string } }
    Limit: { name: limit, in: query, schema: { type: integer } }
  headers:
    Rate: { description: Requests left, schema: { type: integer } }
  securitySchemes:
    oidc: { type: openIdConnect, openIdConnectUrl: "https://o" }
    key: { type: apiKey, in: header, name: X-Key }
"##,
            ),
        )]);
        assert_eq!(document["security"], json!([{ "key": [] }]));
        let a = &document["paths"]["/a"]["get"];
        assert_eq!(
            a["parameters"],
            json!([{ "$ref": "#/parameters/Limit" }]),
            "the cookie parameter reference is left out with its definition"
        );
        assert!(a.get("security").is_none(), "{a:#}");
        assert_eq!(a["x-security"], json!([{ "oidc": [] }]));
        assert_eq!(
            a["responses"]["200"]["headers"]["X-Rate"],
            json!({ "description": "Requests left", "type": "integer" })
        );
        assert_eq!(
            document["paths"]["/b"]["get"]["security"],
            json!([{ "key": [] }])
        );
        assert!(
            messages
                .iter()
                .any(|m| m.contains("cannot express some security schemes")),
            "{messages:?}"
        );
    }
}

#[test]
fn malformed_security_requirements_pass_through() {
    let (spec, _project) = spec_of(&[(
        "root.yaml",
        "openapi: 3.1.0\ninfo: { title: T, version: '1' }\nsecurity: 5",
    )]);
    let document = spec.render(OutputKind::Swagger20).unwrap();
    assert_eq!(document["security"], 5);
}

/// Found by mutation testing: details of the conversion that no other test
/// pinned down.
mod conversion_details {
    use super::*;

    #[test]
    fn extensions_in_components_are_not_reported_as_lost() {
        let (_, messages) = swagger(&[(
            "root.yaml",
            &root("components:\n  x-internal: true\n  schemas: { A: { type: string } }"),
        )]);
        assert!(
            !messages.iter().any(|m| m.contains("x-internal")),
            "{messages:?}"
        );
    }

    #[test]
    fn one_oauth2_flow_is_not_a_loss() {
        let flows = |flows: &str| {
            root(&format!(
                "components:\n  securitySchemes:\n    o: {{ type: oauth2, flows: {{ {flows} }} }}"
            ))
        };
        let one = "password: { tokenUrl: 'https://t', scopes: {} }";
        let two = "password: { tokenUrl: 'https://t', scopes: {} }, implicit: { authorizationUrl: 'https://a', scopes: {} }";
        let several = |m: &Vec<String>| m.iter().any(|m| m.contains("several OAuth2 flows"));
        assert!(!several(&swagger(&[("root.yaml", &flows(one))]).1));
        assert!(several(&swagger(&[("root.yaml", &flows(two))]).1));
    }

    #[test]
    fn parameters_with_content_take_the_first_schema() {
        let (document, _) = swagger(&[(
            "root.yaml",
            &root(
                "paths:\n  /a:\n    get:\n      parameters:\n        - name: q\n          in: query\n          content: { application/json: { schema: { type: integer } } }\n      responses: { '200': { description: OK } }",
            ),
        )]);
        assert_eq!(
            document["paths"]["/a"]["get"]["parameters"][0]["type"],
            "integer"
        );
    }

    #[test]
    fn responses_prefer_a_json_media_type() {
        for json in ["application/json", "application/vnd.api+json"] {
            let (document, _) = swagger(&[(
                "root.yaml",
                &root(&format!(
                    "paths:\n  /a:\n    get:\n      responses:\n        '200':\n          description: OK\n          content:\n            text/plain: {{ schema: {{ type: string }} }}\n            {json}: {{ schema: {{ type: integer }} }}"
                )),
            )]);
            assert_eq!(
                document["paths"]["/a"]["get"]["responses"]["200"]["schema"]["type"], "integer",
                "{json}"
            );
        }
    }
}

mod form_budget {
    use super::*;
    use crate::common::{Project, codes};
    use oas_forge::diagnostics::Code;
    use oas_forge::{Error, Generator};

    /// The budget of the `formData` parameters of one document.
    const BUDGET: usize = 16 * 1024 * 1024;

    /// What each parameter of the forms below costs: 256 bytes and its
    /// JSON text. 64 operations of 64 parameters fill the budget exactly.
    const COST: usize = 4096;

    /// The length of the description that makes a parameter `f00`…`f99`
    /// of type string cost [`COST`].
    fn description_length() -> usize {
        let fixed = json!({ "name": "f00", "in": "formData", "required": false, "description": "", "type": "string" });
        COST - 256 - fixed.to_string().len()
    }

    /// A root whose form request body `Form` (a schema of `properties`
    /// string properties described with `description` bytes each) is used
    /// by `operations` operations `POST /p<n>`.
    fn form_root(properties: usize, description: usize, operations: usize) -> String {
        let filler = "x".repeat(description);
        let mut text =
            root("components:\n  schemas:\n    Big:\n      type: object\n      properties:\n");
        for p in 0..properties {
            text.push_str(&format!(
                "        f{p:02}: {{ type: string, description: '{filler}' }}\n"
            ));
        }
        text.push_str(
            "  requestBodies:\n    Form:\n      content:\n        application/x-www-form-urlencoded:\n          schema: { $ref: '#/components/schemas/Big' }\npaths:\n",
        );
        for n in 0..operations {
            text.push_str(&format!(
                "  /p{n:03}:\n    post:\n      requestBody: {{ $ref: '#/components/requestBodies/Form' }}\n      responses: {{ '200': {{ description: ok }} }}\n"
            ));
        }
        text
    }

    /// The form bodies the budget cuts are left out from the first one
    /// on, with one error naming it; the operations keep `consumes`. A
    /// form whose parameters cost exactly what is left still fits.
    #[test]
    fn form_bodies_beyond_the_budget_are_left_out_and_reported() {
        let (spec, _project) = spec_of(&[("root.yaml", &form_root(64, description_length(), 66))]);
        let (document, diagnostics) = spec.render_with_diagnostics(OutputKind::Swagger20).unwrap();
        let errors = spec::validate(&document);
        assert!(errors.is_empty(), "{errors:?}");
        let operation = |n: usize| document["paths"][format!("/p{n:03}")]["post"].clone();
        let first = operation(0)["parameters"].clone();
        assert_eq!(first.as_array().map(Vec::len), Some(64));
        assert_eq!(first[0]["name"], "f00");
        assert_eq!(first[0]["in"], "formData");
        // Every parameter costs the same: 256 bytes besides its JSON text.
        assert_eq!(256 + first[0].to_string().len(), COST);
        assert_eq!(BUDGET, 64 * 64 * COST);
        for n in 0..66 {
            let operation = operation(n);
            assert_eq!(
                operation["consumes"],
                json!(["application/x-www-form-urlencoded"]),
                "operation {n}"
            );
            assert_eq!(
                operation["parameters"].as_array().map(Vec::len),
                (n < 64).then_some(64),
                "operation {n}"
            );
        }
        // The note is `info.summary`.
        assert_eq!(
            codes(&diagnostics),
            [Code::ExpansionLimit, Code::LossyLowering]
        );
        assert!(diagnostics.has_errors());
        assert_eq!(
            errors_of(&diagnostics),
            [
                "the `formData` parameters of the form bodies of 2 operations, from POST /p064 on, exceed what is left of the 16777216 bytes of form parameters in a Swagger 2.0 document (each costs 256 bytes besides its text); those bodies are left out"
            ]
        );
        // Each export has the whole budget.
        let (again, diagnostics) = spec.render_with_diagnostics(OutputKind::Swagger20).unwrap();
        assert_eq!(again, document);
        assert_eq!(errors_of(&diagnostics).len(), 1);
    }

    /// The messages of the errors among `diagnostics`.
    fn errors_of(diagnostics: &oas_forge::Diagnostics) -> Vec<&str> {
        diagnostics
            .iter()
            .filter(|d| d.code() == Code::ExpansionLimit)
            .map(|d| d.message())
            .collect()
    }

    /// A single form body that does not fit is named without a count.
    #[test]
    fn a_single_cut_form_body_is_named() {
        let (spec, _project) = spec_of(&[("root.yaml", &form_root(64, description_length(), 65))]);
        let (document, diagnostics) = spec.render_with_diagnostics(OutputKind::Swagger20).unwrap();
        assert_eq!(
            document["paths"]["/p063"]["post"]["parameters"]
                .as_array()
                .map(Vec::len),
            Some(64)
        );
        assert!(
            document["paths"]["/p064"]["post"]
                .get("parameters")
                .is_none()
        );
        assert_eq!(
            errors_of(&diagnostics),
            [
                "the `formData` parameters of the form body of POST /p064 exceed what is left of the 16777216 bytes of form parameters in a Swagger 2.0 document (each costs 256 bytes besides its text); that body is left out"
            ]
        );
    }

    /// The run fails on the error, like on a struct expansion that does
    /// not fit, so no incomplete Swagger 2.0 document is written.
    #[test]
    fn a_cut_form_body_fails_the_run() {
        let project = Project::new();
        project.file("src/root.yaml", &form_root(64, description_length(), 65));
        let error = Generator::new()
            .input(project.path("src"))
            .output_swagger2(project.path("swagger.json"))
            .generate()
            .unwrap_err();
        let Error::Diagnostics(findings) = error else {
            panic!("{error}");
        };
        assert_eq!(
            findings.iter().map(|d| d.code()).collect::<Vec<_>>(),
            [Code::ExpansionLimit, Code::LossyLowering]
        );
        assert!(!project.path("swagger.json").exists());
    }
}
