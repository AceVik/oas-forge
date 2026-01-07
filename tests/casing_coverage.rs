use oas_forge::visitor::ExtractedItem;
use oas_forge::visitor::OpenApiVisitor;
use serde_json::Value;
use syn::ItemStruct;
use syn::parse_quote;
use syn::visit::Visit;

#[test]
fn test_snake_to_camel_case() {
    let code: ItemStruct = parse_quote! {
        /// @openapi rename-all camelCase
        pub struct UserProfile {
            pub birth_place: String,
            pub created_at: String,
            pub simple_id: i32,
        }
    };

    let mut visitor = OpenApiVisitor::default();
    visitor.visit_item_struct(&code);

    let item = visitor.items.first().expect("Should extract item");
    if let ExtractedItem::Schema { content, .. } = item {
        let schema: Value = serde_yaml::from_str(content).expect("Valid YAML");
        let props = schema["components"]["schemas"]["UserProfile"]["properties"]
            .as_object()
            .expect("Properties object");

        assert!(
            props.contains_key("birthPlace"),
            "birth_place -> birthPlace"
        );
        assert!(props.contains_key("createdAt"), "created_at -> createdAt");
        assert!(props.contains_key("simpleId"), "simple_id -> simpleId");
    } else {
        panic!("Expected Schema item");
    }
}

#[test]
fn test_snake_to_pascal_case() {
    let code: ItemStruct = parse_quote! {
        /// @openapi rename-all PascalCase
        pub struct SystemConfig {
            pub max_connections: i32,
            pub timeout_ms: i32,
            pub is_active: bool,
        }
    };

    let mut visitor = OpenApiVisitor::default();
    visitor.visit_item_struct(&code);

    let item = visitor.items.first().expect("Should extract item");
    if let ExtractedItem::Schema { content, .. } = item {
        let schema: Value = serde_yaml::from_str(content).expect("Valid YAML");
        let props = schema["components"]["schemas"]["SystemConfig"]["properties"]
            .as_object()
            .expect("Properties object");

        assert!(
            props.contains_key("MaxConnections"),
            "max_connections -> MaxConnections"
        );
        assert!(props.contains_key("TimeoutMs"), "timeout_ms -> TimeoutMs");
        assert!(props.contains_key("IsActive"), "is_active -> IsActive");
    } else {
        panic!("Expected Schema item");
    }
}

#[test]
fn test_serde_rename_all_precedence() {
    let code: ItemStruct = parse_quote! {
        /// @openapi
        #[derive(Serialize)]
        #[serde(rename_all = "camelCase")]
        pub struct ApiError {
            pub error_code: String,
            pub error_message: String,
        }
    };

    let mut visitor = OpenApiVisitor::default();
    visitor.visit_item_struct(&code);

    let item = visitor.items.first().expect("Should extract item");
    if let ExtractedItem::Schema { content, .. } = item {
        let schema: Value = serde_yaml::from_str(content).expect("Valid YAML");
        let props = schema["components"]["schemas"]["ApiError"]["properties"]
            .as_object()
            .expect("Properties object");

        assert!(
            props.contains_key("errorCode"),
            "error_code -> errorCode (via serde)"
        );
        assert!(
            props.contains_key("errorMessage"),
            "error_message -> errorMessage (via serde)"
        );
    }
}
