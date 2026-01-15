use oas_forge::visitor::ExtractedItem;
use oas_forge::visitor::OpenApiVisitor;
use serde_json::Value;
use syn::ItemEnum;
use syn::parse_quote;
use syn::visit::Visit;

#[test]
fn test_enum_tagged_reproduction() {
    let code: ItemEnum = parse_quote! {
        /// Configuration for a storage location.
        /// @openapi
        #[derive(Serialize)]
        #[serde(tag = "type", rename_all = "snake_case")]
        pub enum StorageConfigDto {
            Local { root_path: String },
            StarrRemote { url: String, secret: String },
        }
    };

    let mut visitor = OpenApiVisitor::default();
    visitor.visit_item_enum(&code);

    // Check main schema (discriminator container)
    let main = visitor.items.iter().find(
        |i| matches!(i, ExtractedItem::Schema { name: Some(n), .. } if n == "StorageConfigDto"),
    );
    assert!(
        main.is_some(),
        "Should extract StorageConfigDto (Main Schema)"
    );

    if let ExtractedItem::Schema { content, .. } = main.unwrap() {
        let schema: Value = serde_yaml::from_str(content).expect("Valid YAML");
        let def = &schema["components"]["schemas"]["StorageConfigDto"];

        // 1. Should be oneOf
        let one_of = def["oneOf"].as_array().expect("Should have oneOf");
        assert_eq!(one_of.len(), 2);

        // 2. Discriminator
        let disc = &def["discriminator"];
        assert_eq!(disc["propertyName"], "type");
        let mapping = &disc["mapping"];
        assert_eq!(
            mapping["local"],
            "#/components/schemas/StorageConfigDtoLocal"
        );
        assert_eq!(
            mapping["starr_remote"],
            "#/components/schemas/StorageConfigDtoStarrRemote"
        );
    }

    // Check Local
    let local = visitor.items.iter().find(|i| matches!(i, ExtractedItem::Schema { name: Some(n), .. } if n == "StorageConfigDtoLocal"));
    assert!(local.is_some(), "Should extract StorageConfigDtoLocal");
    if let ExtractedItem::Schema { content, .. } = local.unwrap() {
        let schema: Value = serde_yaml::from_str(content).unwrap();
        let props = &schema["components"]["schemas"]["StorageConfigDtoLocal"]["properties"];
        let type_field = &props["type"];
        assert_eq!(type_field["enum"][0], "local");
        assert_eq!(type_field["type"], "string");

        // Required fields
        let req = schema["components"]["schemas"]["StorageConfigDtoLocal"]["required"]
            .as_array()
            .unwrap();
        assert!(req.contains(&serde_json::json!("type")));
        assert!(req.contains(&serde_json::json!("root_path")));
    }

    // Check StarrRemote
    let remote = visitor.items.iter().find(|i| matches!(i, ExtractedItem::Schema { name: Some(n), .. } if n == "StorageConfigDtoStarrRemote"));
    assert!(
        remote.is_some(),
        "Should extract StorageConfigDtoStarrRemote"
    );
    match remote.unwrap() {
        ExtractedItem::Schema { content, .. } => {
            let schema: Value = serde_yaml::from_str(content).unwrap();
            let props =
                &schema["components"]["schemas"]["StorageConfigDtoStarrRemote"]["properties"];
            let type_field = &props["type"];
            assert_eq!(type_field["enum"][0], "starr_remote");

            let req = schema["components"]["schemas"]["StorageConfigDtoStarrRemote"]["required"]
                .as_array()
                .unwrap();
            assert!(req.contains(&serde_json::json!("type")));
            assert!(req.contains(&serde_json::json!("url")));
        }
        _ => panic!(),
    }
}
