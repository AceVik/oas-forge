use crate::common::{extract, schema, schema_names};
use oas_forge::visitor::ExtractedItem;
use serde_json::json;

#[test]
fn structs_without_openapi_tag_are_not_exported() {
    let items = extract("/// Hidden internal struct\nstruct Internal { secret: String }");
    assert!(items.is_empty());
}

#[test]
fn named_fields_become_properties_and_required_list() {
    let items = extract(
        r#"
        /// @openapi
        struct MyStruct {
            pub id: String,
            pub count: i32,
            pub active: bool,
            pub tags: Vec<String>,
            pub meta: Option<String>,
        }
        "#,
    );

    assert_eq!(
        schema(&items, "MyStruct"),
        json!({
            "type": "object",
            "properties": {
                "id": { "type": "string" },
                "count": { "type": "integer", "format": "int32" },
                "active": { "type": "boolean" },
                "tags": { "type": "array", "items": { "type": "string" } },
                "meta": { "type": "string" }
            },
            "required": ["id", "count", "active", "tags"]
        })
    );
}

#[test]
fn all_optional_fields_omit_required_list() {
    let items = extract("/// @openapi\nstruct Filter { pub q: Option<String> }");
    assert!(schema(&items, "Filter").get("required").is_none());
}

#[test]
fn unit_struct_is_a_plain_object() {
    let items = extract("/// @openapi\nstruct Marker;");
    assert_eq!(schema(&items, "Marker"), json!({ "type": "object" }));
}

#[test]
fn unit_struct_can_define_a_virtual_type() {
    let items = extract(
        r#"
        /// @openapi
        /// type: string
        /// enum: [A, B]
        struct MyEnum;
        "#,
    );
    assert_eq!(
        schema(&items, "MyEnum"),
        json!({ "type": "string", "enum": ["A", "B"] })
    );
}

#[test]
fn generic_type_parameters_become_smart_refs() {
    let items = extract("/// @openapi\nstruct Container<T> { pub item: T, pub items: Vec<T> }");
    let props = &schema(&items, "Container")["properties"];
    assert_eq!(props["item"], json!({ "$ref": "$T" }));
    assert_eq!(props["items"]["items"], json!({ "$ref": "$T" }));
}

#[test]
fn well_known_types_are_resolved_not_referenced() {
    let items = extract(
        r#"
        /// @openapi
        struct TimeLog {
            pub id: Uuid,
            pub created_at: DateTime<Utc>,
            pub modified_at: Option<NaiveDateTime>,
            pub raw: DateTimeUtc,
            pub scores: Vec<f64>,
            pub metadata: HashMap<String, String>,
        }
        "#,
    );
    let props = &schema(&items, "TimeLog")["properties"];
    assert_eq!(props["id"], json!({ "type": "string", "format": "uuid" }));
    for field in ["created_at", "modified_at", "raw"] {
        assert_eq!(
            props[field],
            json!({ "type": "string", "format": "date-time" }),
            "{field}"
        );
    }
    assert_eq!(props["scores"]["items"]["format"], "double");
    assert_eq!(
        props["metadata"]["additionalProperties"],
        json!({ "type": "string" })
    );
}

mod documentation {
    use super::*;

    #[test]
    fn doc_comment_lines_become_description() {
        let items = extract(
            r#"
            /// This is a user struct.
            /// It has multiple lines.
            /// @openapi
            struct User { name: String }
            "#,
        );
        assert_eq!(
            schema(&items, "User")["description"],
            "This is a user struct. It has multiple lines."
        );
    }

    #[test]
    fn explicit_description_override_wins() {
        let items = extract(
            r#"
            /// Original Docs
            /// @openapi
            /// description: Overridden
            struct User { name: String }
            "#,
        );
        assert_eq!(schema(&items, "User")["description"], "Overridden");
    }

    #[test]
    fn yaml_payload_is_not_part_of_the_description() {
        let items = extract(
            r#"
            /// A user account in the system.
            /// @openapi
            /// type: object
            /// required:
            ///   - id
            struct User { id: u64, name: String }
            "#,
        );
        let user = schema(&items, "User");
        assert_eq!(user["description"], "A user account in the system.");
        assert_eq!(user["required"], json!(["id"]));
    }

    #[test]
    fn field_docs_become_field_descriptions() {
        let items = extract(
            r#"
            /// @openapi
            struct Clean {
                /// Clean Description
                /// @openapi example: "dirty"
                pub field: String,
            }
            "#,
        );
        assert_eq!(
            schema(&items, "Clean")["properties"]["field"],
            json!({ "type": "string", "description": "Clean Description", "example": "dirty" })
        );
    }
}

mod overrides {
    use super::*;

    #[test]
    fn inline_struct_override_on_tag_line() {
        let items = extract("/// @openapi description: Inline\nstruct S { a: i32 }");
        assert_eq!(schema(&items, "S")["description"], "Inline");
    }

    #[test]
    fn multi_line_field_override() {
        let items = extract(
            r#"
            /// @openapi
            struct User {
                /// @openapi
                /// example:
                ///   - "Alice"
                ///   - "Bob"
                pub names: Vec<String>
            }
            "#,
        );
        assert_eq!(
            schema(&items, "User")["properties"]["names"]["example"],
            json!(["Alice", "Bob"])
        );
    }

    #[test]
    fn invalid_field_override_is_ignored() {
        let items = extract(
            r#"
            /// @openapi
            struct S {
                /// @openapi
                /// example: [unclosed
                pub a: i32
            }
            "#,
        );
        assert_eq!(
            schema(&items, "S")["properties"]["a"],
            json!({ "type": "integer", "format": "int32" })
        );
    }

    #[test]
    fn invalid_struct_override_is_ignored() {
        let items = extract("/// @openapi\n/// type: [unclosed\nstruct S { a: i32 }");
        assert_eq!(schema(&items, "S")["type"], "object");
    }

    #[test]
    fn openapi_type_marker_exports_under_struct_name() {
        let items = extract("/// @openapi-type Ignored\nstruct Real { a: i32 }");
        assert_eq!(schema_names(&items), ["Real"]);
    }
}

mod renaming {
    use super::*;

    fn property_names(code: &str, name: &str) -> Vec<String> {
        let items = extract(code);
        let mut names: Vec<String> = schema(&items, name)["properties"]
            .as_object()
            .expect("properties")
            .keys()
            .cloned()
            .collect();
        names.sort();
        names
    }

    #[test]
    fn serde_rename_renames_the_schema() {
        let items = extract(
            "/// @openapi\n#[serde(rename = \"RenamedStruct\")]\npub struct MyStruct { pub id: i32 }",
        );
        assert_eq!(schema_names(&items), ["RenamedStruct"]);
    }

    #[test]
    fn openapi_rename_overrides_serde_rename() {
        let items = extract(
            r#"
            /// @openapi-type User
            /// @openapi rename "UserDTO"
            #[serde(rename = "SerdeName")]
            struct User { id: u64 }
            "#,
        );
        assert_eq!(schema_names(&items), ["UserDTO"]);
    }

    #[test]
    fn serde_field_rename() {
        assert_eq!(
            property_names(
                r#"
                /// @openapi
                pub struct User {
                    #[serde(rename = "user_identifier")]
                    pub id: i32,
                    pub name: String,
                }
                "#,
                "User"
            ),
            ["name", "user_identifier"]
        );
    }

    #[test]
    fn openapi_rename_all_camel_case() {
        assert_eq!(
            property_names(
                r#"
                /// @openapi rename-all camelCase
                pub struct UserProfile { pub birth_place: String, pub created_at: String, pub simple_id: i32 }
                "#,
                "UserProfile"
            ),
            ["birthPlace", "createdAt", "simpleId"]
        );
    }

    #[test]
    fn openapi_rename_all_pascal_case() {
        assert_eq!(
            property_names(
                r#"
                /// @openapi rename-all PascalCase
                pub struct SystemConfig { pub max_connections: i32, pub timeout_ms: i32, pub is_active: bool }
                "#,
                "SystemConfig"
            ),
            ["IsActive", "MaxConnections", "TimeoutMs"]
        );
    }

    #[test]
    fn serde_rename_all() {
        assert_eq!(
            property_names(
                r#"
                /// @openapi
                #[serde(rename_all = "camelCase")]
                pub struct ApiError { pub error_code: String, pub error_message: String }
                "#,
                "ApiError"
            ),
            ["errorCode", "errorMessage"]
        );
    }

    #[test]
    fn explicit_field_rename_wins_over_rename_all() {
        let items = extract(
            r#"
            /// @openapi rename-all snake_case
            /// @openapi rename "CustomUser"
            pub struct User {
                pub first_name: String,
                /// @openapi rename "last_name_secret"
                pub lastName: String,
            }
            "#,
        );
        assert_eq!(schema_names(&items), ["CustomUser"]);
        let props = &schema(&items, "CustomUser")["properties"];
        assert!(props.get("first_name").is_some());
        assert!(props.get("last_name_secret").is_some());
        assert!(props.get("last_name").is_none());
    }
}

#[test]
fn validation_attributes_are_merged_into_fields() {
    let items = extract(
        r#"
        /// @openapi
        pub struct UserDto {
            #[validate(email)]
            pub email: String,
            #[validate(url)]
            pub website: String,
            #[validate(length(min = 3, max = 20))]
            pub username: String,
            #[validate(range(min = 18, max = 100))]
            pub age: u8,
        }
        "#,
    );
    let props = &schema(&items, "UserDto")["properties"];
    assert_eq!(props["email"]["format"], "email");
    assert_eq!(props["website"]["format"], "uri");
    assert_eq!(props["username"]["minLength"], 3);
    assert_eq!(props["username"]["maxLength"], 20);
    assert_eq!(props["age"]["minimum"], 18);
    assert_eq!(props["age"]["maximum"], 100);
}

mod blueprints {
    use super::*;

    fn blueprint(items: &[ExtractedItem]) -> (&str, &[String], &str) {
        match items {
            [
                ExtractedItem::Blueprint {
                    name,
                    params,
                    content,
                    ..
                },
            ] => (name, params, content),
            other => panic!("expected a single blueprint, got {other:#?}"),
        }
    }

    #[test]
    fn generic_header_emits_unwrapped_blueprint() {
        let items = extract(
            r#"
            /// @openapi<T>
            /// properties:
            ///   total: { type: integer }
            struct Page<T> { items: Vec<T> }
            "#,
        );
        let (name, params, content) = blueprint(&items);
        assert_eq!(name, "Page");
        assert_eq!(params, ["T"]);
        assert!(
            !content.contains("components:"),
            "blueprints are not wrapped"
        );
        assert_eq!(
            crate::common::yaml(content)["properties"],
            json!({
                "items": { "type": "array", "items": { "$ref": "$T" } },
                "total": { "type": "integer" }
            }),
            "reflected fields and overrides are merged"
        );
    }

    #[test]
    fn unterminated_generic_header_is_a_plain_schema() {
        let items = extract("/// @openapi<T\nstruct Page<T> { items: Vec<T> }");
        assert_eq!(schema_names(&items), ["Page"]);
    }

    #[test]
    fn multiple_params_and_inline_yaml_after_header() {
        let items =
            extract("/// @openapi<K, V> description: Pair\nstruct Pair<K, V> { k: K, v: V }");
        let (_, params, content) = blueprint(&items);
        assert_eq!(params, ["K", "V"]);
        assert_eq!(crate::common::yaml(content)["description"], "Pair");
    }
}
