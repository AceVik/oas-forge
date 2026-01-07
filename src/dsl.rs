use crate::type_mapper::map_syn_type_to_openapi;
use crate::visitor::json_merge;
use regex::Regex;
use serde_json::{Value, json};
use std::collections::HashSet;
use syn;

/// Parses a block of doc comments (lines) into an OpenAPI PathItem (YAML/JSON).
/// Returns Some(yaml_string) if a @route is detected, otherwise None.
pub fn parse_route_dsl(doc_lines: &[String], operation_id: &str) -> Option<String> {
    // 1. Check if it's a route
    // (Optimization: peek first)
    if !doc_lines.iter().any(|l| l.trim().starts_with("@route")) {
        return None;
    }

    let mut operation = json!({
        "summary": Value::Null,
        "description": Value::Null,
        "operationId": operation_id,
        "tags": [],
        "parameters": [],
        "responses": {}
    });

    let mut method = String::new();
    let mut path = String::new();
    let mut description_buffer = Vec::new();
    let mut dsl_override_buffer = Vec::new();
    let mut collecting_openapi = false;
    let mut summary: Option<String> = None;
    let mut declared_path_params = HashSet::new();

    // Regex for inline path parameters: {name: Type "Desc"}
    let re = Regex::new(r#"\{(\w+)(?::\s*([^"}]+))?(?:\s*"([^"]+)")?\}"#).unwrap();

    for line in doc_lines {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }

        if trimmed.starts_with('@') {
            collecting_openapi = false;
        }

        if trimmed.starts_with("@route") {
            let parts: Vec<&str> = trimmed.split_whitespace().collect();
            if parts.len() >= 3 {
                method = parts[1].to_lowercase();
                let raw_path = parts[2..].join(" ");

                // Route Parser Logic (from visitor.rs)
                let mut new_path = String::new();
                let mut last_end = 0;

                for cap in re.captures_iter(&raw_path) {
                    let full_match = cap.get(0).unwrap();
                    let name = cap.get(1).unwrap().as_str();
                    let type_str = cap.get(2).map(|m| m.as_str().trim());
                    let desc = cap.get(3).map(|m| m.as_str().to_string());

                    new_path.push_str(&raw_path[last_end..full_match.start()]);
                    new_path.push('{');
                    new_path.push_str(name);
                    new_path.push('}');
                    last_end = full_match.end();

                    let is_bare = type_str.is_none() && desc.is_none();

                    if !is_bare {
                        declared_path_params.insert(name.to_string());
                        let t = type_str.unwrap_or("String");
                        let (schema, _) = if let Ok(ty) = syn::parse_str::<syn::Type>(t) {
                            map_syn_type_to_openapi(&ty)
                        } else {
                            (json!({ "type": "string" }), true)
                        };

                        let mut param_obj = json!({
                            "name": name,
                            "in": "path",
                            "required": true,
                            "schema": schema
                        });

                        if let Some(d) = desc {
                            if let Value::Object(m) = &mut param_obj {
                                m.insert("description".to_string(), json!(d));
                            }
                        }

                        if let Value::Array(params) = operation.get_mut("parameters").unwrap() {
                            params.push(param_obj);
                        }
                    }
                }
                new_path.push_str(&raw_path[last_end..]);
                path = new_path;
            }
        } else if trimmed.starts_with("@tag") {
            let tags: Vec<String> = trimmed
                .strip_prefix("@tag")
                .unwrap()
                .split(',')
                .map(|s| s.trim().to_string())
                .filter(|s| !s.is_empty())
                .collect();
            operation["tags"] = json!(tags);
        } else if trimmed.starts_with("@path-param")
            || trimmed.starts_with("@query-param")
            || trimmed.starts_with("@header-param")
            || trimmed.starts_with("@cookie-param")
        {
            // Param Logic
            let (param_type, rest) = if trimmed.starts_with("@path-param") {
                ("path", trimmed.strip_prefix("@path-param").unwrap())
            } else if trimmed.starts_with("@query-param") {
                ("query", trimmed.strip_prefix("@query-param").unwrap())
            } else if trimmed.starts_with("@header-param") {
                ("header", trimmed.strip_prefix("@header-param").unwrap())
            } else {
                ("cookie", trimmed.strip_prefix("@cookie-param").unwrap())
            };

            let rest = rest.trim();
            if let Some(colon_idx) = rest.find(':') {
                let name = rest[..colon_idx].trim();
                let type_part = rest[colon_idx + 1..].trim();

                let tokens_vec: Vec<&str> = type_part.split_whitespace().collect();
                let first = tokens_vec.first().copied().unwrap_or("");

                let (type_def, start_idx) = if first == "deprecated"
                    || first == "required"
                    || first.starts_with("example=")
                    || first.starts_with('"')
                {
                    ("String", 0)
                } else if !tokens_vec.is_empty() {
                    (first, 1)
                } else {
                    ("String", 0)
                };

                let (schema, mut is_required) =
                    if let Ok(ty) = syn::parse_str::<syn::Type>(type_def) {
                        map_syn_type_to_openapi(&ty)
                    } else {
                        (json!({ "type": "string" }), true)
                    };

                let mut deprecated = false;
                let mut example = None;
                let mut desc = None;

                let mut desc_tokens = Vec::new();
                let mut in_desc = false;

                // Attributes check in tokens
                for token in tokens_vec.iter().skip(start_idx) {
                    if in_desc {
                        desc_tokens.push(*token);
                        continue;
                    }

                    if *token == "deprecated" {
                        deprecated = true;
                    } else if *token == "required" {
                        is_required = true;
                    } else if token.starts_with("example=") {
                        let val = token.strip_prefix("example=").unwrap().trim_matches('"');
                        example = Some(val.to_string());
                    } else if token.starts_with('"') {
                        in_desc = true;
                        desc_tokens.push(*token);
                    }
                }

                if !desc_tokens.is_empty() {
                    desc = Some(desc_tokens.join(" ").trim_matches('"').to_string());
                }

                let mut param_obj = json!({
                    "name": name,
                    "in": param_type,
                    "required": is_required,
                    "schema": schema
                });

                if deprecated {
                    param_obj
                        .as_object_mut()
                        .unwrap()
                        .insert("deprecated".to_string(), json!(true));
                }
                if let Some(ex) = example {
                    param_obj
                        .as_object_mut()
                        .unwrap()
                        .insert("example".to_string(), json!(ex));
                }
                if param_type == "path" {
                    declared_path_params.insert(name.to_string());
                    param_obj
                        .as_object_mut()
                        .unwrap()
                        .insert("required".to_string(), json!(true));
                }
                if let Some(d) = desc {
                    param_obj
                        .as_object_mut()
                        .unwrap()
                        .insert("description".to_string(), json!(d));
                }

                if let Value::Array(params) = operation.get_mut("parameters").unwrap() {
                    params.push(param_obj);
                }
            }
        } else if trimmed.starts_with("@body") {
            // ... Body Logic (Ported) ...
            let rest = trimmed.strip_prefix("@body").unwrap().trim();
            let parts: Vec<&str> = rest.split_whitespace().collect();
            if !parts.is_empty() {
                let schema_ref = parts[0];
                let mime = if parts.len() > 1 {
                    parts[1]
                } else {
                    "application/json"
                };

                let is_std_generic = schema_ref.starts_with("Option<")
                    || schema_ref.starts_with("Vec<")
                    || schema_ref.starts_with("Box<")
                    || schema_ref.starts_with("Arc<")
                    || schema_ref.starts_with("Rc<")
                    || schema_ref.starts_with("Cow<");

                let schema = if !is_std_generic
                    && (schema_ref.contains('<')
                        || (schema_ref.starts_with('$') && schema_ref.contains('<')))
                {
                    json!({ "$ref": schema_ref })
                } else if let Ok(ty) = syn::parse_str::<syn::Type>(schema_ref) {
                    map_syn_type_to_openapi(&ty).0
                } else if let Some(stripped) = schema_ref.strip_prefix('$') {
                    json!({ "$ref": format!("#/components/schemas/{}", stripped) })
                } else {
                    json!({ "$ref": format!("#/components/schemas/{}", schema_ref) })
                };

                operation["requestBody"] = json!({
                    "content": { mime: { "schema": schema } }
                });
            }
        } else if trimmed.starts_with("@return") {
            // ... Return Logic (Ported) ...
            let rest = trimmed.strip_prefix("@return").unwrap().trim();
            if let Some(colon_idx) = rest.find(':') {
                let code = rest[..colon_idx].trim();
                let residue = rest[colon_idx + 1..].trim();

                let (type_str, desc, is_unit) = if residue.starts_with('"') {
                    ("()", Some(residue.trim_matches('"').to_string()), true)
                } else if let Some(quote_start) = residue.find('"') {
                    (
                        residue[..quote_start].trim(),
                        Some(residue[quote_start + 1..residue.len() - 1].to_string()),
                        false,
                    )
                } else {
                    (residue, None, false)
                };

                let effective_unit = is_unit || type_str == "()" || type_str == "unit";
                let is_std_generic = type_str.starts_with("Option<")
                    || type_str.starts_with("Vec<")
                    || type_str.starts_with("Box<")
                    || type_str.starts_with("Arc<")
                    || type_str.starts_with("Rc<")
                    || type_str.starts_with("Cow<");

                let schema = if effective_unit {
                    json!({})
                } else if !is_std_generic
                    && (type_str.contains('<')
                        || (type_str.starts_with('$') && type_str.contains('<')))
                {
                    json!({ "$ref": type_str })
                } else if let Ok(ty) = syn::parse_str::<syn::Type>(type_str) {
                    map_syn_type_to_openapi(&ty).0
                } else if let Some(stripped) = type_str.strip_prefix('$') {
                    json!({ "$ref": format!("#/components/schemas/{}", stripped) })
                } else if type_str == "String" || type_str == "str" {
                    json!({ "type": "string" })
                } else {
                    json!({ "$ref": format!("#/components/schemas/{}", type_str) })
                };

                let mut resp_obj = json!({ "description": desc.unwrap_or_default() });
                if !effective_unit {
                    resp_obj["content"] = json!({ "application/json": { "schema": schema } });
                }

                if let Value::Object(responses) = operation.get_mut("responses").unwrap() {
                    responses.insert(code.to_string(), resp_obj);
                }
            }
        } else if trimmed.starts_with("@security") {
            // ... Security Logic ...
            let rest = trimmed.strip_prefix("@security").unwrap().trim();
            let (scheme, scopes) = if let Some(paren_start) = rest.find('(') {
                let name = rest[..paren_start].trim();
                let inner = &rest[paren_start + 1..rest.len() - 1];
                let s: Vec<String> = inner
                    .split(',')
                    .map(|s| s.trim().trim_matches('"').to_string())
                    .collect();
                (name, s)
            } else {
                (rest, vec![])
            };

            if operation.get("security").is_none() {
                operation["security"] = json!([]);
            }
            if let Value::Array(sec) = operation.get_mut("security").unwrap() {
                sec.push(json!({ scheme: scopes }));
            }
        } else if !trimmed.starts_with('@') {
            // Override Logic
            let is_yaml_key = trimmed.starts_with("parameters:")
                || trimmed.starts_with("requestBody:")
                || trimmed.starts_with("responses:")
                || trimmed.starts_with("security:")
                || trimmed.starts_with("externalDocs:")
                || trimmed.starts_with("callbacks:")
                || trimmed.starts_with("servers:");

            if is_yaml_key {
                collecting_openapi = true;
            }

            if collecting_openapi {
                dsl_override_buffer.push(line.to_string());
            } else if summary.is_none() {
                summary = Some(trimmed.to_string());
            } else {
                description_buffer.push(line.to_string());
            }
        }
    }

    if let Some(s) = summary {
        operation["summary"] = json!(s);
    }
    if !description_buffer.is_empty() {
        // Calculate min indentation (ignoring empty lines)
        let min_indent = description_buffer
            .iter()
            .filter(|l| !l.trim().is_empty())
            .map(|l| l.len() - l.trim_start().len())
            .min()
            .unwrap_or(0);

        let cleaned_desc: Vec<String> = description_buffer
            .iter()
            .map(|l| {
                if l.len() >= min_indent {
                    l[min_indent..].to_string()
                } else {
                    l.clone()
                }
            })
            .collect();

        operation["description"] = json!(cleaned_desc.join("\n"));
    }

    // Merge Overrides
    if !dsl_override_buffer.is_empty() {
        let override_yaml = dsl_override_buffer.join("\n");
        if let Ok(val) = serde_yaml::from_str::<Value>(&override_yaml) {
            if !val.is_null() {
                json_merge(&mut operation, val);
            }
        }
    }

    // Validation (Path Params)
    let validation_re = Regex::new(r"\{(\w+)\}").unwrap();
    if !method.is_empty() && !path.is_empty() {
        // ... (Checking path params matches declared)
        for cap in validation_re.captures_iter(&path) {
            let var = cap.get(1).unwrap().as_str();
            if !declared_path_params.contains(var) {
                // Return error or panic? Visitor panicked.
                // We should probably panic to maintain behavior or return Result.
                // Panic for now.
                panic!(
                    "Missing definition for path parameter '{}' in route '{}'",
                    var, path
                );
            }
        }
        for declared in declared_path_params {
            if !path.contains(&format!("{{{}}}", declared)) {
                panic!(
                    "Declared path parameter '{}' is unused in route '{}'",
                    declared, path
                );
            }
        }

        // Clean nulls
        if let Value::Object(map) = &mut operation {
            map.retain(|_, v| !v.is_null());
        }

        let mut method_map = serde_json::Map::new();
        method_map.insert(method, operation);
        let mut path_map = serde_json::Map::new();
        path_map.insert(path, Value::Object(method_map));

        let path_item = json!({ "paths": Value::Object(path_map) });

        if let Ok(generated) = serde_yaml::to_string(&path_item) {
            return Some(generated.trim_start_matches("---\n").to_string());
        }
    }

    None
}
