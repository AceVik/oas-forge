use oas_forge::visitor::ExtractedItem;
use oas_forge::visitor::OpenApiVisitor;
use serde_json::Value;
use syn::ItemStruct;
use syn::parse_quote;
use syn::visit::Visit;

#[test]
fn test_validation_attributes() {
    let code: ItemStruct = parse_quote! {
        /// User DTO
        /// @openapi
        #[derive(Serialize, Validate)]
        pub struct UserDto {
            #[validate(email)]
            pub email: String,

            #[validate(url)]
            pub website: String,

            #[validate(length(min = 3, max = 20))]
            pub username: String,

            #[validate(range(min = 18, max = 100))]
            pub age: u8,

            #[validate(regex = "path::to::REGEX")]
            pub code: String,
        }
    };

    let mut visitor = OpenApiVisitor::default();
    visitor.visit_item_struct(&code);

    let item = visitor.items.first().expect("Should extract struct");
    if let ExtractedItem::Schema { content, .. } = item {
        let schema: Value = serde_yaml::from_str(content).expect("Valid YAML");
        let props = &schema["components"]["schemas"]["UserDto"]["properties"];

        // Check Email
        assert_eq!(props["email"]["format"], "email");

        // Check URL
        assert_eq!(props["website"]["format"], "uri");

        // Check Length
        assert_eq!(props["username"]["minLength"], 3);
        assert_eq!(props["username"]["maxLength"], 20);

        // Check Range
        assert_eq!(props["age"]["minimum"], 18);
        assert_eq!(props["age"]["maximum"], 100);

        // Check Regex (we likely won't resolve the path, but if we supported literal "regex = ...", we could check pattern.
        // For now, let's see if we can just detect presence or ignore complex ones gracefully.
        // If we implement basic path handling (just warning or ignoring), assertions might check for absence of crash)
        // Let's assume we won't extract "path::to::REGEX" comfortably yet without resolving.
    } else {
        panic!("Expected Schema item");
    }
}
