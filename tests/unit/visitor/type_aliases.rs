use crate::common::{extract, schema, schema_names};
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
fn invalid_override_is_ignored() {
    let items = extract("/// @openapi\n/// format: [unclosed\ntype Id = u64;");
    assert_eq!(
        schema(&items, "Id"),
        json!({ "type": "integer", "format": "int64" })
    );
}
