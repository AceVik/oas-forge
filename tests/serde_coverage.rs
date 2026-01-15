use oas_forge::visitor::ExtractedItem;
use oas_forge::visitor::OpenApiVisitor;
use serde_json::Value;
use syn::ItemEnum;
use syn::ItemStruct;
use syn::parse_quote;
use syn::visit::Visit;

#[test]
fn test_serde_rename_struct() {
    let code: ItemStruct = parse_quote! {
        /// @openapi
        #[derive(Serialize)]
        #[serde(rename = "RenamedStruct")]
        pub struct MyStruct {
            pub id: i32,
        }
    };

    let mut visitor = OpenApiVisitor::default();
    visitor.visit_item_struct(&code);

    let item = visitor.items.first().expect("Should extract item");
    if let ExtractedItem::Schema { content, .. } = item {
        let schema: Value = serde_yaml::from_str(content).expect("Valid YAML");
        assert!(
            schema["components"]["schemas"]
                .get("RenamedStruct")
                .is_some(),
            "Struct should be renamed to RenamedStruct"
        );
        assert!(
            schema["components"]["schemas"].get("MyStruct").is_none(),
            "Old struct name should not exist"
        );
    } else {
        panic!("Expected Schema item");
    }
}

#[test]
fn test_serde_rename_struct_field() {
    let code: ItemStruct = parse_quote! {
        /// @openapi
        #[derive(Serialize)]
        pub struct User {
            #[serde(rename = "user_identifier")]
            pub id: i32,
            pub name: String,
        }
    };

    let mut visitor = OpenApiVisitor::default();
    visitor.visit_item_struct(&code);

    let item = visitor.items.first().expect("Should extract item");
    if let ExtractedItem::Schema { content, .. } = item {
        let schema: Value = serde_yaml::from_str(content).expect("Valid YAML");
        let props = schema["components"]["schemas"]["User"]["properties"]
            .as_object()
            .expect("Properties object");

        assert!(
            props.contains_key("user_identifier"),
            "Field 'id' should be renamed to 'user_identifier'"
        );
        assert!(!props.contains_key("id"), "Field 'id' should NOT exist");
    }
}

#[test]
fn test_serde_rename_enum() {
    let code: ItemEnum = parse_quote! {
        /// @openapi
        #[derive(Serialize)]
        #[serde(rename = "RenamedEnum")]
        pub enum Status {
            Active,
            Inactive,
        }
    };

    let mut visitor = OpenApiVisitor::default();
    visitor.visit_item_enum(&code);

    let item = visitor.items.first().expect("Should extract item");
    if let ExtractedItem::Schema { content, .. } = item {
        let schema: Value = serde_yaml::from_str(content).expect("Valid YAML");
        assert!(
            schema["components"]["schemas"].get("RenamedEnum").is_some(),
            "Enum should be renamed to RenamedEnum"
        );
    }
}

#[test]
fn test_serde_rename_enum_variant() {
    let code: ItemEnum = parse_quote! {
        /// @openapi
        #[derive(Serialize)]
        pub enum Color {
            #[serde(rename = "RED_cOlOr")]
            Red,
            Blue,
        }
    };

    let mut visitor = OpenApiVisitor::default();
    visitor.visit_item_enum(&code);

    let item = visitor.items.first().expect("Should extract item");
    if let ExtractedItem::Schema { content, .. } = item {
        let schema: Value = serde_yaml::from_str(content).expect("Valid YAML");
        let enums = schema["components"]["schemas"]["Color"]["enum"]
            .as_array()
            .expect("Enum array");

        assert!(
            enums.contains(&serde_json::json!("RED_cOlOr")),
            "Variant 'Red' should be renamed to 'RED_cOlOr'"
        );
        assert!(
            !enums.contains(&serde_json::json!("Red")),
            "Variant 'Red' should NOT exist"
        );
        assert!(
            enums.contains(&serde_json::json!("Blue")),
            "Variant 'Blue' should exist"
        );
    }
}

#[test]
fn test_serde_rename_all_enum() {
    let code: ItemEnum = parse_quote! {
        /// @openapi
        #[derive(Serialize)]
        #[serde(rename_all = "snake_case")]
        pub enum AccessLevel {
            SuperAdmin,
            GuestUser,
        }
    };

    let mut visitor = OpenApiVisitor::default();
    visitor.visit_item_enum(&code);

    let item = visitor.items.first().expect("Should extract item");
    if let ExtractedItem::Schema { content, .. } = item {
        let schema: Value = serde_yaml::from_str(content).expect("Valid YAML");
        let enums = schema["components"]["schemas"]["AccessLevel"]["enum"]
            .as_array()
            .expect("Enum array");

        assert!(
            enums.contains(&serde_json::json!("super_admin")),
            "SuperAdmin -> super_admin"
        );
        assert!(
            enums.contains(&serde_json::json!("guest_user")),
            "GuestUser -> guest_user"
        );
    }
}
