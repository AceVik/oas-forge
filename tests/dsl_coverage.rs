use oas_forge::dsl::parse_route_dsl;
use serde_json::Value;

#[test]
fn test_params_primitive() {
    let lines = vec![
        "@route GET /test".to_string(),
        "@query-param q: String".to_string(),
        "@query-param limit: i32".to_string(),
        "@query-param active: bool".to_string(),
    ];
    let yaml = parse_route_dsl(&lines, "op").unwrap();
    let root: Value = serde_yaml::from_str(&yaml).unwrap();
    let params = root["paths"]["/test"]["get"]["parameters"]
        .as_array()
        .unwrap();

    assert_eq!(params.len(), 3);
    assert_eq!(params[0]["name"], "q");
    assert_eq!(params[0]["schema"]["type"], "string");

    assert_eq!(params[1]["name"], "limit");
    assert_eq!(params[1]["schema"]["type"], "integer");
    assert_eq!(params[1]["schema"]["format"], "int32");

    assert_eq!(params[2]["name"], "active");
    assert_eq!(params[2]["schema"]["type"], "boolean");
}

#[test]
fn test_params_implicit_string() {
    let lines = vec![
        "@route GET /test".to_string(),
        "@query-param simple:".to_string(), // Implicit String with colon
        "@query-param with_attr: deprecated".to_string(), // Implicit String + Attr
    ];
    let yaml = parse_route_dsl(&lines, "op").unwrap();
    let root: Value = serde_yaml::from_str(&yaml).unwrap();
    let params = root["paths"]["/test"]["get"]["parameters"]
        .as_array()
        .unwrap();

    assert_eq!(params[0]["name"], "simple");
    assert_eq!(params[0]["schema"]["type"], "string");

    assert_eq!(params[1]["name"], "with_attr");
    assert_eq!(params[1]["schema"]["type"], "string");
    assert_eq!(params[1]["deprecated"], true);
}

#[test]
fn test_params_array() {
    let lines = vec![
        "@route GET /test".to_string(),
        "@query-param tags: [String]".to_string(),
        "@query-param ids: Vec<i32>".to_string(),
    ];
    let yaml = parse_route_dsl(&lines, "op").unwrap();
    let root: Value = serde_yaml::from_str(&yaml).unwrap();
    let params = root["paths"]["/test"]["get"]["parameters"]
        .as_array()
        .unwrap();

    assert_eq!(params[0]["name"], "tags");
    assert_eq!(params[0]["schema"]["type"], "array");
    assert_eq!(params[0]["schema"]["items"]["type"], "string");

    assert_eq!(params[1]["name"], "ids");
    assert_eq!(params[1]["schema"]["type"], "array");
    assert_eq!(params[1]["schema"]["items"]["type"], "integer");
}

#[test]
fn test_params_attrs() {
    let lines = vec![
        "@route GET /test".to_string(),
        "@query-param q: String required deprecated example=\"foo\" \"Search Term\"".to_string(),
    ];
    let yaml = parse_route_dsl(&lines, "op").unwrap();
    let root: Value = serde_yaml::from_str(&yaml).unwrap();
    let params = root["paths"]["/test"]["get"]["parameters"]
        .as_array()
        .unwrap();
    let p = &params[0];

    assert_eq!(p["name"], "q");
    assert_eq!(p["required"], true);
    assert_eq!(p["deprecated"], true);
    assert_eq!(p["example"], "foo");
    assert_eq!(p["description"], "Search Term");
}

#[test]
fn test_inline_path_params() {
    let lines = vec!["@route GET /users/{id: u32 \"User ID\"}".to_string()];
    let yaml = parse_route_dsl(&lines, "op").unwrap();
    let root: Value = serde_yaml::from_str(&yaml).unwrap();
    let params = root["paths"]["/users/{id}"]["get"]["parameters"]
        .as_array()
        .unwrap();

    assert_eq!(params.len(), 1);
    let p = &params[0];
    assert_eq!(p["name"], "id");
    assert_eq!(p["in"], "path");
    assert_eq!(p["required"], true);
    assert_eq!(p["schema"]["type"], "integer");
    assert_eq!(p["description"], "User ID");
}

#[test]
fn test_inline_path_params_bare() {
    let lines = vec![
        "@route GET /users/{id}".to_string(),
        "@path-param id: String".to_string(), // Defined explicitly
    ];
    let yaml = parse_route_dsl(&lines, "op").unwrap();
    let root: Value = serde_yaml::from_str(&yaml).unwrap();
    let params = root["paths"]["/users/{id}"]["get"]["parameters"]
        .as_array()
        .unwrap();

    // Should merge or deduplicate?
    // Logic says: path param extraction adds to params list.
    // And @path-param adds to params list.
    // If name matches, it might duplicate unless we check uniqueness.
    // The current implementation appends.
    // Let's see behavior.
    assert_eq!(params.len(), 1);
    assert_eq!(params[0]["name"], "id");
}

#[test]
fn test_body_parsing() {
    let lines = vec!["@route POST /users".to_string(), "@body User".to_string()];
    let yaml = parse_route_dsl(&lines, "op").unwrap();
    let root: Value = serde_yaml::from_str(&yaml).unwrap();
    let content = &root["paths"]["/users"]["post"]["requestBody"]["content"]["application/json"];

    assert_eq!(content["schema"]["$ref"], "$User");
}

#[test]
fn test_body_custom_mime() {
    let lines = vec![
        "@route POST /users".to_string(),
        "@body User application/xml".to_string(),
    ];
    let yaml = parse_route_dsl(&lines, "op").unwrap();
    let root: Value = serde_yaml::from_str(&yaml).unwrap();
    let content = &root["paths"]["/users"]["post"]["requestBody"]["content"];

    assert!(content.get("application/xml").is_some());
    assert!(content.get("application/json").is_none());
}

#[test]
fn test_return_parsing() {
    let lines = vec![
        "@route GET /users".to_string(),
        "@return 200: User \"Success\"".to_string(),
        "@return 404: \"Not Found\"".to_string(), // Unit return
    ];
    let yaml = parse_route_dsl(&lines, "op").unwrap();
    let root: Value = serde_yaml::from_str(&yaml).unwrap();
    let responses = &root["paths"]["/users"]["get"]["responses"];

    let r200 = &responses["200"];
    assert_eq!(r200["description"], "Success");
    assert_eq!(
        r200["content"]["application/json"]["schema"]["$ref"],
        "$User"
    );

    let r404 = &responses["404"];
    assert_eq!(r404["description"], "Not Found");
    assert!(r404.get("content").is_none());
}

#[test]
fn test_return_wrappers() {
    let lines = vec![
        "@route GET /users".to_string(),
        "@return 200: Option<User>".to_string(),
        // Json<User> not standard so likely raw ref if I don't add it to exception list
        // I didn't add Json to exception list.
        // So Json<User> -> "$ref": "Json<User>"
        "@return 201: Json<User>".to_string(),
    ];
    let yaml = parse_route_dsl(&lines, "op").unwrap();
    let root: Value = serde_yaml::from_str(&yaml).unwrap();
    let responses = &root["paths"]["/users"]["get"]["responses"];

    // Option<T> should be T (nullable logic is usually handled by map_syn_type_to_openapi but here we check structure)
    // Note: map_syn_type_to_openapi returns (schema, required).
    // For T, it returns $ref.
    // For Option<T>, it matches "Option" and recurses.

    // Check 200 (Option<User>) -> Now handled by map -> $User
    assert_eq!(
        responses["200"]["content"]["application/json"]["schema"]["$ref"],
        "$User"
    );

    // Check 201 (Json<User>) -> Generic check catches it -> ref: Json<User>
    assert_eq!(
        responses["201"]["content"]["application/json"]["schema"]["$ref"],
        "Json<User>"
    );
}

#[test]
fn test_security_parsing() {
    let lines = vec![
        "@route GET /users".to_string(),
        "@security Basic".to_string(),
        "@security OAuth2(\"read\", \"write\")".to_string(),
    ];
    let yaml = parse_route_dsl(&lines, "op").unwrap();
    let root: Value = serde_yaml::from_str(&yaml).unwrap();
    let security = root["paths"]["/users"]["get"]["security"]
        .as_array()
        .unwrap();

    assert_eq!(security.len(), 2);
    assert_eq!(security[0]["Basic"].as_array().unwrap().len(), 0);

    let oauth = &security[1]["OAuth2"].as_array().unwrap();
    assert_eq!(oauth.len(), 2);
    assert_eq!(oauth[0], "read");
    assert_eq!(oauth[1], "write");
}

#[test]
fn test_raw_yaml_overrides() {
    let lines = vec![
        "@route GET /users".to_string(),
        "responses:".to_string(),
        "  '200':".to_string(),
        "    description: Override".to_string(),
        "servers:".to_string(),
        "  - url: https://api.example.com".to_string(),
    ];
    let yaml = parse_route_dsl(&lines, "op").unwrap();
    let root: Value = serde_yaml::from_str(&yaml).unwrap();

    let responses = &root["paths"]["/users"]["get"]["responses"];
    assert_eq!(responses["200"]["description"], "Override");

    // "servers" is not typically under PathItem directly in standard OAS structure (it's under Operation)
    // dsl.rs merges overrides into the Operation object.
    let servers = &root["paths"]["/users"]["get"]["servers"];
    assert_eq!(servers[0]["url"], "https://api.example.com");
}
