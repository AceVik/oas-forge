use oas_forge::generics::Monomorphizer;
use oas_forge::index::Registry;

#[test]
fn test_monomorphization_naming() {
    let mut registry = Registry::new();
    registry.insert_blueprint(
        "Page".to_string(),
        vec!["T".to_string()],
        "data: $ref: $T".to_string(),
    );

    let mut mono = Monomorphizer::new(&mut registry);
    let result = mono.process("$Page<User>");

    // Page<User> -> Page_User
    assert_eq!(result, "$Page_User");
}

#[test]
fn test_complex_generic_nesting() {
    let mut registry = Registry::new();
    registry.insert_blueprint(
        "Page".to_string(),
        vec!["T".to_string()],
        "items: $T".to_string(),
    );
    registry.insert_blueprint(
        "Wrapper".to_string(),
        vec!["W".to_string()],
        "wrapped: $W".to_string(),
    );
    registry.insert_blueprint(
        "User".to_string(), // Fake blueprint or just schema context?
        // Generics engine doesn't care if leaf is blueprint or schema, it just outputs name.
        vec![],
        "type: object".to_string(),
    );

    let mut mono = Monomorphizer::new(&mut registry);

    // Page<Wrapper<User>> -> Page_Wrapper_User
    // Inner: Wrapper<User> -> Wrapper_User
    // Outer: Page<Wrapper_User> -> Page_Wrapper_User

    let result = mono.process("$Page<$Wrapper<User>>");
    assert_eq!(result, "$Page_Wrapper_User");

    // Check Registry for generated schemas
    assert!(registry.concrete_schemas.contains_key("Wrapper_User"));
    assert!(registry.concrete_schemas.contains_key("Page_Wrapper_User"));

    // Check content substitution
    // Wrapper_User should have "wrapped: $User"
    let wrapper_user = registry.concrete_schemas.get("Wrapper_User").unwrap();
    assert_eq!(wrapper_user, "wrapped: $User");

    // Page_Wrapper_User should have "items: $Wrapper_User"
    // Because Page blueprint says "items: $T" and T is "Wrapper_User"
    let page_wrapper = registry.concrete_schemas.get("Page_Wrapper_User").unwrap();
    assert_eq!(page_wrapper, "items: $Wrapper_User");
}

#[test]
fn test_multiple_params() {
    let mut registry = Registry::new();
    registry.insert_blueprint(
        "Map".to_string(),
        vec!["K".to_string(), "V".to_string()],
        "key: $K\nvalue: $V".to_string(),
    );

    let mut mono = Monomorphizer::new(&mut registry);
    let result = mono.process("$Map<String, User>");

    assert_eq!(result, "$Map_String_User");

    let schema = registry.concrete_schemas.get("Map_String_User").unwrap();
    assert!(schema.contains("key: $String"));
    assert!(schema.contains("value: $User"));
}
