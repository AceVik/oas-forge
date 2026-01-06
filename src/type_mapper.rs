use serde_json::{Value, json};

/// Helper for type mapping
/// Converts a `syn::Type` into an OpenAPI JSON Schema.
/// Returns a tuple of (Schema Value, is_required).
pub fn map_syn_type_to_openapi(ty: &syn::Type) -> (Value, bool) {
    match ty {
        syn::Type::Path(p) => {
            if let Some(seg) = p.path.segments.last() {
                let ident = seg.ident.to_string();

                if ["Box", "Arc", "Rc", "Cow"].contains(&ident.as_str()) {
                    if let syn::PathArguments::AngleBracketed(args) = &seg.arguments {
                        if let Some(syn::GenericArgument::Type(inner)) = args.args.first() {
                            return map_syn_type_to_openapi(inner);
                        }
                    }
                }

                match ident.as_str() {
                    "bool" => (json!({ "type": "boolean" }), true),
                    "String" | "str" | "char" => (json!({ "type": "string" }), true),
                    "i8" | "i16" | "i32" | "u8" | "u16" | "u32" => {
                        (json!({ "type": "integer", "format": "int32" }), true)
                    }
                    "i64" | "u64" | "isize" | "usize" => {
                        (json!({ "type": "integer", "format": "int64" }), true)
                    }
                    "f32" => (json!({ "type": "number", "format": "float" }), true),
                    "f64" => (json!({ "type": "number", "format": "double" }), true),
                    "Uuid" => (json!({ "type": "string", "format": "uuid" }), true),
                    "NaiveDate" => (json!({ "type": "string", "format": "date" }), true),
                    "DateTime" | "NaiveDateTime" | "DateTimeUtc" => {
                        (json!({ "type": "string", "format": "date-time" }), true)
                    }
                    "NaiveTime" => (json!({ "type": "string", "format": "time" }), true),
                    "Url" | "Uri" => (json!({ "type": "string", "format": "uri" }), true),
                    "Decimal" | "BigDecimal" => {
                        (json!({ "type": "string", "format": "decimal" }), true)
                    }
                    "ObjectId" => (json!({ "type": "string", "format": "objectid" }), true),
                    "Value" => (json!({}), true),
                    "Option" => {
                        if let syn::PathArguments::AngleBracketed(args) = &seg.arguments {
                            if let Some(syn::GenericArgument::Type(inner)) = args.args.first() {
                                let (inner_val, _) = map_syn_type_to_openapi(inner);
                                return (inner_val, false);
                            }
                        }
                        (json!({}), false)
                    }
                    "Vec" | "LinkedList" | "HashSet" => {
                        if let syn::PathArguments::AngleBracketed(args) = &seg.arguments {
                            if let Some(syn::GenericArgument::Type(inner)) = args.args.first() {
                                let (inner_val, _) = map_syn_type_to_openapi(inner);
                                return (json!({ "type": "array", "items": inner_val }), true);
                            }
                        }
                        (json!({ "type": "array" }), true)
                    }
                    "HashMap" | "BTreeMap" => {
                        if let syn::PathArguments::AngleBracketed(args) = &seg.arguments {
                            if args.args.len() >= 2 {
                                if let syn::GenericArgument::Type(val_type) = &args.args[1] {
                                    let (val_schema, _) = map_syn_type_to_openapi(val_type);
                                    return (
                                        json!({ "type": "object", "additionalProperties": val_schema }),
                                        true,
                                    );
                                }
                            }
                        }
                        (json!({ "type": "object" }), true)
                    }
                    _ => (json!({ "$ref": format!("${}", ident) }), true),
                }
            } else {
                (json!({ "type": "object" }), true)
            }
        }
        syn::Type::Array(a) => {
            let (inner, _) = map_syn_type_to_openapi(&a.elem);
            (json!({ "type": "array", "items": inner }), true)
        }
        syn::Type::Slice(s) => {
            let (inner, _) = map_syn_type_to_openapi(&s.elem);
            (json!({ "type": "array", "items": inner }), true)
        }
        syn::Type::Reference(r) => map_syn_type_to_openapi(&r.elem),
        _ => (json!({ "type": "object" }), true),
    }
}
