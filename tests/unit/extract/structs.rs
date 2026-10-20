//! Structs: fields, documentation, overrides, renaming, validation and blueprints.

use crate::common::{codes, extract, extract_with_diagnostics, schema, schema_names};
use oas_forge::diagnostics::Code;
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
                "meta": { "type": ["string", "null"] }
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
fn unit_struct_is_null_like_serde_writes_it() {
    let items = extract("/// @openapi\nstruct Marker;\n/// @openapi\nstruct Empty {}");
    assert_eq!(schema(&items, "Marker"), json!({ "type": "null" }));
    assert_eq!(schema(&items, "Empty"), json!({ "type": "object" }));
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
    let props = &crate::extract::blueprint(&items, "Container").schema["properties"];
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
    for field in ["created_at", "raw"] {
        assert_eq!(
            props[field],
            json!({ "type": "string", "format": "date-time" }),
            "{field}"
        );
    }
    assert_eq!(
        props["modified_at"],
        json!({ "type": ["string", "null"], "format": "date-time-local" })
    );
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

    #[test]
    fn javadoc_block_comments_are_read() {
        let items = extract(
            "/**\n * A user.\n *\n * Second paragraph.\n * @openapi\n */\nstruct J { id: bool }\n/** Starts on the first line.\n * @openapi */\nstruct K { id: bool }",
        );
        assert_eq!(
            schema(&items, "J")["description"],
            "A user.\n\nSecond paragraph."
        );
        assert_eq!(
            schema(&items, "K")["description"],
            "Starts on the first line."
        );
    }

    #[test]
    fn markdown_keeps_its_line_breaks() {
        let items = extract(
            r#"
            /// A user.
            /// Still the first paragraph.
            ///
            /// # Examples
            /// Some text
            /// continued.
            ///
            /// - one
            /// - two
            ///   continued
            ///   - nested
            /// 1. first
            ///
            /// ```
            /// let x = 1;
            ///
            ///     indented();
            /// ```
            /// After the code.
            /// @openapi
            struct U { id: bool }
            "#,
        );
        assert_eq!(
            schema(&items, "U")["description"],
            "A user. Still the first paragraph.\n\n# Examples\nSome text continued.\n\n\
             - one\n- two\n  continued\n  - nested\n1. first\n\n\
             ```\nlet x = 1;\n\n    indented();\n```\nAfter the code."
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
    fn invalid_field_override_is_reported_and_ignored() {
        let (items, diagnostics) = extract_with_diagnostics(
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
        assert_eq!(codes(&diagnostics), [Code::InvalidOverride]);
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

    #[test]
    fn invalid_struct_override_is_reported_and_ignored() {
        let (items, diagnostics) =
            extract_with_diagnostics("/// @openapi\n/// type: [unclosed\nstruct S { a: i32 }");
        assert_eq!(schema(&items, "S")["type"], "object");
        assert_eq!(codes(&diagnostics), [Code::InvalidOverride]);
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
    use crate::extract::blueprint;

    #[test]
    fn generic_header_declares_a_blueprint() {
        let items = extract(
            r#"
            /// @openapi<T>
            /// properties:
            ///   total: { type: integer }
            struct Page<T> { items: Vec<T> }
            "#,
        );
        assert_eq!(schema_names(&items), Vec::<String>::new());
        let page = blueprint(&items, "Page");
        assert_eq!(page.rust_name, "Page");
        assert_eq!(page.params, ["T"]);
        assert_eq!(
            page.schema["properties"],
            json!({
                "items": { "type": "array", "items": { "$ref": "$T" } },
                "total": { "type": "integer" }
            }),
            "reflected fields and overrides are merged"
        );
    }

    #[test]
    fn generic_structs_are_blueprints_without_a_header() {
        // `Page<T>` is instantiated where it is used, so `@openapi<T>` is optional.
        for code in [
            "/// @openapi<T\nstruct Page<T> { items: Vec<T> }",
            "/// @openapi\nstruct Page<T> { items: Vec<T> }",
        ] {
            let items = extract(code);
            assert!(schema_names(&items).is_empty(), "{code}");
            assert_eq!(blueprint(&items, "Page").params, ["T"]);
        }
        let items = extract("/// @openapi\nstruct Borrowed<'a, const N: usize> { s: &'a str }");
        assert_eq!(
            schema_names(&items),
            ["Borrowed"],
            "lifetimes and constants are not type parameters"
        );
    }

    #[test]
    fn multiple_params_and_inline_yaml_after_header() {
        let items =
            extract("/// @openapi<K, V> description: Pair\nstruct Pair<K, V> { k: K, v: V }");
        let pair = blueprint(&items, "Pair");
        assert_eq!(pair.params, ["K", "V"]);
        assert_eq!(pair.schema["description"], "Pair");
    }

    #[test]
    fn renamed_blueprints_keep_their_rust_name() {
        let items = extract(
            "/// @openapi<T>\n/// @openapi rename \"Paged\"\nstruct Page<T> { items: Vec<T> }",
        );
        assert_eq!(blueprint(&items, "Paged").rust_name, "Page");
    }
}

mod root_documents {
    use super::*;
    use crate::extract::{document_items, kinds};

    #[test]
    fn unit_struct_with_document_keys_is_a_document() {
        // Regression: the root on a unit struct became a schema called like the struct.
        let items = extract(
            r#"
            /// The API.
            /// @openapi
            /// openapi: 3.1.0
            /// info: { title: T, version: "1" }
            struct ApiDoc;
            "#,
        );
        assert_eq!(kinds(&items), ["document"]);
        let document = document_items(&items)[0];
        assert_eq!(
            document.text,
            " openapi: 3.1.0\n info: { title: T, version: \"1\" }"
        );
        assert_eq!(document.location.line(), 4);
    }

    #[test]
    fn every_document_key_counts() {
        for key in [
            "openapi",
            "info",
            "paths",
            "webhooks",
            "servers",
            "components",
        ] {
            let items = extract(&format!("/// @openapi\n/// {key}: {{}}\nstruct Doc;"));
            assert_eq!(kinds(&items), ["document"], "{key}");
        }
    }

    #[test]
    fn other_keys_and_invalid_yaml_stay_schemas() {
        let items = extract("/// @openapi\n/// x-root: true\nstruct A;");
        assert_eq!(
            schema(&items, "A"),
            json!({ "type": "object", "x-root": true })
        );

        let (items, diagnostics) =
            extract_with_diagnostics("/// @openapi\n/// info: [unclosed\nstruct B;");
        assert_eq!(schema(&items, "B"), json!({ "type": "object" }));
        assert_eq!(codes(&diagnostics), [Code::InvalidOverride]);
    }

    #[test]
    fn structs_with_fields_are_always_schemas() {
        let items = extract("/// @openapi\n/// info: { x: 1 }\nstruct WithFields { a: String }");
        assert_eq!(kinds(&items), ["schema:WithFields"]);
    }
}

mod serde_fields {
    use super::*;
    use serde_json::Value;

    fn properties(code: &str) -> Value {
        schema(&extract(code), "S")["properties"].clone()
    }

    #[test]
    fn skipped_fields_are_left_out() {
        let properties = properties(
            r#"
            /// @openapi
            struct S {
                #[serde(skip)]
                a: String,
                #[serde(skip_serializing, skip_deserializing)]
                b: String,
                c: String,
            }
            "#,
        );
        assert_eq!(properties, json!({ "c": { "type": "string" } }));
    }

    #[test]
    fn one_way_fields_are_read_or_write_only() {
        let items = extract(
            r#"
            /// @openapi
            struct S {
                #[serde(skip_serializing)]
                password: String,
                #[serde(skip_deserializing)]
                created_at: String,
            }
            "#,
        );
        let s = schema(&items, "S");
        assert_eq!(
            s["properties"]["password"],
            json!({ "type": "string", "writeOnly": true })
        );
        assert_eq!(
            s["properties"]["created_at"],
            json!({ "type": "string", "readOnly": true })
        );
        assert_eq!(
            s["required"],
            json!(["password", "created_at"]),
            "required per direction"
        );
    }

    #[test]
    fn defaults_and_conditional_fields_are_optional() {
        let items = extract(
            r#"
            /// @openapi
            struct S {
                #[serde(default)]
                a: u8,
                #[serde(default = "five")]
                b: u8,
                #[serde(skip_serializing_if = "Vec::is_empty")]
                c: Vec<u8>,
                d: u8,
            }
            /// @openapi
            #[serde(default)]
            struct All { a: u8, b: String }
            "#,
        );
        assert_eq!(schema(&items, "S")["required"], json!(["d"]));
        assert!(schema(&items, "All").get("required").is_none());
    }

    #[test]
    fn flattened_structs_become_all_of() {
        let items = extract(
            r#"
            /// @openapi
            struct S {
                id: u64,
                #[serde(flatten)]
                meta: Meta,
                #[serde(flatten)]
                extra: Option<Extra>,
            }
            "#,
        );
        assert_eq!(
            schema(&items, "S"),
            json!({
                "type": "object",
                "properties": { "id": { "type": "integer", "format": "int64", "minimum": 0 } },
                "required": ["id"],
                "allOf": [{ "$ref": "$Meta" }, { "anyOf": [{ "$ref": "$Extra" }, {}] }],
            })
        );
    }

    #[test]
    fn flattened_maps_are_additional_properties() {
        let items = extract(
            "/// @openapi\nstruct S {\n  id: u64,\n  #[serde(flatten)]\n  rest: HashMap<String, Value>,\n}",
        );
        let s = schema(&items, "S");
        assert_eq!(s["additionalProperties"], json!({}));
        assert!(s.get("allOf").is_none());
    }

    #[test]
    fn flattened_maps_next_to_flattened_structs_are_unevaluated_properties() {
        // `additionalProperties` would also apply to the fields of `Inner`.
        let items = extract(
            "/// @openapi\nstruct S {\n  a: bool,\n  #[serde(flatten)]\n  inner: Inner,\n  #[serde(flatten)]\n  rest: HashMap<String, String>,\n}",
        );
        let s = schema(&items, "S");
        assert_eq!(s["allOf"], json!([{ "$ref": "$Inner" }]));
        assert_eq!(s["unevaluatedProperties"], json!({ "type": "string" }));
        assert!(s.get("additionalProperties").is_none());
    }

    #[test]
    fn deny_unknown_fields_closes_the_object() {
        let items = extract(
            r#"
            /// @openapi
            #[serde(deny_unknown_fields)]
            struct Closed { a: u8 }
            /// @openapi
            #[serde(deny_unknown_fields)]
            struct WithBase { a: u8, #[serde(flatten)] base: Base }
            "#,
        );
        assert_eq!(schema(&items, "Closed")["additionalProperties"], false);
        let with_base = schema(&items, "WithBase");
        assert_eq!(with_base["unevaluatedProperties"], false);
        assert!(with_base.get("additionalProperties").is_none());
    }

    #[test]
    fn raw_identifiers_lose_their_prefix() {
        let items = extract(
            "/// @openapi\n#[serde(rename_all = \"UPPERCASE\")]\nstruct r#S { r#type: String }",
        );
        assert!(schema(&items, "S")["properties"].get("TYPE").is_some());
    }

    #[test]
    fn known_with_modules_change_the_type() {
        let properties = properties(
            r#"
            /// @openapi
            struct S {
                #[serde(with = "chrono::serde::ts_seconds")]
                a: DateTime<Utc>,
                #[serde(with = "chrono::serde::ts_milliseconds_option")]
                b: Option<DateTime<Utc>>,
                #[serde(with = "time::serde::rfc3339")]
                c: OffsetDateTime,
                #[serde(serialize_with = "time::serde::timestamp::serialize")]
                d: OffsetDateTime,
                #[serde(with = "humantime_serde")]
                e: Duration,
                #[serde(with = "hex")]
                f: Vec<u8>,
                #[serde(with = "base64_serde")]
                g: Vec<u8>,
                #[serde(with = "my::custom")]
                h: u8,
            }
            "#,
        );
        assert_eq!(
            properties["a"],
            json!({ "type": "integer", "format": "int64" })
        );
        assert_eq!(
            properties["b"],
            json!({ "type": ["integer", "null"], "format": "int64" })
        );
        assert_eq!(
            properties["c"],
            json!({ "type": "string", "format": "date-time" })
        );
        assert_eq!(
            properties["d"],
            json!({ "type": "integer", "format": "int64" })
        );
        assert_eq!(properties["e"], json!({ "type": "string" }));
        assert_eq!(properties["f"], json!({ "type": "string" }));
        assert_eq!(
            properties["g"],
            json!({ "type": "string", "format": "byte" })
        );
        assert_eq!(
            properties["h"]["format"], "int32",
            "unknown modules keep the type"
        );
    }

    #[test]
    fn deprecated_items_and_fields_are_marked() {
        let items = extract(
            "/// @openapi\n#[deprecated]\nstruct S {\n  #[deprecated(note = \"x\")]\n  a: u8,\n  b: u8,\n}",
        );
        let s = schema(&items, "S");
        assert_eq!(s["deprecated"], true);
        assert_eq!(s["properties"]["a"]["deprecated"], true);
        assert!(s["properties"]["b"].get("deprecated").is_none());
    }
}

mod struct_shapes {
    use super::*;

    #[test]
    fn newtype_structs_are_their_field() {
        let items = extract(
            "/// A user id.\n/// @openapi\nstruct UserId(pub u64);\n/// @openapi\nstruct MaybeName(#[validate(length(min = 1))] Option<String>);",
        );
        assert_eq!(
            schema(&items, "UserId"),
            json!({ "type": "integer", "format": "int64", "minimum": 0, "description": "A user id." })
        );
        assert_eq!(
            schema(&items, "MaybeName"),
            json!({ "type": ["string", "null"], "minLength": 1 })
        );
    }

    #[test]
    fn tuple_structs_are_fixed_arrays() {
        let items = extract("/// @openapi\nstruct Pair(pub String, #[serde(skip)] u8, pub i32);");
        assert_eq!(
            schema(&items, "Pair"),
            json!({
                "type": "array",
                "prefixItems": [{ "type": "string" }, { "type": "integer", "format": "int32" }],
                "items": false, "minItems": 2, "maxItems": 2,
            })
        );
    }

    #[test]
    fn transparent_structs_are_their_field() {
        let items = extract(
            r#"
            /// @openapi
            #[serde(transparent)]
            struct Email {
                #[validate(email)]
                inner: String,
            }
            /// @openapi
            #[serde(transparent)]
            struct Tagged { #[serde(skip)] marker: u8, value: Vec<u8> }
            /// @openapi
            #[serde(transparent)]
            struct Nothing { #[serde(skip)] a: u8 }
            "#,
        );
        assert_eq!(
            schema(&items, "Email"),
            json!({ "type": "string", "format": "email" })
        );
        assert_eq!(schema(&items, "Tagged")["type"], "array");
        assert_eq!(schema(&items, "Nothing"), json!({}));
    }

    #[test]
    fn into_and_from_use_the_other_type() {
        let items = extract(
            r#"
            /// @openapi
            #[serde(into = "String", from = "u8")]
            struct Code { raw: u8 }
            /// @openapi
            #[serde(try_from = "Vec<u8>")]
            struct Bytes2 { a: u8 }
            /// @openapi
            #[serde(into = "not a type")]
            struct Weird { a: u8 }
            "#,
        );
        assert_eq!(schema(&items, "Code"), json!({ "type": "string" }));
        assert_eq!(schema(&items, "Bytes2")["type"], "array");
        assert_eq!(schema(&items, "Weird")["type"], "object");
    }

    #[test]
    fn tuple_structs_with_skipped_members_stay_arrays() {
        // serde writes `[1]` for `T(1, 2)`: only a one-field struct (or a
        // transparent one) is a newtype.
        let items = extract(
            "/// @openapi\nstruct T(pub u8, #[serde(skip)] pub u8);\n/// @openapi\n#[serde(transparent)]\nstruct W(bool, #[serde(skip)] PhantomData<u8>);",
        );
        assert_eq!(
            schema(&items, "T"),
            json!({
                "type": "array",
                "prefixItems": [{ "type": "integer", "format": "int32", "minimum": 0, "maximum": 255 }],
                "items": false, "minItems": 1, "maxItems": 1,
            })
        );
        assert_eq!(schema(&items, "W"), json!({ "type": "boolean" }));
    }

    #[test]
    fn tagged_structs_carry_their_tag() {
        let items = extract(
            r#"
            /// @openapi
            #[serde(tag = "type", deny_unknown_fields)]
            struct Tagged { a: bool }
            /// @openapi rename "Doc"
            #[serde(tag = "kind", rename = "Wire")]
            struct Empty {}
            "#,
        );
        let tagged = schema(&items, "Tagged");
        assert_eq!(
            tagged,
            json!({
                "type": "object",
                "properties": {
                    "type": { "type": "string", "enum": ["Tagged"] },
                    "a": { "type": "boolean" },
                },
                "required": ["type", "a"],
                "additionalProperties": false,
            })
        );
        let names: Vec<&String> = tagged["properties"].as_object().unwrap().keys().collect();
        assert_eq!(names, ["type", "a"]);
        // The tag is serde's name, not the documented one.
        assert_eq!(
            schema(&items, "Doc"),
            json!({
                "type": "object",
                "properties": { "kind": { "type": "string", "enum": ["Wire"] } },
                "required": ["kind"],
            })
        );
    }
}

#[test]
fn structural_overrides_replace_a_reflected_reference() {
    let items = extract(
        "/// @openapi<T>\n/// type: object\n/// properties:\n///   data: { $ref: $T }\nstruct Wrapper<T>(T);\n/// @openapi\n/// description: An id\nstruct Id(Uuid);",
    );
    assert_eq!(
        crate::extract::blueprint(&items, "Wrapper").schema,
        json!({ "type": "object", "properties": { "data": { "$ref": "$T" } } })
    );
    assert_eq!(
        schema(&items, "Id"),
        json!({ "type": "string", "format": "uuid", "description": "An id" })
    );
}

#[test]
fn structural_overrides_replace_a_nullable_reference() {
    let items = extract(
        "/// @openapi\nstruct S {\n/// The id.\n/// @openapi\n/// type: string\n/// format: uuid\nid: Option<UserId>,\n}",
    );
    assert_eq!(
        schema(&items, "S")["properties"]["id"],
        json!({ "type": ["string", "null"], "format": "uuid", "description": "The id." })
    );
}

mod directives {
    use super::*;
    use oas_forge::diagnostics::Diagnostics;
    use oas_forge::extract::{Options, extract_str};
    use oas_forge::template::Variables;
    use std::path::Path;

    #[test]
    fn skip_leaves_out_items_and_fields() {
        let items = extract(
            "/// @openapi skip\nstruct Hidden { a: u8 }\n/// @openapi\nstruct S {\n  /// @openapi skip\n  internal: u8,\n  shown: u8,\n}\n/// @openapi skip\ntype Alias = u8;",
        );
        assert_eq!(schema_names(&items), ["S"]);
        assert_eq!(
            schema(&items, "S"),
            json!({
                "type": "object",
                "properties": { "shown": { "type": "integer", "format": "int32", "minimum": 0, "maximum": 255 } },
                "required": ["shown"]
            })
        );
    }

    #[test]
    fn examples_are_yaml_values_or_plain_text() {
        let items = extract(
            "/// @openapi\n/// @example { a: 1 }\n/// @example plain: [text\nstruct S {\n  /// @example 42\n  /// @example \"quoted\"\n  a: u8,\n}",
        );
        let s = schema(&items, "S");
        assert_eq!(s["examples"], json!([{ "a": 1 }, "plain: [text"]));
        assert_eq!(s["properties"]["a"]["examples"], json!([42, "quoted"]));
    }

    #[test]
    fn explicit_null_examples_are_null() {
        let items = extract(
            "/// @openapi\nstruct S {\n  /// @example null\n  /// @example ~\n  /// @example # not yaml\n  a: Option<u8>,\n}",
        );
        assert_eq!(
            schema(&items, "S")["properties"]["a"]["examples"],
            json!([null, null, "# not yaml"])
        );
    }

    #[test]
    fn an_empty_example_is_an_empty_string() {
        let items = extract("/// @openapi\nstruct S {\n  /// @example\n  a: String,\n}");
        assert_eq!(
            schema(&items, "S")["properties"]["a"]["examples"],
            json!([""])
        );
    }

    #[test]
    fn example_needs_a_word_boundary() {
        let items = extract("/// @examples are not examples\n/// @openapi\nstruct S { a: u8 }");
        let s = schema(&items, "S");
        assert!(s.get("examples").is_none());
        assert_eq!(s["description"], "@examples are not examples");
    }

    fn extract_with(
        code: &str,
        variables: &Variables,
    ) -> (Vec<oas_forge::extract::Item>, Diagnostics) {
        let mut diagnostics = Diagnostics::new();
        let items = extract_str(
            Path::new("test.rs"),
            code,
            Options::new().variables(variables),
            &mut diagnostics,
        )
        .unwrap();
        (items, diagnostics)
    }

    #[test]
    fn variables_fill_overrides_and_examples() {
        let variables = Variables::from([("STAGE".to_string(), "1.10".to_string())]);
        let (items, diagnostics) = extract_with(
            "/// @openapi\n/// x-stage: {{STAGE}}\nstruct S {\n  /// @example {{STAGE}}\n  /// @openapi\n  /// description: Stage {{STAGE}}\n  a: u8,\n}",
            &variables,
        );
        assert!(diagnostics.is_empty(), "{diagnostics:?}");
        let s = schema(&items, "S");
        // A whole-value placeholder stays a string.
        assert_eq!(s["x-stage"], "1.10");
        assert_eq!(s["properties"]["a"]["description"], "Stage 1.10");
        assert_eq!(s["properties"]["a"]["examples"], json!(["1.10"]));
    }

    #[test]
    fn unset_environment_variables_are_reported_where_they_are_used() {
        let (items, diagnostics) = extract_with(
            "/// @openapi\nstruct S {\n  /// @openapi\n  /// description: '{{env:OAS_FORGE_TEST_UNSET_VARIABLE}}'\n  a: u8,\n}",
            &Variables::new(),
        );
        assert_eq!(codes(&diagnostics), [Code::UnknownVariable]);
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
        assert_eq!(
            schema(&items, "S")["properties"]["a"]["description"],
            "{{env:OAS_FORGE_TEST_UNSET_VARIABLE}}"
        );
    }

    #[test]
    fn comment_only_overrides_change_nothing() {
        let (items, diagnostics) =
            extract_with_diagnostics("/// @openapi\n/// # just a comment\nstruct S { a: u8 }");
        assert!(diagnostics.is_empty());
        assert_eq!(
            schema(&items, "S"),
            schema(&extract("/// @openapi\nstruct S { a: u8 }"), "S")
        );
    }

    #[test]
    fn one_way_flags_need_an_object_schema() {
        use oas_forge::schema::TypeMapper;
        let types = TypeMapper::new().with_type("Anything", json!(true));
        let mut diagnostics = Diagnostics::new();
        let items = extract_str(
            Path::new("test.rs"),
            "/// @openapi\nstruct S { #[serde(skip_serializing)] a: Anything }",
            Options::new().types(&types),
            &mut diagnostics,
        )
        .unwrap();
        assert_eq!(schema(&items, "S")["properties"]["a"], json!(true));
    }

    #[test]
    fn without_variables_placeholders_are_kept() {
        let items = extract("/// @openapi\n/// x-stage: '{{STAGE}}'\nstruct S { a: u8 }");
        assert_eq!(schema(&items, "S")["x-stage"], "{{STAGE}}");
    }
}
