use syn::punctuated::Punctuated;
use syn::{Attribute, Expr, ExprLit, Lit, Meta};

/// Helper to extract doc comments from attributes
pub fn extract_doc_comments(attrs: &[Attribute]) -> Vec<String> {
    let mut doc_lines = Vec::new();
    for attr in attrs {
        if attr.path().is_ident("doc") {
            if let Meta::NameValue(meta) = &attr.meta {
                if let Expr::Lit(expr_lit) = &meta.value {
                    if let Lit::Str(lit_str) = &expr_lit.lit {
                        doc_lines.push(lit_str.value());
                    }
                }
            }
        }
    }
    doc_lines
}

pub fn apply_casing(text: &str, case: &str) -> String {
    match case {
        "lowercase" => text.to_lowercase(),
        "UPPERCASE" => text.to_uppercase(),
        "PascalCase" => {
            // Check if it contains underscores (snake_case -> PascalCase)
            if text.contains('_') {
                text.split('_')
                    .map(|part| {
                        let mut c = part.chars();
                        match c.next() {
                            None => String::new(),
                            Some(f) => f.to_uppercase().to_string() + c.as_str(),
                        }
                    })
                    .collect()
            } else {
                // Assume it is already Pascal or camel, just ensure first char is Upper
                let mut c = text.chars();
                match c.next() {
                    None => String::new(),
                    Some(f) => f.to_uppercase().to_string() + c.as_str(),
                }
            }
        }
        "camelCase" => {
            // Check if it contains underscores (snake_case -> camelCase)
            if text.contains('_') {
                let parts: Vec<&str> = text.split('_').collect();
                if parts.is_empty() {
                    return String::new();
                }
                let first = parts[0].to_lowercase();
                let rest: String = parts[1..]
                    .iter()
                    .map(|part| {
                        let mut c = part.chars();
                        match c.next() {
                            None => String::new(),
                            Some(f) => f.to_uppercase().to_string() + c.as_str(),
                        }
                    })
                    .collect();
                first + &rest
            } else {
                // Just ensure first char is Lower
                let mut c = text.chars();
                match c.next() {
                    None => String::new(),
                    Some(f) => f.to_lowercase().to_string() + c.as_str(),
                }
            }
        }
        "snake_case" => {
            let mut s = String::new();
            for (i, c) in text.chars().enumerate() {
                if c.is_uppercase() && i > 0 {
                    s.push('_');
                }
                if let Some(lower) = c.to_lowercase().next() {
                    s.push(lower);
                }
            }
            s
        }
        "SCREAMING_SNAKE_CASE" => apply_casing(text, "snake_case").to_uppercase(),
        "kebab-case" => apply_casing(text, "snake_case").replace('_', "-"),
        "SCREAMING-KEBAB-CASE" => apply_casing(text, "kebab-case").to_uppercase(),
        _ => text.to_string(),
    }
}

/// Extracts doc comments and handles "@openapi rename/rename-all" + Serde logic.
pub fn extract_naming_and_doc(
    attrs: &[Attribute],
    default_name: &str,
) -> (
    String,
    String,
    Option<String>,
    Vec<String>,
    Option<String>,
    Option<String>,
) {
    let mut doc_lines = Vec::new();
    // We collect cleaned lines here (without @openapi tags)
    let mut clean_doc_lines = Vec::new();

    let mut final_name = default_name.to_string();
    let mut rename_rule = None;
    let mut serde_tag = None;
    let mut serde_content = None;

    // 1. Check Serde Attributes (Lower Precedence)
    for attr in attrs {
        if attr.path().is_ident("serde") {
            if let Meta::List(list) = &attr.meta {
                if let Ok(nested) =
                    list.parse_args_with(Punctuated::<Meta, syn::Token![,]>::parse_terminated)
                {
                    for meta in nested {
                        if let Meta::NameValue(nv) = meta {
                            if nv.path.is_ident("rename") {
                                if let Expr::Lit(ExprLit {
                                    lit: Lit::Str(s), ..
                                }) = nv.value
                                {
                                    final_name = s.value();
                                }
                            } else if nv.path.is_ident("rename_all") {
                                if let Expr::Lit(ExprLit {
                                    lit: Lit::Str(s), ..
                                }) = nv.value
                                {
                                    rename_rule = Some(s.value());
                                }
                            } else if nv.path.is_ident("tag") {
                                if let Expr::Lit(ExprLit {
                                    lit: Lit::Str(s), ..
                                }) = nv.value
                                {
                                    serde_tag = Some(s.value());
                                }
                            } else if nv.path.is_ident("content") {
                                if let Expr::Lit(ExprLit {
                                    lit: Lit::Str(s), ..
                                }) = nv.value
                                {
                                    serde_content = Some(s.value());
                                }
                            }
                        }
                    }
                }
            }
        }
    }

    // 2. Doc Comments (Higher Precedence)
    for attr in attrs {
        if attr.path().is_ident("doc") {
            if let Meta::NameValue(meta) = &attr.meta {
                if let Expr::Lit(expr_lit) = &meta.value {
                    if let Lit::Str(lit_str) = &expr_lit.lit {
                        let val = lit_str.value();
                        doc_lines.push(val.clone());
                        let trimmed = val.trim();

                        if trimmed.starts_with("@openapi") {
                            let rest = trimmed.strip_prefix("@openapi").unwrap().trim();
                            if rest.starts_with("rename-all") {
                                let rule = rest
                                    .strip_prefix("rename-all")
                                    .unwrap()
                                    .trim()
                                    .trim_matches('"');
                                rename_rule = Some(rule.to_string());
                            } else if rest.starts_with("rename") {
                                let name_part = rest
                                    .strip_prefix("rename")
                                    .unwrap()
                                    .trim()
                                    .trim_matches('"');
                                final_name = name_part.to_string();
                            } else {
                                // Only if not a rename directive, treat as doc content?
                                // Actually, standard logic splits @openapi lines separate.
                                // We just pass it through here.
                            }
                        } else {
                            clean_doc_lines.push(val.trim().to_string());
                        }
                    }
                }
            }
        }
    }

    (
        final_name,
        clean_doc_lines.join(" "),
        rename_rule,
        doc_lines,
        serde_tag,
        serde_content,
    )
}

use serde_json::{Value, json};

/// Extracts validation attributes from `#[validate(...)]` and maps them to OpenAPI properties.
pub fn extract_validation(attrs: &[Attribute]) -> Value {
    let mut validation_schema = serde_json::Map::new();

    for attr in attrs {
        if attr.path().is_ident("validate") {
            if let Meta::List(list) = &attr.meta {
                if let Ok(nested) =
                    list.parse_args_with(Punctuated::<Meta, syn::Token![,]>::parse_terminated)
                {
                    for meta in nested {
                        match meta {
                            // Helper: #[validate(email)]
                            Meta::Path(p) if p.is_ident("email") => {
                                validation_schema.insert("format".to_string(), json!("email"));
                            }
                            // Helper: #[validate(url)]
                            Meta::Path(p) if p.is_ident("url") => {
                                validation_schema.insert("format".to_string(), json!("uri"));
                            }
                            // Helper: #[validate(length(min = 1, max = 10))]
                            Meta::List(list) if list.path.is_ident("length") => {
                                if let Ok(args) = list.parse_args_with(
                                    Punctuated::<Meta, syn::Token![,]>::parse_terminated,
                                ) {
                                    for arg in args {
                                        if let Meta::NameValue(nv) = arg {
                                            if let Expr::Lit(ExprLit {
                                                lit: Lit::Int(i), ..
                                            }) = nv.value
                                            {
                                                if let Ok(val) = i.base10_parse::<u64>() {
                                                    if nv.path.is_ident("min") {
                                                        validation_schema.insert(
                                                            "minLength".to_string(),
                                                            json!(val),
                                                        );
                                                    } else if nv.path.is_ident("max") {
                                                        validation_schema.insert(
                                                            "maxLength".to_string(),
                                                            json!(val),
                                                        );
                                                    }
                                                }
                                            }
                                        }
                                    }
                                }
                            }
                            // Helper: #[validate(range(min = 1, max = 10))]
                            Meta::List(list) if list.path.is_ident("range") => {
                                if let Ok(args) = list.parse_args_with(
                                    Punctuated::<Meta, syn::Token![,]>::parse_terminated,
                                ) {
                                    for arg in args {
                                        if let Meta::NameValue(nv) = arg {
                                            if let Expr::Lit(ExprLit {
                                                lit: Lit::Int(i), ..
                                            }) = nv.value
                                            {
                                                if let Ok(val) = i.base10_parse::<i64>() {
                                                    if nv.path.is_ident("min") {
                                                        validation_schema.insert(
                                                            "minimum".to_string(),
                                                            json!(val),
                                                        );
                                                    } else if nv.path.is_ident("max") {
                                                        validation_schema.insert(
                                                            "maximum".to_string(),
                                                            json!(val),
                                                        );
                                                    }
                                                }
                                            }
                                        }
                                    }
                                }
                            }
                            // Helper: #[validate(regex = "path")] or #[validate(pattern = "...")]
                            Meta::NameValue(nv) => {
                                if nv.path.is_ident("pattern") {
                                    if let Expr::Lit(ExprLit {
                                        lit: Lit::Str(s), ..
                                    }) = nv.value
                                    {
                                        validation_schema
                                            .insert("pattern".to_string(), json!(s.value()));
                                    }
                                }
                            }
                            _ => {}
                        }
                    }
                }
            }
        }
    }
    Value::Object(validation_schema)
}
