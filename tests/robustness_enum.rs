use oas_forge::visitor::ExtractedItem;
use oas_forge::visitor::OpenApiVisitor;
use serde_json::Value;
use syn::ItemEnum;
use syn::parse_quote;
use syn::visit::Visit;

#[test]
fn test_adjacently_tagged_enum_with_content() {
    let code: ItemEnum = parse_quote! {
        /// @openapi
        #[derive(Serialize)]
        #[serde(tag = "t", content = "c")]
        pub enum Adjacent {
            Str(String),
            Struct { x: i32 },
        }
    };

    let mut visitor = OpenApiVisitor::default();
    visitor.visit_item_enum(&code);

    let item = visitor
        .items
        .iter()
        .find(|i| matches!(i, ExtractedItem::Schema { name: Some(n), .. } if n == "Adjacent"))
        .expect("Should extract enum");
    if let ExtractedItem::Schema { content, name, .. } = item {
        assert_eq!(name.as_deref(), Some("Adjacent"));
        let schema: Value = serde_yaml::from_str(content).expect("Valid YAML");
        let def = &schema["components"]["schemas"]["Adjacent"];

        // 1. Check oneOf
        assert!(def["oneOf"].is_array());

        // 2. Discriminator
        let disc = &def["discriminator"];
        assert_eq!(disc["propertyName"], "t");
    }

    // Check Variants
    // Variant 1: AdjacentStr
    // Structure should be:
    // properties:
    //   t: { enum: ["Str"] }
    //   c: { type: string }
    let str_var = visitor
        .items
        .iter()
        .find(|i| matches!(i, ExtractedItem::Schema { name: Some(n), .. } if n == "AdjacentStr"));
    assert!(str_var.is_some());
    if let ExtractedItem::Schema { content, .. } = str_var.unwrap() {
        let schema: Value = serde_yaml::from_str(content).unwrap();
        let props = &schema["components"]["schemas"]["AdjacentStr"]["properties"];

        assert_eq!(props["t"]["enum"][0], "Str");
        // Content field "c" should hold the String schema
        assert_eq!(props["c"]["type"], "string");
        assert!(
            props.get("Str").is_none(),
            "Should not have inner fields at top level"
        );
    }

    // Variant 2: AdjacentStruct
    // Structure should be:
    // properties:
    //   t: { enum: ["Struct"] }
    //   c: { type: object, properties: { x: { type: integer } } }
    let struct_var = visitor.items.iter().find(
        |i| matches!(i, ExtractedItem::Schema { name: Some(n), .. } if n == "AdjacentStruct"),
    );
    assert!(struct_var.is_some());
    if let ExtractedItem::Schema { content, .. } = struct_var.unwrap() {
        let schema: Value = serde_yaml::from_str(content).unwrap();
        let props = &schema["components"]["schemas"]["AdjacentStruct"]["properties"];

        assert_eq!(props["t"]["enum"][0], "Struct");
        assert_eq!(props["c"]["type"], "object");
        assert_eq!(props["c"]["properties"]["x"]["type"], "integer");
    }
}
