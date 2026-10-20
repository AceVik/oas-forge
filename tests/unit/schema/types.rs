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

#[test]
fn primitives() {
    assert_all(&["bool"], json!({ "type": "boolean" }));
    assert_all(&["String", "str", "char"], json!({ "type": "string" }));
    assert_all(
        &["i8", "i16", "i32", "u8", "u16", "u32"],
        json!({ "type": "integer", "format": "int32" }),
    );
    assert_all(
        &["i64", "u64", "isize", "usize"],
        json!({ "type": "integer", "format": "int64" }),
    );
    assert_all(&["f32"], json!({ "type": "number", "format": "float" }));
    assert_all(&["f64"], json!({ "type": "number", "format": "double" }));
}

#[test]
fn well_known_library_types() {
    assert_all(
        &["Uuid", "uuid::Uuid"],
        json!({ "type": "string", "format": "uuid" }),
    );
    assert_all(
        &["NaiveDate"],
        json!({ "type": "string", "format": "date" }),
    );
    assert_all(
        &["DateTime<Utc>", "chrono::NaiveDateTime", "DateTimeUtc"],
        json!({ "type": "string", "format": "date-time" }),
    );
    assert_all(
        &["NaiveTime"],
        json!({ "type": "string", "format": "time" }),
    );
    assert_all(
        &["Url", "Uri"],
        json!({ "type": "string", "format": "uri" }),
    );
    assert_all(
        &["Decimal", "BigDecimal"],
        json!({ "type": "string", "format": "decimal" }),
    );
    assert_all(
        &["ObjectId"],
        json!({ "type": "string", "format": "objectid" }),
    );
    assert_all(&["serde_json::Value"], json!({}));
}

#[test]
fn option_is_not_required() {
    let option = map("Option<i64>");
    assert_eq!(
        option.schema,
        json!({ "type": "integer", "format": "int64" })
    );
    assert!(!option.required);
    assert_eq!(map("Option").schema, json!({}));
    assert!(!map("Option<'a>").required);
    assert!(!map("Option<Vec<String>>").required);
}

#[test]
fn sequences() {
    assert_all(
        &[
            "Vec<String>",
            "LinkedList<String>",
            "HashSet<String>",
            "[String; 4]",
            "[String]",
        ],
        json!({ "type": "array", "items": { "type": "string" } }),
    );
    assert_all(&["Vec", "Vec<'a>"], json!({ "type": "array" }));
    assert_eq!(
        schema("Vec<Option<u8>>"),
        json!({ "type": "array", "items": { "type": "integer", "format": "int32" } })
    );
}

#[test]
fn maps() {
    assert_all(
        &["HashMap<String, f32>", "BTreeMap<String, f32>"],
        json!({ "type": "object", "additionalProperties": { "type": "number", "format": "float" } }),
    );
    assert_all(
        &["HashMap", "HashMap<String>", "HashMap<String, 'a>"],
        json!({ "type": "object" }),
    );
}

#[test]
fn smart_pointers_and_references_are_transparent() {
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
        ],
        json!({ "type": "string" }),
    );
    assert!(
        !map("Box<Option<String>>").required,
        "Option inside Box stays optional"
    );
    assert_eq!(
        schema("Cow<'a, [u8]>"),
        json!({ "type": "array", "items": { "type": "integer", "format": "int32" } })
    );
}

#[test]
fn custom_types_become_smart_refs() {
    assert_all(
        &["User", "crate::models::User", "$User"],
        json!({ "$ref": "$User" }),
    );
    assert_all(&["Page<User>"], json!({ "$ref": "$Page" }));
    assert_all(&["Box"], json!({ "$ref": "$Box" }));
}

#[test]
fn other_type_forms_are_plain_objects() {
    assert_all(
        &["(i32, String)", "()", "fn()", "impl Trait"],
        json!({ "type": "object" }),
    );
}
