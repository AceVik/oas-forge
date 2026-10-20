//! Enums: string enums, internally and adjacently tagged enums.

use crate::common::{codes, extract, extract_with_diagnostics, schema, schema_names};
use oas_forge::diagnostics::Code;
use serde_json::{Value, json};

fn variants(code: &str, name: &str) -> Value {
    schema(&extract(code), name)["enum"].clone()
}

mod string_enums {
    use super::*;

    #[test]
    fn enums_without_openapi_tag_are_not_exported() {
        let items = extract(
            r#"
            /// This should be ignored
            pub enum IgnoredEnum { A, B }

            /// @openapi
            pub enum IncludedEnum { C, D }
            "#,
        );
        assert_eq!(schema_names(&items), ["IncludedEnum"]);
    }

    #[test]
    fn unit_variants_become_string_enum() {
        let items = extract("/// Order state\n/// @openapi\nenum State { Open, Closed }");
        assert_eq!(
            schema(&items, "State"),
            json!({ "type": "string", "enum": ["Open", "Closed"], "description": "Order state" })
        );
    }

    #[test]
    fn non_unit_variants_are_skipped() {
        assert_eq!(
            variants("/// @openapi\nenum E { A, B(u32), C { x: i32 } }", "E"),
            json!(["A"])
        );
    }

    #[test]
    fn enum_without_unit_variants_or_overrides_is_not_emitted() {
        let items = extract(
            r#"
            /// @openapi-type Event
            enum Event { Join(u32), Message { text: String } }
            "#,
        );
        assert!(items.is_empty());
    }

    #[test]
    fn overrides_alone_emit_the_enum() {
        let items = extract(
            r#"
            /// @openapi
            /// format: custom
            enum Event { Join(u32) }
            "#,
        );
        assert_eq!(
            schema(&items, "Event"),
            json!({ "type": "string", "format": "custom" })
        );
    }

    #[test]
    fn inline_override_on_tag_line() {
        let items = extract("/// @openapi description: Inline\nenum E { A }");
        assert_eq!(schema(&items, "E")["description"], "Inline");
    }

    #[test]
    fn invalid_override_is_reported_and_ignored() {
        let (items, diagnostics) =
            extract_with_diagnostics("/// @openapi\n/// enum: [unclosed\nenum E { A }");
        assert_eq!(schema(&items, "E")["enum"], json!(["A"]));
        assert_eq!(codes(&diagnostics), [Code::InvalidOverride]);
    }

    #[test]
    fn generic_header_emits_blueprint() {
        let items = extract("/// @openapi<T> x-kind: $T\nenum Choice<T> { A }");
        let choice = crate::extract::blueprint(&items, "Choice");
        assert_eq!(choice.params, ["T"]);
        assert_eq!(choice.schema["x-kind"], "$T");
        assert_eq!(choice.schema["enum"], json!(["A"]));
    }

    #[test]
    fn unterminated_generic_header_is_a_plain_schema() {
        let items = extract("/// @openapi<T\nenum Choice<T> { A }");
        assert_eq!(schema_names(&items), ["Choice"]);
    }
}

mod renaming {
    use super::*;

    #[test]
    fn serde_rename_renames_the_schema() {
        let items =
            extract("/// @openapi\n#[serde(rename = \"RenamedEnum\")]\npub enum Status { Active }");
        assert_eq!(schema_names(&items), ["RenamedEnum"]);
    }

    #[test]
    fn serde_variant_rename() {
        assert_eq!(
            variants(
                r#"
                /// @openapi
                pub enum Color {
                    #[serde(rename = "RED_cOlOr")]
                    Red,
                    Blue,
                }
                "#,
                "Color"
            ),
            json!(["RED_cOlOr", "Blue"])
        );
    }

    #[test]
    fn serde_rename_all() {
        assert_eq!(
            variants(
                "/// @openapi\n#[serde(rename_all = \"snake_case\")]\npub enum AccessLevel { SuperAdmin, GuestUser }",
                "AccessLevel"
            ),
            json!(["super_admin", "guest_user"])
        );
        assert_eq!(
            variants(
                "/// @openapi\n#[serde(rename_all = \"camelCase\")]\npub enum Status { PendingValidation, CompletedSuccess }",
                "Status"
            ),
            json!(["pendingValidation", "completedSuccess"])
        );
    }

    #[test]
    fn explicit_variant_rename_wins_over_rename_all() {
        assert_eq!(
            variants(
                r#"
                /// @openapi-type Status
                #[serde(rename_all = "camelCase")]
                enum Status {
                    Active,
                    /// Pending approval
                    PendingReview,
                    #[serde(rename = "archived_state")]
                    Archived,
                    /// @openapi rename "gone"
                    Deleted,
                }
                "#,
                "Status"
            ),
            json!(["active", "pendingReview", "archived_state", "gone"])
        );
    }
}

mod internally_tagged {
    use super::*;

    const STORAGE: &str = r#"
        /// Configuration for a storage location.
        /// @openapi
        #[serde(tag = "type", rename_all = "snake_case")]
        pub enum StorageConfigDto {
            Local { root_path: String },
            /// Remote storage
            StarrRemote { url: String, secret: Option<String> },
            Disabled,
        }
    "#;

    #[test]
    fn container_is_a_discriminated_one_of() {
        let items = extract(STORAGE);
        assert_eq!(
            schema(&items, "StorageConfigDto"),
            json!({
                "type": "object",
                "description": "Configuration for a storage location.",
                "oneOf": [
                    { "$ref": "#/components/schemas/StorageConfigDtoLocal" },
                    { "$ref": "#/components/schemas/StorageConfigDtoStarrRemote" },
                    { "$ref": "#/components/schemas/StorageConfigDtoDisabled" }
                ],
                "discriminator": {
                    "propertyName": "type",
                    "mapping": {
                        "local": "#/components/schemas/StorageConfigDtoLocal",
                        "starr_remote": "#/components/schemas/StorageConfigDtoStarrRemote",
                        "disabled": "#/components/schemas/StorageConfigDtoDisabled"
                    }
                }
            })
        );
    }

    #[test]
    fn struct_variant_fields_are_flattened_next_to_the_tag() {
        let items = extract(STORAGE);
        assert_eq!(
            schema(&items, "StorageConfigDtoStarrRemote"),
            json!({
                "type": "object",
                "description": "Remote storage",
                "properties": {
                    "type": {
                        "type": "string",
                        "enum": ["starr_remote"],
                        "description": "Discriminator: starr_remote"
                    },
                    "url": { "type": "string" },
                    "secret": { "type": "string" }
                },
                "required": ["type", "url"]
            })
        );
    }

    #[test]
    fn unit_variant_only_has_the_tag() {
        let items = extract(STORAGE);
        let disabled = schema(&items, "StorageConfigDtoDisabled");
        assert_eq!(disabled["required"], json!(["type"]));
        assert_eq!(disabled["properties"].as_object().unwrap().len(), 1);
    }

    #[test]
    fn variant_rename_changes_tag_value_but_not_schema_name() {
        let items = extract(
            r#"
            /// @openapi
            #[serde(tag = "kind")]
            enum Shape {
                #[serde(rename = "circle")]
                Round { r: f64 },
            }
            "#,
        );
        assert_eq!(
            schema(&items, "Shape")["discriminator"]["mapping"]["circle"],
            "#/components/schemas/ShapeRound"
        );
        assert_eq!(
            schema(&items, "ShapeRound")["properties"]["kind"]["enum"],
            json!(["circle"])
        );
    }

    #[test]
    fn single_field_tuple_variant_contributes_no_properties() {
        let items = extract("/// @openapi\n#[serde(tag = \"type\")]\nenum Msg { Text(String) }");
        assert_eq!(
            schema(&items, "MsgText")["properties"]
                .as_object()
                .unwrap()
                .len(),
            1
        );
    }

    #[test]
    fn multi_field_tuple_variant_is_reduced_to_the_tag_and_reported() {
        let (items, diagnostics) = extract_with_diagnostics(
            "/// @openapi\n#[serde(tag = \"type\")]\nenum Msg {\n    Pair(String, i32),\n}",
        );
        assert_eq!(schema(&items, "MsgPair")["required"], json!(["type"]));
        assert_eq!(codes(&diagnostics), [Code::UnsupportedVariant]);
        assert_eq!(
            diagnostics
                .iter()
                .next()
                .unwrap()
                .location()
                .unwrap()
                .line(),
            4
        );
    }

    #[test]
    fn variant_field_validation_is_applied() {
        let items = extract(
            r#"
            /// @openapi
            #[serde(tag = "type")]
            pub enum ValidatedEnum {
                Variant {
                    #[validate(length(min = 5))]
                    name: String,
                }
            }
            "#,
        );
        assert_eq!(
            schema(&items, "ValidatedEnumVariant")["properties"]["name"]["minLength"],
            5
        );
    }
}

mod adjacently_tagged {
    use super::*;

    const ADJACENT: &str = r#"
        /// @openapi
        #[serde(tag = "t", content = "c")]
        pub enum Adjacent {
            Str(String),
            Struct { x: i32 },
            Empty,
        }
    "#;

    #[test]
    fn container_uses_tag_as_discriminator() {
        let items = extract(ADJACENT);
        let adjacent = schema(&items, "Adjacent");
        assert_eq!(adjacent["discriminator"]["propertyName"], "t");
        assert_eq!(adjacent["oneOf"].as_array().unwrap().len(), 3);
    }

    #[test]
    fn tuple_variant_content_holds_the_inner_schema() {
        let items = extract(ADJACENT);
        let variant = schema(&items, "AdjacentStr");
        assert_eq!(variant["properties"]["t"]["enum"], json!(["Str"]));
        assert_eq!(variant["properties"]["c"], json!({ "type": "string" }));
        assert_eq!(variant["required"], json!(["t", "c"]));
    }

    #[test]
    fn struct_variant_content_is_an_object() {
        let items = extract(ADJACENT);
        assert_eq!(
            schema(&items, "AdjacentStruct")["properties"]["c"],
            json!({
                "type": "object",
                "properties": { "x": { "type": "integer", "format": "int32" } },
                "required": ["x"]
            })
        );
    }

    #[test]
    fn unit_variant_has_no_content() {
        let items = extract(ADJACENT);
        let empty = schema(&items, "AdjacentEmpty");
        assert!(empty["properties"].get("c").is_none());
        assert_eq!(empty["required"], json!(["t"]));
    }

    #[test]
    fn tuple_field_validation_is_applied_to_content() {
        let items = extract(
            r#"
            /// @openapi
            #[serde(tag = "type", content = "content")]
            pub enum ValidatedTupleEnum {
                Tuple(#[validate(length(min = 10))] String)
            }
            "#,
        );
        assert_eq!(
            schema(&items, "ValidatedTupleEnumTuple")["properties"]["content"]["minLength"],
            10
        );
    }
}

/// Field naming in tagged enums must match what serde serializes.
mod tagged_field_renaming {
    use super::*;

    fn field_names(code: &str, variant_schema: &str) -> Vec<String> {
        let items = extract(code);
        let variant = schema(&items, variant_schema);
        let fields = match variant["properties"].get("c") {
            Some(content) => &content["properties"],
            None => &variant["properties"],
        };
        let mut names: Vec<String> = fields.as_object().unwrap().keys().cloned().collect();
        names.sort();
        names
    }

    #[test]
    fn rename_all_renames_variants_but_not_fields() {
        // Regression: `rename_all` on the enum also renamed the variant fields,
        // although serde applies it to variant names only.
        let code = r#"
            /// @openapi
            #[serde(tag = "type", rename_all = "camelCase")]
            enum Event { UserCreated { user_id: i32 } }
        "#;
        assert_eq!(field_names(code, "EventUserCreated"), ["type", "user_id"]);
        assert_eq!(
            schema(&extract(code), "EventUserCreated")["properties"]["type"]["enum"],
            json!(["userCreated"])
        );
    }

    #[test]
    fn rename_all_fields_renames_fields_of_every_variant() {
        let code = r#"
            /// @openapi
            #[serde(tag = "type", rename_all_fields = "camelCase")]
            enum Event {
                UserCreated { user_id: i32 },
                UserDeleted { deleted_at: String },
            }
        "#;
        assert_eq!(field_names(code, "EventUserCreated"), ["type", "userId"]);
        assert_eq!(field_names(code, "EventUserDeleted"), ["deletedAt", "type"]);
        assert_eq!(
            schema(&extract(code), "EventUserCreated")["properties"]["type"]["enum"],
            json!(["UserCreated"]),
            "variant names are untouched by rename_all_fields"
        );
    }

    #[test]
    fn variant_rename_all_wins_over_rename_all_fields() {
        let code = r#"
            /// @openapi
            #[serde(tag = "type", rename_all_fields = "camelCase")]
            enum Event {
                #[serde(rename_all = "SCREAMING_SNAKE_CASE")]
                UserCreated { user_id: i32 },
                /// @openapi rename-all kebab-case
                UserDeleted { deleted_at: String },
            }
        "#;
        assert_eq!(field_names(code, "EventUserCreated"), ["USER_ID", "type"]);
        assert_eq!(
            field_names(code, "EventUserDeleted"),
            ["deleted-at", "type"]
        );
    }

    #[test]
    fn explicit_field_rename_wins() {
        let code = r#"
            /// @openapi
            #[serde(tag = "type", rename_all_fields = "camelCase")]
            enum Event {
                UserCreated {
                    #[serde(rename = "id")]
                    user_id: i32,
                    created_at: String,
                },
            }
        "#;
        assert_eq!(
            field_names(code, "EventUserCreated"),
            ["createdAt", "id", "type"]
        );
    }

    #[test]
    fn adjacently_tagged_content_follows_the_same_rules() {
        let code = r#"
            /// @openapi
            #[serde(tag = "t", content = "c", rename_all = "snake_case", rename_all_fields = "camelCase")]
            enum Event { UserCreated { user_id: i32 } }
        "#;
        assert_eq!(field_names(code, "EventUserCreated"), ["userId"]);
        assert_eq!(
            schema(&extract(code), "EventUserCreated")["properties"]["t"]["enum"],
            json!(["user_created"])
        );
    }
}
