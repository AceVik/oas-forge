use oas_forge::visitor::ExtractedItem;
use oas_forge::visitor::OpenApiVisitor;
use syn::File;
use syn::parse_quote;
use syn::visit::Visit;

#[test]
fn test_virtual_routes() {
    let code: File = parse_quote! {
        //! Module level docs
        //!
        //! @route GET /virtual/users
        //! @return 200: "List of users"

        pub mod inner {}
    };

    let mut visitor = OpenApiVisitor::default();
    visitor.visit_file(&code);

    let item = visitor.items.first().expect("Should extract virtual route");
    if let ExtractedItem::RouteDSL {
        content,
        operation_id,
        ..
    } = item
    {
        assert!(content.contains("@route GET /virtual/users"));
        // Default operation ID logic might need to be checked or mocked
        // dsl.rs doesn't autogenerate operationId for virtual routes yet, or does it?
        // visitor.rs currently uses function name for operation_id in visit_item_fn.
        // For virtual routes, we need a strategy.
        // If we implement the plan, we might reuse `dsl::parse_route_dsl` which allows overrides.
        // But for extraction, we need to know what ID `visitor` assigned.
        // The implementation plan suggested `dsl.rs` parses it.
    } else {
        panic!("Expected RouteDSL item");
    }
}

#[test]
fn test_virtual_route_explicit_id() {
    let code: File = parse_quote! {
        //! @route POST /virtual/create
        //! operationId: createVirtualUser
        //! @return 201: "Created"
    };

    let mut visitor = OpenApiVisitor::default();
    visitor.visit_file(&code);

    let item = visitor.items.first().expect("Should extract virtual route");
    if let ExtractedItem::RouteDSL { content, .. } = item {
        assert!(content.contains("operationId: createVirtualUser"));
    }
}
