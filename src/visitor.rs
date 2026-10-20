use serde_json::{Value, json};
use syn::spanned::Spanned;
use syn::visit::{self, Visit};
use syn::{Attribute, Expr, File, ImplItemFn, ItemEnum, ItemFn, ItemMod, ItemStruct, ItemType};

/// Extracted item type
#[derive(Debug)]
pub enum ExtractedItem {
    /// Standard @openapi body
    Schema {
        name: Option<String>,
        content: String,
        line: usize,
    },
    /// @openapi-fragment Name(args...)
    Fragment {
        name: String,
        params: Vec<String>,
        content: String,
        line: usize,
    },
    /// @openapi<T, U>
    Blueprint {
        name: String,
        params: Vec<String>,
        content: String,
        line: usize,
    },
    // Raw DSL block (for late binding)
    RouteDSL {
        content: String,
        line: usize,
        operation_id: String,
    },
}

#[derive(Default)]
pub struct OpenApiVisitor {
    pub items: Vec<ExtractedItem>,
    pub current_tags: Vec<String>,
}

impl OpenApiVisitor {
    // Process doc attributes on items (structs, fns, types)
    // Updated: No longer accepts generated_content. Strictly for @openapi blocks (Paths/Fragments).
    fn check_attributes(
        &mut self,
        attrs: &[Attribute],
        item_ident: Option<String>,
        item_line: usize,
    ) {
        let doc_lines = crate::doc_parser::extract_doc_comments(attrs);

        let has_openapi = doc_lines.iter().any(|l| l.contains("@openapi"));

        // Safety: Only process if explicit @openapi tag exists
        if !has_openapi {
            return;
        }

        let full_doc = doc_lines.join("\n");
        self.parse_doc_block(&full_doc, item_ident, item_line);
    }

    fn parse_doc_block(&mut self, doc: &str, item_ident: Option<String>, line: usize) {
        let lines: Vec<&str> = doc.lines().collect();
        // Naive unindent
        let min_indent = lines
            .iter()
            .filter(|line| !line.trim().is_empty())
            .map(|line| line.chars().take_while(|c| *c == ' ').count())
            .min()
            .unwrap_or(0);

        let unindented: Vec<String> = lines
            .into_iter()
            .map(|l| {
                if l.len() >= min_indent {
                    l[min_indent..].to_string()
                } else {
                    l.to_string()
                }
            })
            .collect();
        let content = unindented.join("\n");

        let mut sections = Vec::new();
        let mut current_header = String::new();
        let mut current_body = Vec::new();

        // Lines before the first header are plain documentation of the Rust item
        // and are dropped: emitted on their own they would not be a YAML mapping.
        for line in content.lines() {
            let trimmed = line.trim();
            if trimmed.starts_with("@openapi") {
                if !current_header.is_empty() {
                    sections.push((current_header.clone(), current_body.join("\n")));
                }
                current_header = trimmed.to_string();
                current_body.clear();
            } else if trimmed.starts_with('{') && current_header.is_empty() {
                current_header = "@json".to_string();
                current_body.clear();
                current_body.push(line.to_string());
            } else {
                current_body.push(line.to_string());
            }
        }
        if !current_header.is_empty() {
            sections.push((current_header, current_body.join("\n")));
        }

        for (header, body) in sections {
            let mut body_content = body.trim().to_string();

            if header.starts_with("@openapi-fragment") {
                let rest = header.strip_prefix("@openapi-fragment").unwrap().trim();
                let (name, params) = if let Some(idx) = rest.find('(') {
                    let name = rest[..idx].trim().to_string();
                    let params_str = rest[idx + 1..].trim_end_matches(')');
                    let params: Vec<String> = params_str
                        .split(',')
                        .map(|p| p.trim().to_string())
                        .filter(|p| !p.is_empty())
                        .collect();
                    (name, params)
                } else {
                    (rest.to_string(), Vec::new())
                };

                self.items.push(ExtractedItem::Fragment {
                    name,
                    params,
                    content: body_content,
                    line,
                });
            } else if header.starts_with("@openapi-type") {
                let name = header
                    .strip_prefix("@openapi-type")
                    .unwrap()
                    .trim()
                    .to_string();
                // Wrap content in schema definition
                let wrapped = wrap_in_schema(&name, &body_content);
                self.items.push(ExtractedItem::Schema {
                    name: Some(name),
                    content: wrapped,
                    line,
                });
            } else if header.starts_with("@openapi") && header.contains('<') {
                if let Some(start) = header.find('<') {
                    if let Some(end) = header.rfind('>') {
                        let params_str = &header[start + 1..end];
                        let params: Vec<String> = params_str
                            .split(',')
                            .map(|p| p.trim().to_string())
                            .filter(|p| !p.is_empty())
                            .collect();

                        if let Some(ident) = &item_ident {
                            self.items.push(ExtractedItem::Blueprint {
                                name: ident.clone(),
                                params,
                                content: body_content,
                                line,
                            });
                        }
                    }
                }
            } else if (header.starts_with("@openapi") && !header.contains('<')) || header == "@json"
            {
                // TAG INJECTION
                if !self.current_tags.is_empty() {
                    let tags_yaml_list = self
                        .current_tags
                        .iter()
                        .map(|t| format!("- {}", t))
                        .collect::<Vec<_>>();

                    let verbs = [
                        "get:", "post:", "put:", "delete:", "patch:", "head:", "options:", "trace:",
                    ];
                    let mut new_lines = Vec::new();
                    let mut injected_any = false;

                    for line in body_content.lines() {
                        new_lines.push(line.to_string());
                        let trimmed = line.trim();
                        if verbs.contains(&trimmed) {
                            let indent = line.chars().take_while(|c| *c == ' ').count();
                            let child_indent = " ".repeat(indent + 2);

                            if !body_content.contains("tags:") {
                                new_lines.push(format!("{}tags:", child_indent));
                                for tag in &tags_yaml_list {
                                    new_lines.push(format!("{}  {}", child_indent, tag));
                                }
                                injected_any = true;
                            }
                        }
                    }

                    if injected_any {
                        body_content = new_lines.join("\n");
                    }
                }

                // Auto-Wrap Heuristic (Only for manual blocks now)
                let starts_with_toplevel = body_content.lines().any(|line| {
                    let trimmed = line.trim();
                    if trimmed.starts_with("#") {
                        return false;
                    }
                    if let Some(key) = trimmed.split(':').next() {
                        matches!(
                            key.trim(),
                            "openapi"
                                | "info"
                                | "paths"
                                | "components"
                                | "tags"
                                | "servers"
                                | "security"
                        )
                    } else {
                        false
                    }
                });

                let final_content = if !starts_with_toplevel && !body_content.trim().is_empty() {
                    if let Some(n) = &item_ident {
                        wrap_in_schema(n, &body_content)
                    } else {
                        body_content
                    }
                } else {
                    body_content
                };

                self.items.push(ExtractedItem::Schema {
                    name: item_ident.clone(),
                    content: final_content,
                    line,
                });
            }
        }
    }
    // Helper to process a single struct field
    fn process_struct_field(
        field: &syn::Field,
        rename_rule: &Option<String>,
    ) -> (String, Value, bool) {
        let default_field_name = field.ident.as_ref().unwrap().to_string();

        // Extract field info
        let (mut field_final_name, field_desc, _, field_doc_lines, _, _) =
            crate::doc_parser::extract_naming_and_doc(&field.attrs, &default_field_name);

        // Apply Rename Rule
        // Only apply if the name hasn't been explicitly renamed via attributes
        // AND there is a rename rule present.
        if field_final_name == default_field_name {
            if let Some(rule) = rename_rule {
                field_final_name = crate::doc_parser::apply_casing(&field_final_name, rule);
            }
        }

        let (mut field_schema, is_required) = map_syn_type_to_openapi(&field.ty);

        // Field Description
        if !field_desc.is_empty() {
            if let Value::Object(map) = &mut field_schema {
                map.insert("description".to_string(), Value::String(field_desc));
            }
        }

        // Validation Attributes
        let validation_props = crate::doc_parser::extract_validation(&field.attrs);
        if !validation_props.as_object().unwrap().is_empty() {
            json_merge(&mut field_schema, validation_props);
        }

        // Field Overrides (@openapi lines)
        let mut field_openapi_lines = Vec::new();
        let mut collecting_openapi = false;
        for line in &field_doc_lines {
            let trimmed = line.trim();
            if trimmed.starts_with("@openapi") {
                collecting_openapi = true;
                let rest = trimmed.strip_prefix("@openapi").unwrap().trim();
                if !rest.is_empty() && !rest.starts_with("rename") {
                    field_openapi_lines.push(rest.to_string());
                }
            } else if collecting_openapi {
                field_openapi_lines.push(line.to_string());
            }
        }

        if !field_openapi_lines.is_empty() {
            let override_yaml = field_openapi_lines.join("\n");
            match serde_yaml_ng::from_str::<Value>(&override_yaml) {
                Ok(override_val) => {
                    if !override_val.is_null() {
                        json_merge(&mut field_schema, override_val);
                    }
                }
                Err(e) => {
                    log::warn!(
                        "Failed to parse @openapi override for field '{}': {}",
                        default_field_name,
                        e
                    );
                }
            }
        }

        (field_final_name, field_schema, is_required)
    }
    fn process_enum_variant(
        variant: &syn::Variant,
        rename_rule: &Option<String>,
    ) -> Option<String> {
        if !matches!(variant.fields, syn::Fields::Unit) {
            return None;
        }
        let default_variant_name = variant.ident.to_string();
        // Extract variant info (renaming only)
        let (mut variant_final_name, _, _, _, _, _) =
            crate::doc_parser::extract_naming_and_doc(&variant.attrs, &default_variant_name);

        // Apply Rename Rule
        if variant_final_name == default_variant_name {
            if let Some(rule) = rename_rule {
                variant_final_name = crate::doc_parser::apply_casing(&variant_final_name, rule);
            }
        }
        Some(variant_final_name)
    }
}

// Helper to wrap content in components/schemas
fn wrap_in_schema(name: &str, content: &str) -> String {
    let indented = content
        .lines()
        .map(|l| format!("      {}", l))
        .collect::<Vec<_>>()
        .join("\n");
    format!("components:\n  schemas:\n    {}:\n{}", name, indented)
}

pub use crate::type_mapper::map_syn_type_to_openapi;

// Deep Merge Helper for JSON Values
pub fn json_merge(a: &mut Value, b: Value) {
    match (a, b) {
        (Value::Object(a), Value::Object(b)) => {
            for (k, v) in b {
                json_merge(a.entry(k).or_insert(Value::Null), v);
            }
        }
        (a, b) => *a = b,
    }
}

impl<'ast> Visit<'ast> for OpenApiVisitor {
    fn visit_file(&mut self, i: &'ast File) {
        // State machine for file-level doc blocks
        let mut current_block_type: Option<String> = None;
        let mut current_block_lines = Vec::new();
        let mut start_line = 1;

        // Process file attributes (inner doc comments)
        for attr in &i.attrs {
            if attr.path().is_ident("doc") {
                if let syn::Meta::NameValue(meta) = &attr.meta {
                    if let Expr::Lit(expr_lit) = &meta.value {
                        if let syn::Lit::Str(lit_str) = &expr_lit.lit {
                            let raw_line = lit_str.value();
                            let trimmed = raw_line.trim();

                            if trimmed.starts_with("@openapi-type") {
                                // Flush previous if exists
                                if !current_block_lines.is_empty() {
                                    let body = current_block_lines.join("\n");
                                    if let Some(name) = current_block_type.take() {
                                        let wrapped = wrap_in_schema(&name, &body);
                                        self.items.push(ExtractedItem::Schema {
                                            name: Some(name),
                                            content: wrapped,
                                            line: start_line,
                                        });
                                    } else {
                                        // Standard Root/Fragment block
                                        self.parse_doc_block(&body, None, start_line);
                                    }
                                    current_block_lines.clear();
                                }

                                // Start New Type
                                if let Some(name) = trimmed.strip_prefix("@openapi-type") {
                                    current_block_type = Some(name.trim().to_string());
                                    start_line = attr.span().start().line;
                                }
                            } else if trimmed.starts_with("@openapi") {
                                // Flush previous
                                if !current_block_lines.is_empty() {
                                    let body = current_block_lines.join("\n");
                                    if let Some(name) = current_block_type.take() {
                                        let wrapped = wrap_in_schema(&name, &body);
                                        self.items.push(ExtractedItem::Schema {
                                            name: Some(name),
                                            content: wrapped,
                                            line: start_line,
                                        });
                                    } else {
                                        self.parse_doc_block(&body, None, start_line);
                                    }
                                    current_block_lines.clear();
                                }

                                // Start Root/Fragment
                                current_block_type = None;
                                start_line = attr.span().start().line;
                                current_block_lines.push(raw_line); // preserve header
                            } else if trimmed.starts_with("@route") {
                                // Flush previous unless it was just general docs
                                if !current_block_lines.is_empty() && current_block_type.is_some() {
                                    // If we were building a type, flush it
                                    let body = current_block_lines.join("\n");
                                    if let Some(name) = current_block_type.take() {
                                        let wrapped = wrap_in_schema(&name, &body);
                                        self.items.push(ExtractedItem::Schema {
                                            name: Some(name),
                                            content: wrapped,
                                            line: start_line,
                                        });
                                    }
                                    current_block_lines.clear();
                                    start_line = attr.span().start().line;
                                } else if current_block_lines.is_empty() {
                                    start_line = attr.span().start().line;
                                }

                                current_block_type = None;
                                current_block_lines.push(raw_line);
                            } else if !current_block_lines.is_empty()
                                || current_block_type.is_some()
                            {
                                current_block_lines.push(raw_line);
                            }
                        }
                    }
                }
            } else {
                // Flush on non-doc attr to be safe
                if !current_block_lines.is_empty() {
                    let body = current_block_lines.join("\n");
                    if let Some(name) = current_block_type.take() {
                        let wrapped = wrap_in_schema(&name, &body);
                        self.items.push(ExtractedItem::Schema {
                            name: Some(name),
                            content: wrapped,
                            line: start_line,
                        });
                    } else {
                        // Check if it's a virtual route
                        if body.contains("@route") {
                            self.items.push(ExtractedItem::RouteDSL {
                                content: body,
                                line: start_line,
                                operation_id: format!("virtual_route_{}", start_line),
                            });
                        } else {
                            self.parse_doc_block(&body, None, start_line);
                        }
                    }
                    current_block_lines.clear();
                }
            }
        }

        // Flush EOF
        if !current_block_lines.is_empty() {
            let body = current_block_lines.join("\n");
            if let Some(name) = current_block_type {
                let wrapped = wrap_in_schema(&name, &body);
                self.items.push(ExtractedItem::Schema {
                    name: Some(name),
                    content: wrapped,
                    line: start_line,
                });
            } else {
                // Check if it's a virtual route
                if body.contains("@route") {
                    self.items.push(ExtractedItem::RouteDSL {
                        content: body,
                        line: start_line,
                        operation_id: format!("virtual_route_{}", start_line),
                    });
                } else {
                    self.parse_doc_block(&body, None, start_line);
                }
            }
        }

        visit::visit_file(self, i);
    }

    fn visit_item_fn(&mut self, i: &'ast ItemFn) {
        let mut doc_lines = Vec::new();
        for attr in &i.attrs {
            if attr.path().is_ident("doc") {
                if let syn::Meta::NameValue(meta) = &attr.meta {
                    if let Expr::Lit(expr_lit) = &meta.value {
                        if let syn::Lit::Str(lit_str) = &expr_lit.lit {
                            doc_lines.push(lit_str.value());
                        }
                    }
                }
            }
        }

        // Check for DSL trigger
        let has_route = doc_lines.iter().any(|l| l.trim().starts_with("@route"));

        if !has_route {
            // Legacy Fallback
            self.check_attributes(&i.attrs, None, i.span().start().line);
            visit::visit_item_fn(self, i);
            return;
        }

        // Emitting Raw DSL for late binding
        let content = doc_lines.join("\n");
        self.items.push(ExtractedItem::RouteDSL {
            content,
            line: i.span().start().line,
            operation_id: i.sig.ident.to_string(),
        });

        visit::visit_item_fn(self, i);
    }

    fn visit_item_type(&mut self, i: &'ast ItemType) {
        let ident = i.ident.to_string();
        let (mut schema, _) = map_syn_type_to_openapi(&i.ty);

        // Docs & Overrides
        let mut desc_lines = Vec::new();
        let mut openapi_lines = Vec::new();
        let mut collecting_openapi = false;

        for attr in &i.attrs {
            if attr.path().is_ident("doc") {
                if let syn::Meta::NameValue(meta) = &attr.meta {
                    if let Expr::Lit(expr_lit) = &meta.value {
                        if let syn::Lit::Str(lit_str) = &expr_lit.lit {
                            let val = lit_str.value();
                            let trimmed = val.trim();

                            if trimmed.starts_with("@openapi") {
                                collecting_openapi = true;
                                let rest = trimmed.strip_prefix("@openapi").unwrap().trim();
                                if !rest.is_empty() {
                                    openapi_lines.push(rest.to_string());
                                }
                            } else if collecting_openapi {
                                openapi_lines.push(val.to_string());
                            } else {
                                desc_lines.push(val.trim().to_string());
                            }
                        }
                    }
                }
            } else {
                collecting_openapi = false;
            }
        }

        if !desc_lines.is_empty() {
            let desc_str = desc_lines.join(" ");
            if let Value::Object(map) = &mut schema {
                map.insert("description".to_string(), Value::String(desc_str));
            }
        }

        if !openapi_lines.is_empty() {
            let override_yaml = openapi_lines.join("\n");
            if let Ok(override_val) = serde_yaml_ng::from_str::<Value>(&override_yaml) {
                if !override_val.is_null() {
                    json_merge(&mut schema, override_val);
                }
            }
        }

        if let Ok(generated) = serde_yaml_ng::to_string(&schema) {
            let trimmed = generated.trim_start_matches("---\n").to_string();
            let wrapped = wrap_in_schema(&ident, &trimmed);
            self.items.push(ExtractedItem::Schema {
                name: Some(ident),
                content: wrapped,
                line: i.span().start().line,
            });
        }

        visit::visit_item_type(self, i);
    }

    fn visit_item_struct(&mut self, i: &'ast ItemStruct) {
        // 1. Extract Info & Renaming
        let default_name = i.ident.to_string();
        let (final_name, struct_desc, rename_rule, doc_lines, _, _) =
            crate::doc_parser::extract_naming_and_doc(&i.attrs, &default_name);

        // Safety: Explicit export only (check raw doc lines for @openapi tag)
        if !doc_lines.iter().any(|l| l.contains("@openapi")) {
            visit::visit_item_struct(self, i);
            return;
        }

        let mut properties = serde_json::Map::new();
        let mut required_fields = Vec::new();
        let mut has_fields = false;

        if let syn::Fields::Named(fields) = &i.fields {
            for field in &fields.named {
                has_fields = true;
                let (field_final_name, field_schema, is_required) =
                    Self::process_struct_field(field, &rename_rule);

                properties.insert(field_final_name.clone(), field_schema);
                if is_required {
                    required_fields.push(field_final_name);
                }
            }
        }

        // Struct Level Schema
        let mut schema = if has_fields {
            let mut s = json!({
                "type": "object",
                "properties": properties
            });
            if !required_fields.is_empty() {
                if let Value::Object(map) = &mut s {
                    map.insert("required".to_string(), json!(required_fields));
                }
            }
            s
        } else {
            // Unit Struct
            json!({ "type": "object" })
        };

        // Struct Description
        if !struct_desc.is_empty() {
            json_merge(&mut schema, json!({ "description": struct_desc }));
        }

        // Struct Overrides & Blueprint
        let mut openapi_lines = Vec::new();
        let mut collecting_openapi = false;
        let mut blueprint_params: Option<Vec<String>> = None;

        for line in &doc_lines {
            let trimmed = line.trim();
            if trimmed.starts_with("@openapi") {
                collecting_openapi = true;
                let rest = trimmed.strip_prefix("@openapi").unwrap().trim();

                if !rest.is_empty() && !rest.starts_with("rename") && !rest.starts_with("-type") {
                    if rest.contains('<') {
                        // Blueprint detection
                        if let Some(start) = rest.find('<') {
                            if let Some(end) = rest.rfind('>') {
                                let params_str = &rest[start + 1..end];
                                blueprint_params = Some(
                                    params_str
                                        .split(',')
                                        .map(|p| p.trim().to_string())
                                        .filter(|p| !p.is_empty())
                                        .collect(),
                                );

                                let after_gt = rest[end + 1..].trim();
                                if !after_gt.is_empty() {
                                    openapi_lines.push(after_gt.to_string());
                                }
                            }
                        }
                    } else {
                        openapi_lines.push(rest.to_string());
                    }
                }
            } else if collecting_openapi {
                openapi_lines.push(line.to_string());
            }
        }

        if !openapi_lines.is_empty() {
            let override_yaml = openapi_lines.join("\n");
            match serde_yaml_ng::from_str::<Value>(&override_yaml) {
                Ok(override_val) => {
                    if !override_val.is_null() {
                        json_merge(&mut schema, override_val);
                    }
                }
                Err(e) => {
                    log::warn!(
                        "Failed to parse @openapi override for struct '{}': {}",
                        final_name,
                        e
                    );
                }
            }
        }

        // Final Serialize
        match serde_yaml_ng::to_string(&schema) {
            Ok(generated) => {
                let trimmed = generated.trim_start_matches("---\n").to_string();

                if let Some(params) = blueprint_params {
                    self.items.push(ExtractedItem::Blueprint {
                        name: final_name,
                        params,
                        content: trimmed,
                        line: i.span().start().line,
                    });
                } else {
                    let wrapped = wrap_in_schema(&final_name, &trimmed);
                    self.items.push(ExtractedItem::Schema {
                        name: Some(final_name),
                        content: wrapped,
                        line: i.span().start().line,
                    });
                }
            }
            Err(e) => {
                log::error!(
                    "Failed to serialize schema for struct '{}': {}",
                    default_name,
                    e
                );
            }
        }

        visit::visit_item_struct(self, i);
    }

    fn visit_item_enum(&mut self, i: &'ast ItemEnum) {
        // 1. Extract Info & Renaming
        let default_name = i.ident.to_string();
        let (final_name, enum_desc, rename_rule, doc_lines, serde_tag, serde_content) =
            crate::doc_parser::extract_naming_and_doc(&i.attrs, &default_name);

        // Safety: Explicit export only
        if !doc_lines.iter().any(|l| l.contains("@openapi")) {
            visit::visit_item_enum(self, i);
            return;
        }

        // ADJACENTLY TAGGED ENUM LOGIC
        if let Some(tag_prop) = serde_tag {
            // As in serde, the enum's `rename_all` only renames variants. Variant
            // fields follow the variant's own `rename_all`, else `rename_all_fields`.
            let rename_all_fields = crate::doc_parser::serde_attr(&i.attrs, "rename_all_fields");

            // This is a "oneOf" container enum
            // 1. Generate Variant Schemas
            let mut variant_refs = Vec::new();
            let mut mapping = serde_json::Map::new();

            for v in &i.variants {
                let default_variant_name = v.ident.to_string();
                let (variant_final_value, variant_desc, variant_rename_all, _, _, _) =
                    crate::doc_parser::extract_naming_and_doc(&v.attrs, &default_variant_name);
                let field_rename_rule = variant_rename_all.or_else(|| rename_all_fields.clone());

                // Apply Rename Rule (to the TAG value, NOT the schema name)
                let tag_value = if variant_final_value == default_variant_name {
                    if let Some(rule) = &rename_rule {
                        crate::doc_parser::apply_casing(&variant_final_value, rule)
                    } else {
                        variant_final_value
                    }
                } else {
                    variant_final_value
                };

                // Variant Schema Name: {EnumName}{VariantName} (PascalCase)
                let variant_schema_name = format!("{}{}", final_name, v.ident);
                let variant_ref = format!("#/components/schemas/{}", variant_schema_name);

                variant_refs.push(json!({ "$ref": variant_ref }));
                mapping.insert(tag_value.clone(), json!(variant_ref));

                // Build Variant Schema
                let mut properties = serde_json::Map::new();
                let mut required = vec![tag_prop.clone()];

                // Force Tag Property
                properties.insert(
                    tag_prop.clone(),
                    json!({
                        "type": "string",
                        "enum": [tag_value],
                        "description": format!("Discriminator: {}", tag_value)
                    }),
                );

                // Variant Fields
                let mut content_schema = None;

                if let syn::Fields::Named(fields) = &v.fields {
                    let mut inner_props = serde_json::Map::new();
                    let mut inner_req = Vec::new();

                    for field in &fields.named {
                        let (f_name, f_schema, f_req) =
                            Self::process_struct_field(field, &field_rename_rule);
                        inner_props.insert(f_name.clone(), f_schema);
                        if f_req {
                            inner_req.push(f_name);
                        }
                    }
                    content_schema = Some(json!({
                        "type": "object",
                        "properties": inner_props,
                        "required": inner_req
                    }));
                } else if let syn::Fields::Unnamed(fields) = &v.fields {
                    // Tuple Variants
                    if fields.unnamed.len() == 1 {
                        let field = &fields.unnamed[0];
                        let (mut schema, _) =
                            crate::type_mapper::map_syn_type_to_openapi(&field.ty);

                        // Apply validation attributes
                        let validation = crate::doc_parser::extract_validation(&field.attrs);
                        if validation.is_object() {
                            crate::visitor::json_merge(&mut schema, validation);
                        }

                        content_schema = Some(schema);
                    } else {
                        log::warn!(
                            "Tuple variants with >1 fields in tagged enums are complex. Skipping fields for {}",
                            default_variant_name
                        );
                    }
                }

                if let Some(content_prop) = &serde_content {
                    // ADJACENTLY TAGGED: { "tag": "...", "content": <Inner> }
                    if let Some(inner) = content_schema {
                        properties.insert(content_prop.clone(), inner);
                        required.push(content_prop.clone());
                    }
                } else {
                    // INTERNALLY TAGGED: { "tag": "...", ...fields }
                    // Only works for Struct variants or Unit variants.
                    // Tuple variants in Internally Tagged are usually invalid or map to something else, but here we merge fields.
                    if let Some(inner) = content_schema {
                        if let Some(props) = inner.get("properties").and_then(|p| p.as_object()) {
                            for (k, v) in props {
                                properties.insert(k.clone(), v.clone());
                            }
                        }
                        if let Some(req) = inner.get("required").and_then(|r| r.as_array()) {
                            for r in req {
                                if let Some(s) = r.as_str() {
                                    required.push(s.to_string());
                                }
                            }
                        }
                    }
                }

                let mut variant_schema = json!({
                    "type": "object",
                    "properties": properties,
                    "required": required
                });

                if !variant_desc.is_empty() {
                    json_merge(&mut variant_schema, json!({ "description": variant_desc }));
                }

                // Emit Variant Schema
                if let Ok(generated) = serde_yaml_ng::to_string(&variant_schema) {
                    let trimmed = generated.trim_start_matches("---\n").to_string();
                    let wrapped = wrap_in_schema(&variant_schema_name, &trimmed);
                    self.items.push(ExtractedItem::Schema {
                        name: Some(variant_schema_name),
                        content: wrapped,
                        line: v.span().start().line,
                    });
                }
            }

            // 2. Generate Main Discriminator Schema
            let mut main_schema = json!({
                "type": "object",
                "oneOf": variant_refs,
                "discriminator": {
                    "propertyName": tag_prop,
                    "mapping": mapping
                }
            });

            if !enum_desc.is_empty() {
                json_merge(&mut main_schema, json!({ "description": enum_desc }));
            }

            // Emit Main Schema
            if let Ok(generated) = serde_yaml_ng::to_string(&main_schema) {
                let trimmed = generated.trim_start_matches("---\n").to_string();
                let wrapped = wrap_in_schema(&final_name, &trimmed);
                self.items.push(ExtractedItem::Schema {
                    name: Some(final_name),
                    content: wrapped,
                    line: i.span().start().line,
                });
            }

            visit::visit_item_enum(self, i);
            return;
        }

        // STANDARD STRING ENUM LOGIC
        let mut variants = Vec::new();
        for v in &i.variants {
            if let Some(variant_name) = Self::process_enum_variant(v, &rename_rule) {
                variants.push(variant_name);
            }
        }

        let mut schema = if !variants.is_empty() {
            json!({
                "type": "string",
                "enum": variants
            })
        } else {
            json!({ "type": "string" }) // fallback
        };

        // Enum Description
        if !enum_desc.is_empty() {
            json_merge(&mut schema, json!({ "description": enum_desc }));
        }

        // Enum Overrides & Blueprint
        let mut openapi_lines = Vec::new();
        let mut collecting_openapi = false;
        let mut blueprint_params: Option<Vec<String>> = None;

        for line in &doc_lines {
            let trimmed = line.trim();
            if trimmed.starts_with("@openapi") {
                collecting_openapi = true;
                let rest = trimmed.strip_prefix("@openapi").unwrap().trim();

                if !rest.is_empty() && !rest.starts_with("rename") && !rest.starts_with("-type") {
                    if rest.contains('<') {
                        // Blueprint detection
                        if let Some(start) = rest.find('<') {
                            if let Some(end) = rest.rfind('>') {
                                let params_str = &rest[start + 1..end];
                                blueprint_params = Some(
                                    params_str
                                        .split(',')
                                        .map(|p| p.trim().to_string())
                                        .filter(|p| !p.is_empty())
                                        .collect(),
                                );

                                let after_gt = rest[end + 1..].trim();
                                if !after_gt.is_empty() {
                                    openapi_lines.push(after_gt.to_string());
                                }
                            }
                        }
                    } else {
                        openapi_lines.push(rest.to_string());
                    }
                }
            } else if collecting_openapi {
                openapi_lines.push(line.to_string());
            }
        }

        if !openapi_lines.is_empty() {
            let override_yaml = openapi_lines.join("\n");
            match serde_yaml_ng::from_str::<Value>(&override_yaml) {
                Ok(override_val) => {
                    if !override_val.is_null() {
                        json_merge(&mut schema, override_val);
                    }
                }
                Err(e) => {
                    log::warn!(
                        "Failed to parse @openapi override for enum '{}': {}",
                        final_name,
                        e
                    );
                }
            }
        }

        // Only emit if we have variants OR overrides
        if !variants.is_empty() || !openapi_lines.is_empty() {
            if let Ok(generated) = serde_yaml_ng::to_string(&schema) {
                let trimmed = generated.trim_start_matches("---\n").to_string();

                if let Some(params) = blueprint_params {
                    self.items.push(ExtractedItem::Blueprint {
                        name: final_name,
                        params,
                        content: trimmed,
                        line: i.span().start().line,
                    });
                } else {
                    let wrapped = wrap_in_schema(&final_name, &trimmed);
                    self.items.push(ExtractedItem::Schema {
                        name: Some(final_name),
                        content: wrapped,
                        line: i.span().start().line,
                    });
                }
            }
        }

        visit::visit_item_enum(self, i);
    }

    fn visit_item_mod(&mut self, i: &'ast ItemMod) {
        let mut found_tags = Vec::new();
        for attr in &i.attrs {
            if attr.path().is_ident("doc") {
                if let syn::Meta::NameValue(meta) = &attr.meta {
                    if let Expr::Lit(expr_lit) = &meta.value {
                        if let syn::Lit::Str(lit_str) = &expr_lit.lit {
                            let val = lit_str.value();
                            if val.contains("tags:") {
                                if let Some(start) = val.find('[') {
                                    // Search for `]` only after `[` so the range is never inverted
                                    let list = &val[start + 1..];
                                    if let Some(end) = list.find(']') {
                                        found_tags.extend(
                                            list[..end]
                                                .split(',')
                                                .map(|t| t.trim().to_string())
                                                .filter(|t| !t.is_empty()),
                                        );
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }

        let old_len = self.current_tags.len();
        self.current_tags.extend(found_tags);

        self.check_attributes(&i.attrs, None, i.span().start().line);
        visit::visit_item_mod(self, i);

        self.current_tags.truncate(old_len);
    }

    fn visit_impl_item_fn(&mut self, i: &'ast ImplItemFn) {
        self.check_attributes(&i.attrs, None, i.span().start().line);
        visit::visit_impl_item_fn(self, i);
    }
}

pub fn extract_from_file(path: std::path::PathBuf) -> crate::error::Result<Vec<ExtractedItem>> {
    let content = std::fs::read_to_string(&path)?;
    let parsed_file = syn::parse_file(&content).map_err(|e| crate::error::Error::Parse {
        file: path.clone(),
        source: e,
    })?;

    let mut visitor = OpenApiVisitor::default();
    visitor.visit_file(&parsed_file);

    Ok(visitor.items)
}
