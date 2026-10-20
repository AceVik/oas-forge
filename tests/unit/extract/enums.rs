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
    fn empty_enums_are_only_emitted_with_overrides() {
        assert!(extract("/// @openapi\nenum Never {}").is_empty());
        assert!(extract("/// @openapi\nenum Hidden { #[serde(skip)] A }").is_empty());
        let items = extract("/// @openapi\n/// format: custom\nenum Never {}");
        assert_eq!(
            schema(&items, "Never"),
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
    fn generic_enums_are_blueprints() {
        let items = extract("/// @openapi<T\nenum Choice<T> { A, B(T) }");
        assert!(schema_names(&items).is_empty());
        assert_eq!(crate::extract::blueprint(&items, "Choice").params, ["T"]);
        let items =
            extract("/// @openapi\n#[serde(into = \"String\")]\nenum Converted<T> { A(T) }");
        assert_eq!(
            crate::extract::blueprint(&items, "Converted").schema,
            json!({ "type": "string" })
        );
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
                    "secret": { "type": ["string", "null"] }
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

mod external_data_variants {
    use super::*;

    #[test]
    fn data_variants_are_single_key_objects() {
        // Regression: data variants of externally tagged enums were dropped.
        let items = extract(
            r#"
            /// @openapi
            #[serde(rename_all = "snake_case")]
            enum Shape {
                /// A circle.
                Circle { radius: f64 },
                Square(f64),
                Pair(u8, u8),
                Point,
                Origin,
            }
            "#,
        );
        assert_eq!(
            schema(&items, "Shape"),
            json!({ "oneOf": [
                { "type": "string", "enum": ["point", "origin"] },
                {
                    "type": "object",
                    "properties": { "circle": {
                        "type": "object",
                        "properties": { "radius": { "type": "number", "format": "double" } },
                        "required": ["radius"],
                    } },
                    "required": ["circle"],
                    "additionalProperties": false,
                    "description": "A circle.",
                },
                {
                    "type": "object",
                    "properties": { "square": { "type": "number", "format": "double" } },
                    "required": ["square"],
                    "additionalProperties": false,
                },
                {
                    "type": "object",
                    "properties": { "pair": {
                        "type": "array",
                        "prefixItems": [
                            { "type": "integer", "format": "int32", "minimum": 0, "maximum": 255 },
                            { "type": "integer", "format": "int32", "minimum": 0, "maximum": 255 },
                        ],
                        "items": false, "minItems": 2, "maxItems": 2,
                    } },
                    "required": ["pair"],
                    "additionalProperties": false,
                },
            ] })
        );
    }

    #[test]
    fn data_only_enums_have_no_string_alternative() {
        let items = extract("/// @openapi\nenum Event { Join(u32), Leave { id: u8 } }");
        assert_eq!(
            schema(&items, "Event")["oneOf"].as_array().unwrap().len(),
            2
        );
    }

    #[test]
    fn variant_attributes() {
        let items = extract(
            r#"
            /// @openapi
            enum E {
                #[serde(skip)]
                Hidden,
                #[serde(skip_serializing)]
                InputOnly,
                #[serde(other)]
                Unknown,
                #[serde(rename = "renamed")]
                Original(u8),
                #[deprecated]
                Old { a: u8 },
            }
            "#,
        );
        let alternatives = schema(&items, "E")["oneOf"].clone();
        assert_eq!(
            alternatives[0],
            json!({ "type": "string", "enum": ["InputOnly", "Unknown"] })
        );
        assert!(alternatives[1]["properties"].get("renamed").is_some());
        assert_eq!(alternatives[2]["deprecated"], true);
    }

    #[test]
    fn untagged_variants_join_as_alternatives() {
        let items = extract(
            r#"
            /// @openapi
            enum Value {
                Named { name: String },
                #[serde(untagged)]
                Raw(String),
            }
            "#,
        );
        assert_eq!(
            schema(&items, "Value")["anyOf"][1],
            json!({ "type": "string" })
        );
    }
}

mod untagged {
    use super::*;

    #[test]
    fn variants_are_alternatives_of_their_content() {
        // Regression: untagged enums produced an empty string enum.
        let items = extract(
            r#"
            /// Either form.
            /// @openapi
            #[serde(untagged)]
            enum IdOrName {
                /// Numeric id.
                Id(u64),
                Name(String),
                Pair(u8, String),
                Full { first: String, last: Option<String> },
                Nothing,
            }
            "#,
        );
        let s = schema(&items, "IdOrName");
        assert_eq!(s["description"], "Either form.");
        let alternatives = s["anyOf"].as_array().unwrap();
        assert_eq!(
            alternatives[0],
            json!({ "type": "integer", "format": "int64", "minimum": 0, "description": "Numeric id." })
        );
        assert_eq!(alternatives[1], json!({ "type": "string" }));
        assert_eq!(alternatives[2]["prefixItems"].as_array().unwrap().len(), 2);
        assert_eq!(alternatives[3]["required"], json!(["first"]));
        assert_eq!(alternatives[4], json!({ "type": "null" }));
    }
}

mod tagged_newtypes {
    use super::*;
    use crate::common::{codes, extract_with_diagnostics};
    use oas_forge::diagnostics::Code;

    #[test]
    fn internally_tagged_newtypes_of_structs_extend_the_tag_object() {
        // Regression: newtype variants lost the fields of their inner struct.
        let items = extract(
            r#"
            /// @openapi
            #[serde(tag = "type")]
            enum Storage { Custom(CustomStorage), Map(HashMap<String, String>) }
            "#,
        );
        assert_eq!(
            schema(&items, "StorageCustom")["allOf"],
            json!([{ "$ref": "$CustomStorage" }])
        );
        assert_eq!(schema(&items, "StorageMap")["allOf"][0]["type"], "object");
    }

    #[test]
    fn internally_tagged_primitives_and_tuples_are_reported() {
        let (items, diagnostics) = extract_with_diagnostics(
            "/// @openapi\n#[serde(tag = \"t\")]\nenum E {\n    A(u8),\n    B(u8, u8),\n}",
        );
        assert_eq!(schema(&items, "EA")["required"], json!(["t"]));
        assert_eq!(
            codes(&diagnostics),
            [Code::UnsupportedVariant, Code::UnsupportedVariant]
        );
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
    fn adjacently_tagged_tuples_are_arrays() {
        let items = extract(
            "/// @openapi\n#[serde(tag = \"t\", content = \"c\")]\nenum E { Pair(u8, String), Wrapped(Option<u8>) }",
        );
        assert_eq!(schema(&items, "EPair")["properties"]["c"]["minItems"], 2);
        assert_eq!(
            schema(&items, "EWrapped")["properties"]["c"],
            json!({ "type": ["integer", "null"], "format": "int32", "minimum": 0, "maximum": 255 })
        );
    }

    #[test]
    fn untagged_variants_drop_the_discriminator() {
        let items = extract(
            r#"
            /// @openapi
            #[serde(tag = "kind")]
            enum E {
                A { x: u8 },
                #[serde(untagged)]
                Other(String),
            }
            "#,
        );
        let s = schema(&items, "E");
        assert!(s.get("discriminator").is_none() && s.get("type").is_none());
        assert_eq!(
            s["anyOf"],
            json!([{ "$ref": "#/components/schemas/EA" }, { "type": "string" }])
        );
    }

    #[test]
    fn struct_variants_keep_flatten_and_closed_objects() {
        let items = extract(
            r#"
            /// @openapi
            #[serde(tag = "kind")]
            enum E {
                #[serde(deny_unknown_fields)]
                A { x: u8, #[serde(flatten)] base: Base },
            }
            "#,
        );
        // The closing keyword stays next to the merged properties: inside
        // `allOf` it would see none of them and reject every property.
        let ea = schema(&items, "EA");
        assert_eq!(ea["allOf"], json!([{ "allOf": [{ "$ref": "$Base" }] }]));
        assert_eq!(ea["unevaluatedProperties"], json!(false));
    }

    #[test]
    fn conversions_replace_the_enum() {
        let items =
            extract("/// @openapi\n#[serde(into = \"String\")]\nenum Level { Low, High(u8) }");
        assert_eq!(schema(&items, "Level"), json!({ "type": "string" }));
    }
}

mod directives {
    use super::*;

    #[test]
    fn skip_leaves_out_enums_and_variants() {
        let items = extract(
            "/// @openapi skip\nenum Hidden { A }\n/// @openapi\nenum E {\n  A,\n  /// @openapi skip\n  B,\n  /// @example { x: 1 }\n  C { x: u8 },\n}",
        );
        assert_eq!(schema_names(&items), ["E"]);
        let e = schema(&items, "E");
        let text = e.to_string();
        assert!(!text.contains("\"B\""), "{text}");
        assert!(text.contains("\"A\""), "{text}");
        assert!(text.contains("\"examples\":[{\"x\":1}]"), "{text}");
    }

    #[test]
    fn with_modules_on_newtype_variants_change_the_type() {
        let items = extract(
            "/// @openapi\nenum Stamp {\n  #[serde(with = \"chrono::serde::ts_seconds\")]\n  At(DateTime<Utc>),\n}",
        );
        assert_eq!(
            schema(&items, "Stamp")["oneOf"][0]["properties"]["At"],
            json!({ "type": "integer", "format": "int64" })
        );
    }

    #[test]
    fn deprecated_enums_are_marked() {
        let items = extract("/// @openapi\n#[deprecated]\nenum E { A }");
        assert_eq!(schema(&items, "E")["deprecated"], true);
    }

    #[test]
    fn variant_overrides_and_examples_describe_the_serialized_variant() {
        let items = extract(
            "/// @openapi\n#[serde(tag = \"kind\")]\nenum E {\n  /// @example { kind: A, x: 1 }\n  /// @openapi\n  /// x-internal: true\n  A { x: u8 },\n}",
        );
        let a = schema(&items, "EA");
        assert_eq!(a["examples"], json!([{ "kind": "A", "x": 1 }]));
        assert_eq!(a["x-internal"], true);
    }

    #[test]
    fn invalid_variant_overrides_are_reported() {
        let (items, diagnostics) = extract_with_diagnostics(
            "/// @openapi\n#[serde(untagged)]\nenum E {\n  /// @openapi\n  /// [broken\n  A(String),\n}",
        );
        assert_eq!(codes(&diagnostics), [Code::InvalidOverride]);
        assert!(
            diagnostics
                .iter()
                .next()
                .unwrap()
                .message()
                .contains("variant `A`")
        );
        assert_eq!(
            schema(&items, "E"),
            json!({ "anyOf": [{ "type": "string" }] })
        );
    }
}

mod generic_tagged {
    use super::*;
    use crate::common::pipeline;

    const EVENT: &str = r#"
        /// @openapi
        #[serde(tag = "type")]
        pub enum Event<T> { Created { item: T }, Moved { items: Vec<Wrapper<T>> }, Deleted { id: u64 } }
        "#;

    #[test]
    fn variants_that_use_a_type_parameter_are_blueprints() {
        let items = extract(EVENT);
        assert_eq!(
            crate::extract::kinds(&items),
            [
                "blueprint:EventCreated",
                "blueprint:EventMoved",
                "schema:EventDeleted",
                "blueprint:Event"
            ]
        );
        let created = crate::extract::blueprint(&items, "EventCreated");
        assert_eq!(created.params, ["T"]);
        assert_eq!(
            created.schema["properties"]["item"],
            json!({ "$ref": "$T" })
        );
        let event = crate::extract::blueprint(&items, "Event");
        assert_eq!(
            event.schema["oneOf"],
            json!([
                { "$ref": "$EventCreated<$T>" },
                { "$ref": "$EventMoved<$T>" },
                { "$ref": "#/components/schemas/EventDeleted" },
            ])
        );
        assert_eq!(
            event.schema["discriminator"]["mapping"]["Created"],
            "$EventCreated<$T>"
        );
    }

    #[test]
    fn each_instance_has_its_own_variants() {
        let (document, diagnostics) = pipeline(&[(
            "lib.rs",
            &format!(
                "{EVENT}
                /// @openapi
                pub struct Wrapper<T> {{ pub w: T }}
                /// @openapi
                pub struct User {{ pub id: u64 }}
                /// @openapi
                pub struct Holder {{ pub a: Event<User>, pub b: Event<String> }}"
            ),
        )]);
        assert!(diagnostics.is_empty(), "{diagnostics:?}");
        let schemas = &document["components"]["schemas"];
        assert_eq!(
            schemas["Event_User"]["discriminator"]["mapping"],
            json!({
                "Created": "#/components/schemas/EventCreated_User",
                "Moved": "#/components/schemas/EventMoved_User",
                "Deleted": "#/components/schemas/EventDeleted",
            })
        );
        assert_eq!(
            schemas["EventCreated_User"]["properties"]["item"],
            json!({ "$ref": "#/components/schemas/User" })
        );
        assert_eq!(
            schemas["EventCreated_String"]["properties"]["item"],
            json!({ "type": "string" })
        );
        // `Vec<Wrapper<T>>`: the parameter is substituted in nested arguments.
        assert_eq!(
            schemas["EventMoved_User"]["properties"]["items"]["items"],
            json!({ "$ref": "#/components/schemas/Wrapper_User" })
        );
        assert!(schemas.get("EventCreated").is_none());
    }
}

mod closed_variants {
    use super::*;

    #[test]
    fn flattened_maps_of_struct_variants_stay_next_to_the_properties() {
        // Inside `allOf`, `additionalProperties` would apply to the tag and
        // every field.
        let items = extract(
            "/// @openapi\n#[serde(tag = \"kind\")]\nenum E { A { a: bool, #[serde(flatten)] extra: HashMap<String, String> } }",
        );
        let a = schema(&items, "EA");
        assert_eq!(a["additionalProperties"], json!({ "type": "string" }));
        assert!(a.get("allOf").is_none());
    }

    #[test]
    fn the_enums_deny_unknown_fields_closes_struct_variants() {
        let items = extract(
            r#"
            /// @openapi
            #[serde(tag = "t", deny_unknown_fields)]
            enum D { A { a: bool } }
            /// @openapi
            #[serde(deny_unknown_fields)]
            enum X { B { b: bool } }
            "#,
        );
        assert_eq!(schema(&items, "DA")["additionalProperties"], false);
        assert_eq!(
            schema(&items, "X")["oneOf"][0]["properties"]["B"]["additionalProperties"],
            false
        );
    }
}

mod unit_variant_metadata {
    use super::*;

    #[test]
    fn annotated_unit_variants_are_const_alternatives() {
        let items = extract(
            r#"
            /// @openapi
            pub enum Color {
                /// The red one.
                /// @openapi
                /// x-rgb: ff0000
                #[deprecated]
                Red,
                /// @example green
                Green,
                Blue,
            }
            "#,
        );
        // The `enum` stays for tools that ignore `oneOf`.
        assert_eq!(
            schema(&items, "Color"),
            json!({
                "type": "string",
                "enum": ["Red", "Green", "Blue"],
                "oneOf": [
                    { "const": "Red", "description": "The red one.", "deprecated": true, "x-rgb": "ff0000" },
                    { "const": "Green", "examples": ["green"] },
                    { "const": "Blue" },
                ],
            })
        );
    }

    #[test]
    fn annotated_unit_variants_next_to_data_variants() {
        let items =
            extract("/// @openapi\npub enum Shape {\n  #[deprecated]\n  Empty,\n  Circle(f64),\n}");
        assert_eq!(
            schema(&items, "Shape")["oneOf"][0],
            json!({
                "type": "string",
                "enum": ["Empty"],
                "oneOf": [{ "const": "Empty", "deprecated": true }],
            })
        );
    }

    #[test]
    fn documented_unit_variants_stay_a_string_enum() {
        let items = extract("/// @openapi\npub enum State {\n  /// Open.\n  Open,\n  Closed,\n}");
        assert_eq!(
            schema(&items, "State"),
            json!({ "type": "string", "enum": ["Open", "Closed"] })
        );
    }
}

mod serde_repr {
    use super::*;

    #[test]
    fn repr_enums_are_their_discriminants() {
        let items = extract(
            r#"
            /// Status codes.
            /// @openapi
            #[derive(Serialize_repr, Deserialize_repr)]
            #[repr(i16)]
            pub enum Status { A = 1, B, C = -3, D, E = 0x10 }
            /// @openapi
            #[cfg_attr(feature = "serde", derive(serde_repr::Serialize_repr))]
            #[repr(u8)]
            pub enum Implicit { First, Second }
            "#,
        );
        assert_eq!(
            schema(&items, "Status"),
            json!({ "type": "integer", "enum": [1, 2, -3, -2, 16], "description": "Status codes." })
        );
        assert_eq!(
            schema(&items, "Implicit"),
            json!({ "type": "integer", "enum": [0, 1] })
        );
    }

    #[test]
    fn non_literal_discriminants_are_reported() {
        let (items, diagnostics) = extract_with_diagnostics(
            "/// @openapi\n#[derive(Serialize_repr)]\n#[repr(u8)]\npub enum S {\n  A = BASE,\n  B,\n}",
        );
        assert_eq!(schema(&items, "S"), json!({ "type": "integer" }));
        assert_eq!(codes(&diagnostics), [Code::UnknownDiscriminant]);
        assert_eq!(
            diagnostics
                .iter()
                .next()
                .unwrap()
                .location()
                .unwrap()
                .line(),
            5
        );
    }
}
