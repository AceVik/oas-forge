use oas_forge::index::Registry;
use oas_forge::preprocessor;
use oas_forge::visitor::OpenApiVisitor;
use serde_json::Value;
use syn::visit::Visit;

// 1. Enum Safety Test
#[test]
fn test_enum_implicit_safety() {
    let code = r#"
        /// This should be ignored
        pub enum IgnoredEnum { A, B }

        /// @openapi
        pub enum IncludedEnum { C, D }
    "#;

    let mut visitor = OpenApiVisitor::default();
    let syntax = syn::parse_str::<syn::File>(code).unwrap();
    visitor.visit_file(&syntax);

    let items = visitor.items;
    assert_eq!(items.len(), 1, "Should only have 1 item");

    if let oas_forge::visitor::ExtractedItem::Schema { name, .. } = &items[0] {
        assert_eq!(name.as_ref().unwrap(), "IncludedEnum");
    } else {
        panic!("Expected schema");
    }
}

// 2. Serde Renaming Tests
#[test]
fn test_serde_rename_enum_all() {
    let code = r#"
        /// @openapi
        #[serde(rename_all = "camelCase")]
        pub enum Status {
            PendingValidation,
            CompletedSuccess,
        }
    "#;

    let mut visitor = OpenApiVisitor::default();
    let syntax = syn::parse_str::<syn::File>(code).unwrap();
    visitor.visit_file(&syntax);

    let content =
        if let oas_forge::visitor::ExtractedItem::Schema { content, .. } = &visitor.items[0] {
            content
        } else {
            panic!("Expected schema");
        };
    let json = content;
    let schema: Value = serde_yaml::from_str(json.as_str()).unwrap();
    let variants = schema["components"]["schemas"]["Status"]["enum"]
        .as_array()
        .expect("Enum variants not found");

    assert!(variants.contains(&Value::String("pendingValidation".to_string())));
    assert!(variants.contains(&Value::String("completedSuccess".to_string())));
}

#[test]
fn test_serde_rename_variant() {
    let code = r#"
        /// @openapi
        pub enum Kind {
            #[serde(rename = "special_kind")]
            Special,
            Normal
        }
    "#;

    let mut visitor = OpenApiVisitor::default();
    let syntax = syn::parse_str::<syn::File>(code).unwrap();
    visitor.visit_file(&syntax);

    let content =
        if let oas_forge::visitor::ExtractedItem::Schema { content, .. } = &visitor.items[0] {
            content
        } else {
            panic!("Expected schema");
        };
    let json = content;
    let schema: Value = serde_yaml::from_str(json.as_str()).unwrap();
    let variants = schema["components"]["schemas"]["Kind"]["enum"]
        .as_array()
        .expect("Enum variants not found");

    assert!(variants.contains(&Value::String("special_kind".to_string())));
    assert!(variants.contains(&Value::String("Normal".to_string())));
}

// 3. Manual @openapi rename
#[test]
fn test_manual_rename_struct() {
    let code = r#"
        /// @openapi rename-all snake_case
        /// @openapi rename "CustomUser"
        pub struct User {
            pub first_name: String,
            /// @openapi rename "last_name_secret"
            pub lastName: String,
        }
    "#;

    let mut visitor = OpenApiVisitor::default();
    let syntax = syn::parse_str::<syn::File>(code).unwrap();
    visitor.visit_file(&syntax);

    if let oas_forge::visitor::ExtractedItem::Schema { name, content, .. } = &visitor.items[0] {
        assert_eq!(name.as_ref().unwrap(), "CustomUser");
        let schema: Value = serde_yaml::from_str(content.as_str()).unwrap();
        let props = schema["components"]["schemas"]["CustomUser"]["properties"]
            .as_object()
            .expect("Properties not found");

        assert!(props.contains_key("first_name")); // snake_case applied
        assert!(props.contains_key("last_name_secret")); // override applied
    } else {
        panic!("Expected schema");
    }
}

// 4. @insert in doc comments
#[test]
fn test_insert_in_doc_comments() {
    let mut registry = Registry::new();
    registry.insert_fragment(
        "CommonParams".to_string(),
        vec![],
        "- name: id\n  in: query".to_string(),
    );

    let input = r#"
        /// List items
        ///
        /// @insert CommonParams
        fn list() {}
    "#;

    // We expect the preprocessor to detect it's inside a doc comment block (textually) or logic handles it.
    // Wait, the preprocessor runs on raw string BEFORE parsing.
    // So if the input is:
    // /// @insert CommonParams
    // It should become:
    // /// - name: id
    // ///   in: query

    // Let's verify preprocessor behavior directly.
    let processed = preprocessor::preprocess(input, &registry);

    assert!(
        processed.contains("/// - name: id"),
        "Should preserve doc comment marker ///"
    );
    assert!(
        processed.contains("///   in: query"),
        "Should preserve indentation and marker"
    );
}

#[test]
fn test_well_known_types_resolution() {
    let code = r#"
        /// @openapi
        pub struct TimeLog {
            pub created_at: DateTime<Utc>,
            pub modified_at: Option<NaiveDateTime>,
            pub id: Uuid,
            pub counts: Vec<i32>,
        }
    "#;

    let mut visitor = OpenApiVisitor::default();
    let syntax = syn::parse_str::<syn::File>(code).unwrap();
    visitor.visit_file(&syntax);

    if let oas_forge::visitor::ExtractedItem::Schema { content, .. } = &visitor.items[0] {
        let schema: Value = serde_yaml::from_str(content.as_str()).unwrap();
        let props = schema["components"]["schemas"]["TimeLog"]["properties"]
            .as_object()
            .expect("Properties not found");

        let created_at = &props["created_at"];
        // Expecting resolved type, not Ref
        if created_at.get("$ref").is_some() {
            panic!("Regression: generic type not resolved: {:?}", created_at);
        }
        assert_eq!(created_at["type"], "string");
        assert_eq!(created_at["format"], "date-time");

        let modified_at = &props["modified_at"];
        assert_eq!(modified_at["type"], "string");
        assert_eq!(modified_at["format"], "date-time");

        let id = &props["id"];
        assert_eq!(id["type"], "string");
        assert_eq!(id["format"], "uuid");
    } else {
        panic!("Expected schema");
    }
}

#[test]
fn test_alias_resolution() {
    let code = r#"
        pub type DateTimeUtc = DateTime<Utc>;

        /// @openapi
        pub struct Log {
            pub dt: DateTimeUtc
        }
    "#;

    let mut visitor = OpenApiVisitor::default();
    let syntax = syn::parse_str::<syn::File>(code).unwrap();
    visitor.visit_file(&syntax);

    assert_eq!(visitor.items.len(), 2, "Should find Alias and Struct");

    let alias = visitor.items.iter().find(|i| matches!(i, oas_forge::visitor::ExtractedItem::Schema { name: Some(n), .. } if n == "DateTimeUtc"));
    assert!(alias.is_some(), "Alias schema not found");

    if let oas_forge::visitor::ExtractedItem::Schema { content, .. } = alias.unwrap() {
        let schema: Value = serde_yaml::from_str(content.as_str()).unwrap();
        let def = &schema["components"]["schemas"]["DateTimeUtc"];
        assert_eq!(def["type"], "string");
        assert_eq!(def["format"], "date-time");
    }

    let log = visitor.items.iter().find(|i| matches!(i, oas_forge::visitor::ExtractedItem::Schema { name: Some(n), .. } if n == "Log"));
    assert!(log.is_some(), "Log schema not found");

    if let oas_forge::visitor::ExtractedItem::Schema { content, .. } = log.unwrap() {
        let schema: Value = serde_yaml::from_str(content.as_str()).unwrap();
        let prop = &schema["components"]["schemas"]["Log"]["properties"]["dt"];
        assert_eq!(prop["type"], "string");
        assert_eq!(prop["format"], "date-time");
    }
}

#[test]
fn test_datetimeutc_literal() {
    let code = r#"
        /// @openapi
        pub struct RawLog {
            pub dt: DateTimeUtc,
            pub dt2: Option<DateTimeUtc>
        }
    "#;

    let mut visitor = OpenApiVisitor::default();
    let syntax = syn::parse_str::<syn::File>(code).unwrap();
    visitor.visit_file(&syntax);

    if let oas_forge::visitor::ExtractedItem::Schema { content, .. } = &visitor.items[0] {
        let schema: Value = serde_yaml::from_str(content.as_str()).unwrap();
        let props = &schema["components"]["schemas"]["RawLog"]["properties"];

        // Assert we WANT resolved type
        let dt = &props["dt"];
        assert_eq!(dt["type"], "string");
        assert_eq!(dt["format"], "date-time");
    }
}

#[test]
fn test_route_dsl_fragment_insertion() {
    // 1. Setup Registry with Fragment
    let mut registry = Registry::new();
    registry.insert_fragment(
        "QueryParams".to_string(),
        vec![],
        "- name: limit\n  in: query\n  schema: { type: integer }".to_string(),
    );

    // 2. Define DSL with @insert
    // Format mimics extracted doc lines (stripped of ///)
    let doc_content = "
@route GET /list
parameters:
  @insert QueryParams
";

    // 3. Preprocess (Pass 2b)
    let expanded = preprocessor::preprocess(doc_content, &registry);

    // Check expansion first
    assert!(expanded.contains("- name: limit"));

    // 4. Compile (Pass 2c)
    let lines: Vec<String> = expanded.lines().map(|s| s.to_string()).collect();
    let yaml = oas_forge::dsl::parse_route_dsl(&lines, "list_op").expect("DSL Parsing failed");

    // 5. Verify YAML
    let root: Value = serde_yaml::from_str(&yaml).unwrap();
    let params = root["paths"]["/list"]["get"]["parameters"]
        .as_array()
        .expect("No params");

    assert_eq!(params.len(), 1);
    assert_eq!(params[0]["name"], "limit");
}

#[test]
fn test_fixed_response_insert() {
    // 1. Same Fragment
    let mut registry = Registry::new();
    registry.insert_fragment(
        "NotFound".to_string(),
        vec![],
        "'404':\n  description: Not Found".to_string(),
    );

    // 2. Insert NESTED under responses: (Correct usage)
    let doc_content = "
@route GET /test
responses:
  @insert NotFound
";

    let expanded = preprocessor::preprocess(doc_content, &registry);
    let lines: Vec<String> = expanded.lines().map(|s| s.to_string()).collect();
    let yaml = oas_forge::dsl::parse_route_dsl(&lines, "op").unwrap();

    // 3. Expectation: Parsed associated into responses
    let root: Value = serde_yaml::from_str(&yaml).unwrap();

    // Description should NOT contain the YAML
    if let Some(d) = root["paths"]["/test"]["get"].get("description") {
        assert!(!d.as_str().unwrap().contains("'404':"));
    }

    let responses = root["paths"]["/test"]["get"]["responses"]
        .as_object()
        .unwrap();
    assert!(responses.contains_key("404"));
    assert_eq!(responses["404"]["description"], "Not Found");
}

#[test]
fn test_insert_params_in_dsl() {
    let code = r#"
        /// @route GET /items
        /// @tag Items
        ///
        /// parameters:
        ///   - name: p
        ///     in: query
        ///     schema: { type: integer }
        fn list_items() {}
    "#;

    let mut visitor = OpenApiVisitor::default();
    let syntax = syn::parse_str::<syn::File>(code).unwrap();
    visitor.visit_file(&syntax);

    let path_item = visitor.items.last().unwrap();

    if let oas_forge::visitor::ExtractedItem::RouteDSL {
        content,
        operation_id,
        ..
    } = path_item
    {
        let lines: Vec<String> = content.lines().map(|s| s.to_string()).collect();
        let yaml =
            oas_forge::dsl::parse_route_dsl(&lines, operation_id).expect("Failed to parse DSL");

        let root: Value = serde_yaml::from_str(&yaml).unwrap();
        let get = &root["paths"]["/items"]["get"];

        let params = get["parameters"]
            .as_array()
            .expect("Parameters should be an array");
        assert_eq!(params.len(), 1);
        assert_eq!(params[0]["name"], "p");
    } else {
        panic!("Expected RouteDSL item, got {:?}", path_item);
    }
}
