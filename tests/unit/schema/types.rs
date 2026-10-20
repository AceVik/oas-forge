use oas_forge::schema::{MappedType, TypeExpr, TypeMapper};
use serde_json::{Value, json};

fn map(ty: &str) -> MappedType {
    let parsed = TypeExpr::parse(ty).unwrap_or_else(|| panic!("`{ty}` is not a type"));
    TypeMapper::new().map(&parsed)
}

fn schema(ty: &str) -> Value {
    map(ty).schema
}

fn assert_all(types: &[&str], expected: Value) {
    for ty in types {
        let mapped = map(ty);
        assert_eq!(mapped.schema, expected, "type `{ty}`");
        assert!(mapped.required, "type `{ty}` is required");
    }
}

mod parsing {
    use super::*;

    #[test]
    fn named_types_keep_path_and_type_arguments() {
        let ty = TypeExpr::parse("chrono::DateTime<'static, Utc>").unwrap();
        assert_eq!(ty.name(), Some("DateTime"));
        assert_eq!(ty.args(), &[TypeExpr::parse("Utc").unwrap()]);
        assert!(!ty.is_smart());
        assert_eq!(ty.to_string(), "chrono::DateTime<Utc>");
    }

    #[test]
    fn dollar_marks_smart_references() {
        let ty = TypeExpr::parse("$Page<$User, Item>").unwrap();
        assert!(ty.is_smart());
        assert_eq!(ty.name(), Some("Page"));
        assert!(ty.args()[0].is_smart());
        assert!(!ty.args()[1].is_smart());
        assert_eq!(ty.to_string(), "$Page<$User, Item>");
        assert_eq!(TypeExpr::smart("User").to_string(), "$User");
    }

    #[test]
    fn structural_types() {
        assert_eq!(TypeExpr::parse("[u8; 4]").unwrap().to_string(), "[u8; 4]");
        assert_eq!(TypeExpr::parse("[u8; N]").unwrap().to_string(), "[u8; _]");
        assert_eq!(TypeExpr::parse("&[u16]").unwrap().to_string(), "&[u16]");
        assert_eq!(
            TypeExpr::parse("(i32, (String))").unwrap().to_string(),
            "(i32, String)"
        );
        assert_eq!(TypeExpr::parse("()").unwrap().to_string(), "()");
        assert_eq!(TypeExpr::parse("fn()").unwrap().to_string(), "fn ()");
        assert_eq!(TypeExpr::parse("&str").unwrap().name(), None);
        assert!(TypeExpr::parse("&str").unwrap().args().is_empty());
    }

    #[test]
    fn invalid_text_is_not_a_type() {
        assert_eq!(TypeExpr::parse("Vec<"), None);
        assert_eq!(TypeExpr::parse(""), None);
    }
}

fn assert_each(cases: &[(&str, Value)]) {
    for (ty, expected) in cases {
        assert_eq!(&schema(ty), expected, "type `{ty}`");
    }
}

mod table {
    use super::*;

    #[test]
    fn primitives() {
        assert_all(&["bool"], json!({ "type": "boolean" }));
        assert_all(
            &["String", "str", "Box<str>", "Cow<'a, str>"],
            json!({ "type": "string" }),
        );
        assert_all(
            &["char"],
            json!({ "type": "string", "minLength": 1, "maxLength": 1 }),
        );
        assert_all(&["f32"], json!({ "type": "number", "format": "float" }));
        assert_all(&["f64"], json!({ "type": "number", "format": "double" }));
    }

    #[test]
    fn integers_carry_their_range() {
        assert_each(&[
            (
                "i8",
                json!({ "type": "integer", "format": "int32", "minimum": -128, "maximum": 127 }),
            ),
            (
                "i16",
                json!({ "type": "integer", "format": "int32", "minimum": -32768, "maximum": 32767 }),
            ),
            ("i32", json!({ "type": "integer", "format": "int32" })),
            ("i64", json!({ "type": "integer", "format": "int64" })),
            ("isize", json!({ "type": "integer", "format": "int64" })),
            ("i128", json!({ "type": "integer" })),
            (
                "u8",
                json!({ "type": "integer", "format": "int32", "minimum": 0, "maximum": 255 }),
            ),
            (
                "u16",
                json!({ "type": "integer", "format": "int32", "minimum": 0, "maximum": 65535 }),
            ),
            (
                "u32",
                json!({ "type": "integer", "format": "int64", "minimum": 0, "maximum": 4294967295u32 }),
            ),
            (
                "u64",
                json!({ "type": "integer", "format": "int64", "minimum": 0 }),
            ),
            (
                "usize",
                json!({ "type": "integer", "format": "int64", "minimum": 0 }),
            ),
            ("u128", json!({ "type": "integer", "minimum": 0 })),
        ]);
    }

    #[test]
    fn non_zero_integers_exclude_zero() {
        assert_each(&[
            (
                "NonZeroU8",
                json!({ "type": "integer", "format": "int32", "minimum": 1, "maximum": 255 }),
            ),
            (
                "std::num::NonZeroU64",
                json!({ "type": "integer", "format": "int64", "minimum": 1 }),
            ),
            (
                "NonZero<u32>",
                json!({ "type": "integer", "format": "int64", "minimum": 1, "maximum": 4294967295u32 }),
            ),
            (
                "NonZeroI32",
                json!({ "type": "integer", "format": "int32", "not": { "enum": [0] } }),
            ),
            ("NonZero<f32>", json!({ "type": "integer" })),
            ("NonZero", json!({ "type": "integer" })),
            // Only the std names: a type that merely starts with `NonZero`
            // is a reference.
            ("NonZeroX", json!({ "$ref": "$NonZeroX" })),
            ("NonZeroAmount", json!({ "$ref": "$NonZeroAmount" })),
            (
                "NonZeroUsize",
                json!({ "type": "integer", "format": "int64", "minimum": 1 }),
            ),
        ]);
    }

    #[test]
    fn library_types_use_their_serde_format() {
        assert_all(
            &["Uuid", "uuid::Uuid"],
            json!({ "type": "string", "format": "uuid" }),
        );
        assert_all(&["Ulid"], json!({ "type": "string", "format": "ulid" }));
        assert_all(
            &["Url", "Uri"],
            json!({ "type": "string", "format": "uri" }),
        );
        assert_all(
            &[
                "DateTime<Utc>",
                "DateTimeUtc",
                "OffsetDateTime",
                "time::PrimitiveDateTime",
                "UtcDateTime",
                "jiff::Timestamp",
            ],
            json!({ "type": "string", "format": "date-time" }),
        );
        // Without an offset: not RFC 3339 `date-time`/`time`, but the
        // `-local` formats of the OpenAPI format registry.
        assert_all(
            &[
                "chrono::NaiveDateTime",
                "NaiveDateTime",
                "jiff::civil::DateTime",
            ],
            json!({ "type": "string", "format": "date-time-local" }),
        );
        assert_all(
            &["NaiveDate", "time::Date", "jiff::civil::Date"],
            json!({ "type": "string", "format": "date" }),
        );
        assert_all(
            &["NaiveTime", "time::Time", "jiff::civil::Time"],
            json!({ "type": "string", "format": "time-local" }),
        );
        assert_all(
            &["jiff::SignedDuration", "jiff::Span"],
            json!({ "type": "string", "format": "duration" }),
        );
        assert_all(
            &["Decimal", "BigDecimal"],
            json!({ "type": "string", "format": "decimal" }),
        );
        assert_all(
            &["ObjectId"],
            json!({ "type": "string", "format": "objectid" }),
        );
        assert_all(&["serde_json::Value", "Value"], json!({}));
        assert_all(
            &["serde_json::Map<String, Value>", "Map<String, Value>"],
            json!({ "type": "object" }),
        );
        assert_all(&["Ipv4Addr"], json!({ "type": "string", "format": "ipv4" }));
        assert_all(
            &["std::net::Ipv6Addr"],
            json!({ "type": "string", "format": "ipv6" }),
        );
        assert_all(
            &["IpAddr"],
            json!({ "type": "string", "anyOf": [{ "format": "ipv4" }, { "format": "ipv6" }] }),
        );
        assert_all(
            &["SocketAddr", "PathBuf", "std::path::Path"],
            json!({ "type": "string" }),
        );
        assert_all(
            &["Bytes", "bytes::BytesMut"],
            json!({ "type": "array", "items": { "type": "integer", "format": "int32", "minimum": 0, "maximum": 255 } }),
        );
    }

    #[test]
    fn ambiguous_names_need_their_crate() {
        // `Date`, `Time` and `Timestamp` are common names of user types.
        for ty in ["Date", "Time", "Timestamp", "Span", "my::Date"] {
            assert_eq!(
                schema(ty),
                json!({ "$ref": format!("${}", ty.rsplit("::").next().unwrap()) }),
                "{ty}"
            );
        }
    }

    #[test]
    fn durations_and_system_time_are_objects_like_serde_writes_them() {
        assert_eq!(schema("Duration")["required"], json!(["secs", "nanos"]));
        assert_eq!(schema("std::time::Duration")["type"], "object");
        assert_eq!(
            schema("SystemTime")["required"],
            json!(["secs_since_epoch", "nanos_since_epoch"])
        );
        assert_eq!(
            schema("time::Duration")["prefixItems"]
                .as_array()
                .unwrap()
                .len(),
            2
        );
    }

    #[test]
    fn option_is_optional_and_nullable() {
        let option = map("Option<i64>");
        assert_eq!(
            option.schema,
            json!({ "type": "integer", "format": "int64" })
        );
        assert!(!option.required && option.nullable);
        assert_eq!(
            option.into_schema(),
            json!({ "type": ["integer", "null"], "format": "int64" })
        );
        assert_eq!(map("Option").schema, json!({}));
        assert!(!map("Option<'a>").required);
        assert_eq!(
            map("Option<Option<String>>").into_schema(),
            json!({ "type": ["string", "null"] })
        );
        let plain = map("String");
        assert!(plain.required && !plain.nullable);
        assert_eq!(plain.into_schema(), json!({ "type": "string" }));
    }

    #[test]
    fn sequences() {
        assert_all(
            &[
                "Vec<String>",
                "VecDeque<String>",
                "LinkedList<String>",
                "BinaryHeap<String>",
                "[String]",
                "SmallVec<[String; 4]>",
            ],
            json!({ "type": "array", "items": { "type": "string" } }),
        );
        assert_all(
            &[
                "HashSet<String>",
                "BTreeSet<String>",
                "indexmap::IndexSet<String>",
            ],
            json!({ "type": "array", "items": { "type": "string" }, "uniqueItems": true }),
        );
        assert_all(
            &["[String; 4]"],
            json!({ "type": "array", "items": { "type": "string" }, "minItems": 4, "maxItems": 4 }),
        );
        assert_all(&["Vec", "Vec<'a>", "SmallVec"], json!({ "type": "array" }));
        assert_eq!(
            schema("Vec<Option<bool>>"),
            json!({ "type": "array", "items": { "type": ["boolean", "null"] } }),
            "nested options are nullable"
        );
    }

    #[test]
    fn tuples_and_unit() {
        assert_all(
            &["(i32, String)"],
            json!({
                "type": "array",
                "prefixItems": [{ "type": "integer", "format": "int32" }, { "type": "string" }],
                "items": false, "minItems": 2, "maxItems": 2,
            }),
        );
        assert_all(&["()", "PhantomData<T>"], json!({ "type": "null" }));
        assert_all(&["fn()", "impl Trait"], json!({}));
    }

    #[test]
    fn maps() {
        assert_all(
            &[
                "HashMap<String, f32>",
                "BTreeMap<String, f32>",
                "IndexMap<String, f32>",
            ],
            json!({ "type": "object", "additionalProperties": { "type": "number", "format": "float" } }),
        );
        assert_all(
            &["HashMap", "HashMap<String>", "HashMap<String, 'a>"],
            json!({ "type": "object" }),
        );
        assert_eq!(
            schema("HashMap<u32, bool>")["propertyNames"],
            json!({ "pattern": "^[0-9]+$" })
        );
        assert_eq!(
            schema("BTreeMap<i64, bool>")["propertyNames"],
            json!({ "pattern": "^-?[0-9]+$" })
        );
        assert_eq!(
            schema("BTreeMap<bool, u8>")["propertyNames"],
            json!({ "enum": ["true", "false"] })
        );
        assert!(schema("HashMap<Uuid, u8>").get("propertyNames").is_none());
    }

    #[test]
    fn ranges() {
        assert_eq!(
            schema("Range<u8>")["properties"]["start"],
            json!({ "type": "integer", "format": "int32", "minimum": 0, "maximum": 255 })
        );
        assert_eq!(
            schema("RangeInclusive<i32>")["required"],
            json!(["start", "end"])
        );
    }

    #[test]
    fn wrappers_are_transparent() {
        assert_all(
            &[
                "Box<String>",
                "Arc<String>",
                "Rc<String>",
                "Cow<str>",
                "Cow<'static, str>",
                "&str",
                "&'a String",
                "(String)",
                "RefCell<String>",
                "Cell<String>",
                "Mutex<String>",
                "RwLock<String>",
                "Wrapping<String>",
                "Saturating<String>",
                "Reverse<String>",
            ],
            json!({ "type": "string" }),
        );
        assert!(
            !map("Box<Option<String>>").required,
            "Option inside Box stays optional"
        );
        assert_all(&["Box", "Arc"], json!({}));
    }

    #[test]
    fn custom_types_become_smart_refs() {
        assert_all(
            &["User", "crate::models::User", "$User"],
            json!({ "$ref": "$User" }),
        );
        // Arguments are written as references too (`$T`), so that type
        // parameters are substituted inside nested arguments of blueprints.
        assert_all(&["Page<User>"], json!({ "$ref": "$Page<$User>" }));
        assert_all(
            &["models::Pair<u8, Vec<User>>"],
            json!({ "$ref": "$Pair<$u8, $Vec<$User>>" }),
        );
        // Other arguments are written as Rust types.
        assert_all(
            &["Page<(u8, User)>"],
            json!({ "$ref": "$Page<(u8, User)>" }),
        );
        // Only `Map<String, Value>` is taken for `serde_json::Map`.
        assert_all(
            &["Map<String, User>"],
            json!({ "$ref": "$Map<$String, $User>" }),
        );
        assert_all(&["$Page<User>"], json!({ "$ref": "$Page<User>" }));
    }
}

mod custom_mappings {
    use super::*;

    #[test]
    fn names_and_paths_override_the_table() {
        let mapper = TypeMapper::new()
            .with_type("UserId", json!({ "type": "string", "format": "uuid" }))
            .with_type("geo::Point", json!({ "type": "array" }))
            .with_type("Uuid", json!({ "type": "string" }));
        let map = |ty: &str| mapper.map(&TypeExpr::parse(ty).unwrap()).schema;
        assert_eq!(map("UserId"), json!({ "type": "string", "format": "uuid" }));
        assert_eq!(
            map("my::UserId"),
            json!({ "type": "string", "format": "uuid" }),
            "by name"
        );
        assert_eq!(map("geo::Point"), json!({ "type": "array" }));
        assert_eq!(
            map("Point"),
            json!({ "$ref": "$Point" }),
            "paths only match paths"
        );
        assert_eq!(map("Uuid"), json!({ "type": "string" }));
        assert_eq!(map("Vec<UserId>")["items"]["format"], "uuid");
        assert_eq!(mapper.custom_types().count(), 3);
        assert_ne!(&mapper, TypeMapper::builtin());
    }
}

mod generic_arguments {
    use crate::common::pipeline;
    use serde_json::json;

    #[test]
    fn type_parameters_are_substituted_in_nested_arguments() {
        let (document, diagnostics) = pipeline(&[(
            "lib.rs",
            r#"
            /// @openapi
            pub struct Wrapper<T> { pub w: T }
            /// @openapi
            pub struct Page<T> { pub items: Vec<Wrapper<T>>, pub pairs: Vec<(u8, Wrapper<T>)> }
            /// @openapi
            pub struct User { pub id: u64 }
            /// @openapi
            pub struct H { pub p: Page<User> }
            "#,
        )]);
        let schemas = &document["components"]["schemas"];
        assert_eq!(
            schemas["Page_User"]["properties"]["items"]["items"],
            json!({ "$ref": "#/components/schemas/Wrapper_User" })
        );
        assert_eq!(
            schemas["Wrapper_User"]["properties"]["w"],
            json!({ "$ref": "#/components/schemas/User" })
        );
        assert!(schemas.get("Wrapper_T").is_none());
        assert!(diagnostics.is_empty(), "{diagnostics:?}");
    }
}
