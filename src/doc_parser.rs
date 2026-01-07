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
) -> (String, String, Option<String>, Vec<String>) {
    let mut doc_lines = Vec::new();
    // We collect cleaned lines here (without @openapi tags)
    let mut clean_doc_lines = Vec::new();

    let mut final_name = default_name.to_string();
    let mut rename_rule = None;

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
    )
}
