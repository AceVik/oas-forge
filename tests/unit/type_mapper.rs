use oas_forge::type_mapper::map_syn_type_to_openapi;
use serde_json::{Value, json};
use syn::punctuated::Punctuated;

fn map(ty: &str) -> (Value, bool) {
    let parsed: syn::Type =
        syn::parse_str(ty).unwrap_or_else(|e| panic!("`{ty}` is not a type: {e}"));
    map_syn_type_to_openapi(&parsed)
}

fn schema(ty: &str) -> Value {
    map(ty).0
}

fn assert_all(types: &[&str], expected: Value) {
    for ty in types {
        assert_eq!(map(ty), (expected.clone(), true), "type `{ty}`");
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
    assert_eq!(
        map("Option<i64>"),
        (json!({ "type": "integer", "format": "int64" }), false)
    );
    assert_eq!(map("Option"), (json!({}), false));
    assert_eq!(map("Option<'a>"), (json!({}), false));
    assert!(!map("Option<Vec<String>>").1);
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
            "Cow<'a, String>",
            "&str",
            "&'a String",
        ],
        json!({ "type": "string" }),
    );
    assert!(
        !map("Box<Option<String>>").1,
        "Option inside Box stays optional"
    );
    // Regression: the leading lifetime made `Cow<'a, T>` a `$ref: $Cow`.
    assert_eq!(
        schema("Cow<'a, [u8]>"),
        json!({ "type": "array", "items": { "type": "integer", "format": "int32" } })
    );
}

#[test]
fn custom_types_become_smart_refs() {
    assert_all(&["User", "crate::models::User"], json!({ "$ref": "$User" }));
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

#[test]
fn path_without_segments_is_a_plain_object() {
    let empty = syn::Type::Path(syn::TypePath {
        qself: None,
        path: syn::Path {
            leading_colon: None,
            segments: Punctuated::new(),
        },
    });
    assert_eq!(
        map_syn_type_to_openapi(&empty),
        (json!({ "type": "object" }), true)
    );
}

#[test]
fn visitor_re_exports_the_mapper() {
    let ty: syn::Type = syn::parse_str("bool").unwrap();
    assert_eq!(
        oas_forge::visitor::map_syn_type_to_openapi(&ty),
        map_syn_type_to_openapi(&ty)
    );
}
