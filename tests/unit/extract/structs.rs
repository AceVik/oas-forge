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
    let props = &crate::extract::eager(&schema(&items, "TimeLog")["properties"]);
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
        assert_eq!(crate::extract::eager(&s["additionalProperties"]), json!({}));
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
    let id = schema(&items, "Id");
    assert_eq!(
        crate::extract::eager(&id),
        json!({ "type": "string", "format": "uuid", "description": "An id" })
    );
    // The alternative for a documented `Uuid` gets the same docs.
    assert_eq!(
        id["x-oas-forge-builtin"]["variants"][0]["schema"],
        json!({ "$ref": "$Uuid", "description": "An id" })
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

mod fragments {
    use super::*;
    use crate::common::pipeline;
    use crate::extract::{fragment, kinds};

    #[test]
    fn fragments_before_a_type_are_registered() {
        let (items, diagnostics) = extract_with_diagnostics(
            r#"
            /// @openapi-fragment Paging(max)
            /// - name: page
            ///   in: query
            ///   schema: { type: integer, maximum: {{max}} }

            /// One page.
            /// @openapi
            pub struct Page { pub total: bool }
            "#,
        );
        assert!(diagnostics.is_empty(), "{diagnostics:?}");
        assert_eq!(kinds(&items), ["fragment:Paging", "schema:Page"]);
        let paging = fragment(&items, "Paging");
        assert_eq!(paging.params, ["max"]);
        assert_eq!(
            paging.body,
            "- name: page\n  in: query\n  schema: { type: integer, maximum: {{max}} }"
        );
        assert_eq!(paging.location.line(), 2);
        assert_eq!(
            schema(&items, "Page"),
            json!({
                "type": "object",
                "properties": { "total": { "type": "boolean" } },
                "required": ["total"],
                "description": "One page.",
            })
        );
    }

    #[test]
    fn a_fragment_ends_at_the_next_directive_and_exports_nothing() {
        let items = extract(
            r#"
            /// @openapi-fragment A
            /// a: 1
            /// @openapi-fragment B
            /// b: 2
            /// @example { total: true }
            pub struct Hidden { pub total: bool }
            pub enum E {
                /// @openapi-fragment C
                /// c: 3
                V,
            }
            "#,
        );
        // `C` is on a variant of an enum that is not exported: a fragment
        // all the same.
        assert_eq!(kinds(&items), ["fragment:A", "fragment:B", "fragment:C"]);
        assert_eq!(fragment(&items, "B").body, "b: 2");
    }

    #[test]
    fn fragments_of_fields_and_variants_are_registered() {
        let items = extract(
            "/// @openapi\npub struct S {\n  /// @openapi-fragment F\n  /// f: 1\n  pub a: bool,\n}\n/// @openapi\npub enum E {\n  /// @openapi-fragment G\n  /// g: 1\n  V,\n}",
        );
        assert_eq!(
            kinds(&items),
            ["fragment:F", "schema:S", "fragment:G", "schema:E"]
        );
        assert_eq!(
            schema(&items, "S")["properties"]["a"],
            json!({ "type": "boolean" })
        );
    }

    #[test]
    fn fragments_of_members_of_types_that_are_not_exported_are_registered() {
        let (items, diagnostics) = extract_with_diagnostics(
            r#"
            pub struct Plain {
                /// @openapi-fragment Named
                /// a: 1
                /// @openapi rename
                pub a: bool,
            }
            pub struct Tuple(
                /// @openapi-fragment Unnamed
                /// b: 1
                pub u8,
            );
            /// @openapi skip
            pub enum Skipped {
                /// @openapi-fragment Variant
                /// c: 1
                V {
                    /// @openapi-fragment VariantField
                    /// d: 1
                    f: u8,
                },
                W(
                    /// @openapi-fragment TupleVariantField
                    /// e: 1
                    u8,
                ),
            }
            "#,
        );
        // Only fragments are read: the directives of members of a type that
        // is not exported (`@openapi rename` without a name) are not checked.
        assert!(diagnostics.is_empty(), "{diagnostics:?}");
        assert_eq!(
            kinds(&items),
            [
                "fragment:Named",
                "fragment:Unnamed",
                "fragment:Variant",
                "fragment:VariantField",
                "fragment:TupleVariantField",
            ]
        );
        assert_eq!(fragment(&items, "Named").body, "a: 1");
        assert_eq!(fragment(&items, "VariantField").location.line(), 18);
    }

    #[test]
    fn fragments_of_members_that_are_not_reflected_one_by_one_are_registered() {
        let items = extract(
            r#"
            /// @openapi
            pub struct Id(
                /// @openapi-fragment IdField
                /// a: 1
                pub u64,
            );
            /// @openapi
            #[serde(transparent)]
            pub struct Wrapper {
                /// @openapi-fragment WrappedField
                /// b: 1
                pub inner: u64,
            }
            /// @openapi
            #[derive(Serialize_repr)]
            #[repr(u8)]
            pub enum Level {
                /// @openapi-fragment Low
                /// c: 1
                Low = 1,
            }
            /// @openapi
            #[serde(into = "String")]
            pub enum Code {
                /// @openapi-fragment Converted
                /// d: 1
                A,
            }
            "#,
        );
        assert_eq!(
            kinds(&items),
            [
                "fragment:IdField",
                "schema:Id",
                "fragment:WrappedField",
                "schema:Wrapper",
                "fragment:Low",
                "schema:Level",
                "fragment:Converted",
                "schema:Code",
            ]
        );
    }

    #[test]
    fn fragments_of_enums_are_registered_once_in_source_order() {
        let items = extract(
            r#"
            /// @openapi-fragment Container
            /// a: 1
            ///
            /// @openapi
            #[serde(tag = "kind")]
            pub enum Event {
                /// @openapi-fragment First
                /// b: 1
                Created {
                    /// @openapi-fragment FirstField
                    /// c: 1
                    id: u64,
                },
                /// @openapi-fragment Second
                /// d: 1
                Deleted,
            }
            "#,
        );
        assert_eq!(
            kinds(&items),
            [
                "fragment:Container",
                "fragment:First",
                "fragment:FirstField",
                "fragment:Second",
                "schema:EventCreated",
                "schema:EventDeleted",
                "schema:Event",
            ]
        );
    }

    #[test]
    fn fragments_on_fields_of_types_that_are_not_exported_can_be_inserted() {
        let (document, diagnostics) = pipeline(&[(
            "lib.rs",
            r#"
            /// @openapi
            /// openapi: 3.1.0
            /// info: { title: T, version: 1.0.0 }
            pub fn root() {}

            pub struct Query {
                /// @openapi-fragment Paging
                /// - name: page
                ///   in: query
                ///   schema: { type: integer }
                pub page: u32,
            }

            /// @route GET /x
            /// parameters:
            ///   - @insert Paging
            pub async fn x() {}
            "#,
        )]);
        assert!(diagnostics.is_empty(), "{diagnostics:?}");
        assert_eq!(
            document["paths"]["/x"]["get"]["parameters"][0]["name"],
            "page"
        );
    }

    #[test]
    fn fragments_on_types_can_be_inserted() {
        let (document, diagnostics) = pipeline(&[(
            "lib.rs",
            r#"
            /// @openapi
            /// openapi: 3.1.0
            /// info: { title: T, version: 1.0.0 }
            pub fn root() {}

            /// @openapi-fragment Paging
            /// - name: page
            ///   in: query
            ///   schema: { type: integer }

            /// One page.
            /// @openapi
            pub struct Page { pub total: u64 }

            /// @route GET /x
            /// parameters:
            ///   - @insert Paging
            pub async fn x() {}
            "#,
        )]);
        assert!(diagnostics.is_empty(), "{diagnostics:?}");
        assert_eq!(
            document["paths"]["/x"]["get"]["parameters"][0]["name"],
            "page"
        );
        assert_eq!(
            document["components"]["schemas"]["Page"]["description"],
            "One page."
        );
    }
}

mod builtin_names {
    use crate::common::pipeline;
    use serde_json::{Value, json};

    const USER_TYPES: &str = r#"
        /// A duration of our own.
        /// @openapi
        pub struct Duration { pub minutes: u32 }
        /// @openapi rename "JsonValue"
        pub enum Value { Text(String), Number(f64) }
        "#;

    const JOB: &str = r#"
        use std::collections::HashMap;
        /// @openapi
        pub struct Job {
            /// The timeout.
            #[deprecated]
            pub timeout: Duration,
            pub retry: Option<Duration>,
            pub steps: Vec<Duration>,
            pub pair: (Uuid, Duration),
            pub hooks: Vec<(serde_json::Value, Duration)>,
            pub std: std::time::Duration,
            pub value: Value,
            pub json: serde_json::Value,
            #[serde(flatten)]
            pub extra: HashMap<String, Duration>,
        }
        /// @openapi
        pub struct Timeout(pub Duration);
        /// @openapi
        pub type Delay = Duration;
        "#;

    /// The second of two bare names is the documented one (alternatives are
    /// built for every subset; the right one must be picked).
    #[test]
    fn only_the_documented_one_of_several_names_is_a_reference() {
        let schemas = schemas(&[(
            "lib.rs",
            "/// @openapi rename \"JsonValue\"\npub enum Value { Text(String) }\n/// @openapi\npub struct Pair { pub pair: (Duration, Value) }",
        )]);
        let items = &schemas["Pair"]["properties"]["pair"]["prefixItems"];
        assert_eq!(items[0]["type"], "object", "std::time::Duration");
        assert_eq!(
            items[1],
            json!({ "$ref": "#/components/schemas/JsonValue" })
        );
    }

    /// Six names are the limit that still gets alternatives.
    #[test]
    fn six_names_in_one_type_still_follow_documented_types() {
        let schemas = schemas(&[(
            "lib.rs",
            "/// @openapi\npub struct Duration { pub minutes: u32 }\n/// @openapi\npub struct Six { pub all: (Uuid, Url, Value, Decimal, Bytes, Duration) }",
        )]);
        assert_eq!(
            schemas["Six"]["properties"]["all"]["prefixItems"][5],
            json!({ "$ref": "#/components/schemas/Duration" })
        );
    }

    fn schemas(files: &[(&str, &str)]) -> Value {
        let (document, diagnostics) = pipeline(files);
        assert!(diagnostics.is_empty(), "{diagnostics:?}");
        let text = document.to_string();
        assert!(!text.contains("x-oas-forge-builtin"), "{text}");
        document["components"]["schemas"].clone()
    }

    #[test]
    fn documented_types_win_over_built_in_names() {
        let schemas = schemas(&[("job.rs", JOB), ("types.rs", USER_TYPES)]);
        let job = &schemas["Job"]["properties"];
        let duration = json!({ "$ref": "#/components/schemas/Duration" });
        assert_eq!(
            job["timeout"],
            json!({
                "$ref": "#/components/schemas/Duration",
                "description": "The timeout.",
                "deprecated": true,
            })
        );
        assert_eq!(
            job["retry"],
            json!({ "anyOf": [duration, { "type": "null" }] })
        );
        assert_eq!(job["steps"]["items"], duration);
        assert_eq!(job["hooks"]["items"]["prefixItems"], json!([{}, duration]));
        assert_eq!(
            job["pair"]["prefixItems"],
            json!([{ "type": "string", "format": "uuid" }, duration])
        );
        // Paths keep the built-in type.
        assert_eq!(job["std"]["required"], json!(["secs", "nanos"]));
        assert_eq!(job["json"], json!({}));
        // References follow the rename of the documented type.
        assert_eq!(
            job["value"],
            json!({ "$ref": "#/components/schemas/JsonValue" })
        );
        assert_eq!(schemas["Job"]["additionalProperties"], duration);
        assert_eq!(schemas["Timeout"], duration);
        assert_eq!(schemas["Delay"], duration);
    }

    #[test]
    fn without_documented_types_the_built_ins_stay() {
        let schemas = schemas(&[("job.rs", JOB)]);
        let job = &schemas["Job"]["properties"];
        assert_eq!(job["timeout"]["required"], json!(["secs", "nanos"]));
        assert_eq!(job["timeout"]["description"], "The timeout.");
        assert_eq!(job["retry"]["type"], json!(["object", "null"]));
        assert_eq!(job["value"], json!({}));
        assert_eq!(schemas["Timeout"]["type"], "object");
    }

    #[test]
    fn named_yaml_schemas_count_as_documented() {
        let schemas = schemas(&[
            (
                "job.rs",
                "/// @openapi\npub struct Job { pub id: Uuid, pub at: Option<Url> }",
            ),
            (
                "types.rs",
                "//! @openapi-type Uuid\n//! type: string\n//! pattern: '^[0-9a-f-]{36}$'",
            ),
        ]);
        let job = &schemas["Job"]["properties"];
        assert_eq!(job["id"], json!({ "$ref": "#/components/schemas/Uuid" }));
        assert_eq!(
            job["at"],
            json!({ "type": ["string", "null"], "format": "uri" })
        );
    }

    #[test]
    fn types_with_very_many_built_in_names_keep_the_built_ins() {
        let schemas = schemas(&[(
            "lib.rs",
            "/// @openapi\npub struct Uuid { pub raw: String }\n/// @openapi\npub struct Job { pub all: (Uuid, Ulid, Url, Uri, Decimal, Bytes, IpAddr) }",
        )]);
        assert_eq!(
            schemas["Job"]["properties"]["all"]["prefixItems"][0],
            json!({ "type": "string", "format": "uuid" })
        );
    }

    #[test]
    fn custom_mappings_are_not_replaced() {
        let project = crate::common::Project::new();
        project
            .file(
                "src/job.rs",
                "/// @openapi\npub struct Job { pub id: Uuid }",
            )
            .file(
                "src/types.rs",
                "/// @openapi\npub struct Uuid { pub raw: String }",
            );
        let spec = oas_forge::Generator::new()
            .input(project.path("src"))
            .map_type("Uuid", json!({ "type": "string", "format": "uuid" }))
            .build()
            .unwrap();
        assert_eq!(
            spec.document()["components"]["schemas"]["Job"]["properties"]["id"],
            json!({ "type": "string", "format": "uuid" })
        );
    }
}

mod undocumentable {
    use super::*;

    #[test]
    fn fields_that_cannot_be_documented_are_empty_schemas_and_reported() {
        let (items, diagnostics) = extract_with_diagnostics(
            r#"/// @openapi
pub struct Hooks {
    pub on_close: fn(&mut tokio::net::TcpStream),
    pub listener: TapIo<TcpListener, fn(&mut TcpStream)>,
    pub error: Box<dyn std::error::Error + Send>,
    pub many: Vec<impl Fn()>,
    pub made: my_type!(),
    #[serde(skip)]
    pub skipped: fn(),
    pub marker: std::marker::PhantomData<fn() -> u8>,
    pub fine: u8,
}
/// @openapi
pub struct Callback(pub fn(u8) -> u8);
"#,
        );
        let hooks = schema(&items, "Hooks");
        for field in ["on_close", "listener", "error", "made"] {
            assert_eq!(hooks["properties"][field], json!({}), "field `{field}`");
        }
        assert_eq!(
            hooks["properties"]["many"],
            json!({ "type": "array", "items": {} })
        );
        assert_eq!(hooks["properties"]["marker"], json!({ "type": "null" }));
        assert_eq!(schema(&items, "Callback"), json!({}));
        let findings: Vec<String> = diagnostics.iter().map(ToString::to_string).collect();
        assert_eq!(
            findings,
            [
                "warning[OF1010]: `fn(&mut tokio::net::TcpStream)` cannot be documented (a function pointer, trait object, `impl Trait` or macro); its schema is `{}` (test.rs:3)",
                "warning[OF1010]: `fn(&mut TcpStream)` cannot be documented (a function pointer, trait object, `impl Trait` or macro); its schema is `{}` (test.rs:4)",
                "warning[OF1010]: `dyn std::error::Error + Send` cannot be documented (a function pointer, trait object, `impl Trait` or macro); its schema is `{}` (test.rs:5)",
                "warning[OF1010]: `impl Fn()` cannot be documented (a function pointer, trait object, `impl Trait` or macro); its schema is `{}` (test.rs:6)",
                "warning[OF1010]: `my_type!()` cannot be documented (a function pointer, trait object, `impl Trait` or macro); its schema is `{}` (test.rs:7)",
                "warning[OF1010]: `fn(u8) -> u8` cannot be documented (a function pointer, trait object, `impl Trait` or macro); its schema is `{}` (test.rs:14)",
            ]
        );
    }

    #[test]
    fn types_that_are_not_documented_report_nothing() {
        let (items, diagnostics) = extract_with_diagnostics(
            "pub struct Plain { pub f: fn() }\n#[derive(Serialize)]\npub enum E { A(Box<dyn Fn()>) }",
        );
        assert!(items.is_empty(), "{items:?}");
        assert!(diagnostics.is_empty(), "{diagnostics:?}");
    }

    #[test]
    fn variants_that_cannot_be_documented_are_reported() {
        let (_, diagnostics) = extract_with_diagnostics(
            "/// @openapi\npub enum Job {\n    Run(Box<dyn FnOnce()>),\n    Wait { until: fn() -> bool },\n}",
        );
        let lines: Vec<usize> = diagnostics
            .iter()
            .filter(|d| d.code() == Code::UndocumentableType)
            .filter_map(|d| d.location().map(|l| l.line()))
            .collect();
        assert_eq!(lines, [3, 4]);
    }
}

/// Overrides merged into a schema that accepts `null` keep accepting it.
mod nullable_overrides {
    use super::*;
    use crate::common::pipeline;

    fn field(docs: &str, ty: &str) -> serde_json::Value {
        let items = extract(&format!(
            "/// @openapi\npub struct S {{\n{docs}\npub field: {ty},\n}}"
        ));
        schema(&items, "S")["properties"]["field"].clone()
    }

    #[test]
    fn an_enum_gets_null() {
        assert_eq!(
            field(
                "/// @openapi enum: [central, gateway]\n#[serde(skip_serializing_if = \"Option::is_none\")]",
                "Option<&'static str>"
            ),
            json!({ "type": ["string", "null"], "enum": ["central", "gateway", null] })
        );
        assert_eq!(
            field("/// @openapi enum: [1, null, 2]", "Option<u8>")["enum"],
            json!([1, null, 2]),
            "an enum with `null` stays as written"
        );
    }

    #[test]
    fn a_const_becomes_an_enum_with_null() {
        assert_eq!(
            field("/// @openapi const: central", "Option<String>"),
            json!({ "type": ["string", "null"], "enum": ["central", null] })
        );
    }

    #[test]
    fn an_explicit_type_or_nullable_is_kept() {
        assert_eq!(
            field(
                "/// @openapi\n/// type: string\n/// enum: [a]",
                "Option<String>"
            ),
            json!({ "type": "string", "enum": ["a"] })
        );
        assert_eq!(
            field(
                "/// @openapi\n/// nullable: false\n/// enum: [a]",
                "Option<String>"
            ),
            json!({ "type": ["string", "null"], "enum": ["a"], "nullable": false })
        );
    }

    #[test]
    fn schemas_without_null_are_unchanged() {
        assert_eq!(
            field("/// @openapi enum: [a, b]", "String"),
            json!({ "type": "string", "enum": ["a", "b"] })
        );
        assert_eq!(
            field("/// @openapi const: a", "String"),
            json!({ "type": "string", "const": "a" })
        );
    }

    #[test]
    fn a_nullable_reference_stays_nullable() {
        assert_eq!(
            field("/// @openapi enum: [a, b]", "Option<Code>"),
            json!({ "anyOf": [{ "enum": ["a", "b"] }, { "type": "null" }] })
        );
        assert_eq!(
            field(
                "/// @openapi\n/// type: string\n/// const: a",
                "Option<Code>"
            ),
            json!({ "anyOf": [{ "type": "string", "const": "a" }, { "type": "null" }] })
        );
    }

    #[test]
    fn openapi_30_lowering_keeps_null_in_the_enum() {
        let (document, diagnostics) = pipeline(&[(
            "lib.rs",
            "//! @openapi\n//! openapi: 3.0.3\n//! info: { title: T, version: '1' }\n\n/// @openapi\npub struct S {\n    /// @openapi enum: [central, gateway]\n    pub a: Option<String>,\n    /// @openapi const: c\n    pub b: Option<String>,\n}",
        )]);
        assert!(
            diagnostics
                .iter()
                .all(|d| d.code() != Code::InvalidOverride),
            "{diagnostics:?}"
        );
        let s = &document["components"]["schemas"]["S"]["properties"];
        assert_eq!(
            s["a"],
            json!({ "type": "string", "nullable": true, "enum": ["central", "gateway", null] })
        );
        assert_eq!(
            s["b"],
            json!({ "type": "string", "nullable": true, "enum": ["c", null] })
        );
    }
}

/// Required and nullable fields follow the serde direction of the container.
mod serde_direction {
    use super::*;
    use serde_json::Value;

    /// The schema of `struct S` with `derives` and `body`.
    fn struct_schema(header: &str, body: &str) -> Value {
        let items = extract(&format!(
            "/// @openapi\n{header}\npub struct S {{\n{body}\n}}"
        ));
        schema(&items, "S")
    }

    const FIELDS: &str = r#"
        pub plain: Option<u8>,
        #[serde(skip_serializing_if = "Option::is_none")]
        pub skipped_none: Option<u8>,
        #[serde(skip_serializing_if = "std::option::Option::is_none")]
        pub std_none: Option<u8>,
        #[serde(skip_serializing_if = "Option::<u8>::is_none")]
        pub turbofish_none: Option<u8>,
        #[serde(skip_serializing_if = "is_zero")]
        pub other_check: Option<u8>,
        #[serde(skip_serializing_if = "Vec::is_empty")]
        pub list: Vec<u8>,
        #[serde(default)]
        pub defaulted: u8,
        pub value: u8,
    "#;

    fn int() -> Value {
        json!({ "type": "integer", "format": "int32", "minimum": 0, "maximum": 255 })
    }

    fn nullable_int() -> Value {
        json!({ "type": ["integer", "null"], "format": "int32", "minimum": 0, "maximum": 255 })
    }

    #[test]
    fn output_types_require_every_field_that_is_always_written() {
        for header in [
            "#[derive(Serialize)]",
            "#[derive(Debug, serde::Serialize)]\n#[serde(default)]",
            "#[cfg_attr(feature = \"api\", derive(Serialize))]",
        ] {
            let s = struct_schema(header, FIELDS);
            assert_eq!(
                s["required"],
                json!(["plain", "defaulted", "value"]),
                "{header}"
            );
            let properties = &s["properties"];
            assert_eq!(properties["plain"], nullable_int(), "`null` is written");
            for name in ["skipped_none", "std_none", "turbofish_none"] {
                assert_eq!(properties[name], int(), "{name}: `None` is left out");
            }
            assert_eq!(properties["other_check"], nullable_int());
        }
    }

    #[test]
    fn input_and_unknown_types_keep_the_rules_of_both_directions() {
        for header in [
            "#[derive(Deserialize)]",
            "#[derive(Serialize, Deserialize)]",
            "#[derive(Serialize)]\n#[cfg_attr(test, derive(serde::Deserialize))]",
            "",
        ] {
            let s = struct_schema(header, FIELDS);
            assert_eq!(s["required"], json!(["value"]), "{header}");
            assert_eq!(s["properties"]["skipped_none"], nullable_int(), "{header}");
        }
    }

    #[test]
    fn struct_variants_follow_their_enum() {
        let items = extract(
            "/// @openapi\n#[derive(Serialize)]\npub enum Event {\n    Moved { to: Option<u8>, #[serde(skip_serializing_if = \"Option::is_none\")] by: Option<u8> },\n}",
        );
        let moved = &schema(&items, "Event")["oneOf"][0]["properties"]["Moved"];
        assert_eq!(moved["required"], json!(["to"]));
        assert_eq!(moved["properties"]["by"], int());
    }

    #[test]
    fn an_omitted_none_takes_no_null_into_an_enum() {
        let s = struct_schema(
            "#[derive(serde::Serialize)]",
            "/// @openapi enum: [central, gateway]\n#[serde(skip_serializing_if = \"Option::is_none\")]\npub terms_required: Option<&'static str>,\n#[serde(skip_serializing_if = \"Option::is_none\")]\npub ban: Option<Ban>,",
        );
        assert_eq!(
            s["properties"]["terms_required"],
            json!({ "type": "string", "enum": ["central", "gateway"] })
        );
        assert_eq!(s["properties"]["ban"], json!({ "$ref": "$Ban" }));
        assert!(s.get("required").is_none(), "{s}");
    }

    #[test]
    fn a_required_override_moves_the_field_into_or_out_of_the_list() {
        let s = struct_schema(
            "#[derive(Deserialize)]",
            "/// @openapi required: true\npub a: Option<u8>,\n/// @openapi\n/// required: false\n/// minimum: 1\npub b: u8,\n/// @openapi required: [x]\npub c: Inner,",
        );
        assert_eq!(s["required"], json!(["a", "c"]));
        assert_eq!(s["properties"]["a"], nullable_int());
        assert_eq!(s["properties"]["b"]["minimum"], 1);
        assert!(s["properties"]["b"].get("required").is_none());
        assert_eq!(
            s["properties"]["c"],
            json!({ "$ref": "$Inner", "required": ["x"] }),
            "a list is a keyword of the property"
        );
    }

    #[test]
    fn a_required_override_on_a_built_in_name_is_lifted_everywhere() {
        let (document, diagnostics) = crate::common::pipeline(&[(
            "lib.rs",
            "/// @openapi\n#[derive(Deserialize)]\npub struct S {\n    /// @openapi required: false\n    pub d: Duration,\n}",
        )]);
        assert!(diagnostics.is_empty(), "{diagnostics:?}");
        let s = &document["components"]["schemas"]["S"];
        assert!(s.get("required").is_none(), "{s}");
        assert_eq!(s["properties"]["d"]["required"], json!(["secs", "nanos"]));
    }
}

#[test]
fn a_lifted_required_keeps_the_order_of_the_other_keys() {
    let items = extract(
        "/// @openapi\npub struct S {\n    /// @openapi\n    /// required: true\n    /// minLength: 1\n    /// maxLength: 2\n    pub a: Option<String>,\n}",
    );
    let a = &schema(&items, "S")["properties"]["a"];
    let keys: Vec<&String> = a.as_object().unwrap().keys().collect();
    assert_eq!(keys, ["type", "minLength", "maxLength"]);
}

/// `@openapi` overrides in fenced code blocks.
mod fenced_overrides {
    use super::*;
    use crate::extract::fragment;

    #[test]
    fn fenced_overrides_of_types_and_fields() {
        let fenced = extract(
            "/// A user.\n/// @openapi\n/// ```yaml\n/// x-table: users\n/// ```\n/// More about users.\npub struct User {\n    /// @openapi\n    /// ```yaml\n    /// format: email\n    /// ```\n    pub email: String,\n}\n\n/// @openapi<T>\n/// ~~~yaml\n/// x-page: true\n/// ~~~\npub struct Page<T> { pub items: Vec<T> }",
        );
        let plain = extract(
            "/// A user.\n///\n/// More about users.\n/// @openapi\n/// x-table: users\npub struct User {\n    /// @openapi\n    /// format: email\n    pub email: String,\n}\n\n/// @openapi<T>\n/// x-page: true\npub struct Page<T> { pub items: Vec<T> }",
        );
        assert_eq!(schema(&fenced, "User"), schema(&plain, "User"));
        assert_eq!(schema(&fenced, "User")["x-table"], "users");
        assert_eq!(
            schema(&fenced, "User")["properties"]["email"]["format"],
            "email"
        );
        let blueprint = |items: &[oas_forge::extract::Item]| {
            crate::extract::blueprint(items, "Page").schema.clone()
        };
        assert_eq!(blueprint(&fenced), blueprint(&plain));
    }

    #[test]
    fn fenced_fragments_and_documents_on_types() {
        let items = extract(
            "/// @openapi-fragment Audit\n/// ```yaml\n/// x-audit: true\n/// ```\n///\n/// @openapi document\n/// ```yaml\n/// tags: [{ name: Users }]\n/// ```\n/// @openapi\npub struct User { pub id: u64 }",
        );
        assert_eq!(fragment(&items, "Audit").body, "x-audit: true");
        assert_eq!(
            crate::extract::document_items(&items)[0].text,
            "tags: [{ name: Users }]"
        );
    }
}

#[test]
fn document_blocks_on_types_keep_the_indentation_of_their_yaml() {
    let items = extract(
        "/// @openapi document\n/// x-first: 1\n/// x-second:\n///   nested: 2\n/// @openapi\npub struct User { pub id: u64 }",
    );
    assert_eq!(
        crate::extract::document_items(&items)[0].text,
        "x-first: 1\nx-second:\n  nested: 2"
    );
}
