//! Type aliases.

use crate::common::{codes, extract, extract_with_diagnostics, schema, schema_names};
use oas_forge::diagnostics::Code;
use oas_forge::extract::Item;
use serde_json::json;

#[test]
fn alias_is_reflected_with_overrides() {
    let items = extract(
        r#"
        /// @openapi
        /// format: uuid
        /// description: User ID Alias
        type UserId = String;
        "#,
    );
    assert_eq!(
        schema(&items, "UserId"),
        json!({ "type": "string", "format": "uuid", "description": "User ID Alias" })
    );
}

#[test]
fn aliases_are_exported_even_without_openapi_tag() {
    let items = extract(
        r#"
        pub type DateTimeUtc = DateTime<Utc>;

        /// @openapi
        pub struct Log { pub dt: DateTimeUtc }
        "#,
    );
    assert_eq!(schema_names(&items), ["DateTimeUtc", "Log"]);
    assert_eq!(
        schema(&items, "DateTimeUtc"),
        json!({ "type": "string", "format": "date-time" })
    );
    assert_eq!(
        schema(&items, "Log")["properties"]["dt"],
        json!({ "type": "string", "format": "date-time" })
    );
}

#[test]
fn doc_lines_before_tag_become_description() {
    let items =
        extract("/// Identifier of a user.\n/// @openapi format: uuid\ntype UserId = String;");
    assert_eq!(
        schema(&items, "UserId"),
        json!({ "type": "string", "format": "uuid", "description": "Identifier of a user." })
    );
}

#[test]
fn alias_to_custom_type_is_a_smart_ref() {
    let items = extract(
        "#[allow(dead_code)]\n#[doc(hidden)]\n#[doc = include_str!(\"x.md\")]\n#[doc = 5]\ntype Owner = User;",
    );
    assert_eq!(schema(&items, "Owner"), json!({ "$ref": "$User" }));
}

#[test]
fn renamed_aliases_keep_their_rust_name() {
    let items = extract("/// @openapi rename \"OwnerRef\"\ntype Owner = User;");
    match &items[0] {
        Item::Schema(s) => assert_eq!(
            (s.name.as_str(), s.rust_name.as_str()),
            ("OwnerRef", "Owner")
        ),
        other => panic!("expected a schema, got {other:?}"),
    }
}

#[test]
fn invalid_override_is_reported_and_ignored() {
    let (items, diagnostics) =
        extract_with_diagnostics("/// @openapi\n/// format: [unclosed\ntype Id = u64;");
    assert_eq!(
        schema(&items, "Id"),
        json!({ "type": "integer", "format": "int64", "minimum": 0 })
    );
    assert_eq!(codes(&diagnostics), [Code::InvalidOverride]);
}

#[test]
fn optional_aliases_are_nullable() {
    let items = extract("type MaybeName = Option<String>;");
    assert_eq!(
        schema(&items, "MaybeName"),
        json!({ "type": ["string", "null"] })
    );
}

#[test]
fn generic_aliases_are_blueprints() {
    let items = extract("pub type Paged<T> = Page<T>;");
    let paged = crate::extract::blueprint(&items, "Paged");
    assert_eq!(paged.params, ["T"]);
    assert_eq!(paged.schema, json!({ "$ref": "$Page<$T>" }));
}

#[test]
fn generic_aliases_are_instantiated_where_used() {
    let (document, diagnostics) = crate::common::pipeline(&[(
        "lib.rs",
        r#"
        pub type Result<T> = std::result::Result<T, AppError>;
        pub type Paged<T> = Page<T>;
        /// @openapi
        pub struct Page<T> { pub items: Vec<T> }
        /// @openapi
        pub struct User { pub id: u64 }
        /// @openapi
        pub struct H { pub p: Paged<User> }
        "#,
    )]);
    assert!(diagnostics.is_empty(), "{diagnostics:?}");
    let schemas = &document["components"]["schemas"];
    assert_eq!(
        schemas["Paged_User"],
        json!({ "$ref": "#/components/schemas/Page_User" })
    );
    assert_eq!(
        schemas["Page_User"]["properties"]["items"]["items"],
        json!({ "$ref": "#/components/schemas/User" })
    );
    // Unused generic aliases (`Result<T>`) are not exported.
    assert!(schemas.get("Result").is_none() && schemas.get("Paged").is_none());
}
