use oas_forge::visitor::ExtractedItem;
use oas_forge::visitor::OpenApiVisitor;
use serde_json::Value;
use syn::ItemEnum;
use syn::parse_quote;
use syn::visit::Visit;

#[test]
fn test_enum_variant_validation() {
    let code: ItemEnum = parse_quote! {
        /// @openapi
        #[derive(Serialize)]
        #[serde(tag = "type")]
        pub enum ValidatedEnum {
            Variant {
                #[validate(length(min = 5))]
                name: String,
            }
        }
    };

    let mut visitor = OpenApiVisitor::default();
    visitor.visit_item_enum(&code);

    // Find the variant schema "ValidatedEnumVariant"
    let variant = visitor.items.iter().find(
        |i| matches!(i, ExtractedItem::Schema { name: Some(n), .. } if n == "ValidatedEnumVariant"),
    );
    assert!(variant.is_some(), "Should extract variant schema");

    if let ExtractedItem::Schema { content, .. } = variant.unwrap() {
        let schema: Value = serde_yaml::from_str(content).expect("Valid YAML");
        let item = &schema["components"]["schemas"]["ValidatedEnumVariant"];
        let props = &item["properties"];

        let name_schema = &props["name"];

        // Debug output
        println!("Name schema: {}", name_schema);

        // Check for minLength: 5
        let min_len = name_schema.get("minLength");
        assert!(min_len.is_some(), "Should have minLength validation");
        assert_eq!(min_len.unwrap().as_i64(), Some(5));
    }
}

#[test]
fn test_enum_tuple_variant_validation() {
    let code: ItemEnum = parse_quote! {
        /// @openapi
        #[derive(Serialize)]
        #[serde(tag = "type", content = "content")]
        pub enum ValidatedTupleEnum {
            Tuple(
                #[validate(length(min = 10))]
                String
            )
        }
    };

    let mut visitor = OpenApiVisitor::default();
    visitor.visit_item_enum(&code);

    let variant = visitor.items.iter().find(|i| matches!(i, ExtractedItem::Schema { name: Some(n), .. } if n == "ValidatedTupleEnumTuple"));
    assert!(variant.is_some());

    if let ExtractedItem::Schema { content, .. } = variant.unwrap() {
        let schema: Value = serde_yaml::from_str(content).expect("Valid YAML");
        let item = &schema["components"]["schemas"]["ValidatedTupleEnumTuple"];
        let props = &item["properties"];
        let content_field = &props["content"]; // Adjacently tagged content

        println!("Content schema: {}", content_field);

        let min_len = content_field.get("minLength");
        assert!(
            min_len.is_some(),
            "Tuple field should have minLength validation"
        );
        assert_eq!(min_len.unwrap().as_i64(), Some(10));
    }
}
