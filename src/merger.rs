use crate::error::{Error, Result};
use crate::scanner::Snippet;
use serde_yaml_ng::{Mapping, Value};

/// Merges multiple OpenAPI YAML/JSON fragments into a single Value.
/// Empty snippets are skipped; any other snippet must be a mapping.
pub fn merge_openapi(snippets: Vec<Snippet>) -> Result<Value> {
    let mut root: Option<Value> = None;
    let mut others: Vec<Value> = Vec::new();

    for snippet in snippets {
        let value: Value = match serde_yaml_ng::from_str(&snippet.content) {
            Ok(v) => v,
            Err(e) => return Err(source_mapped(&snippet, e)),
        };

        let map = match value {
            Value::Mapping(map) => map,
            // Empty `@openapi` blocks or files contribute nothing.
            Value::Null => continue,
            // Merging a non-mapping would replace everything merged so far.
            other => {
                let found = match other {
                    Value::Bool(_) => "a boolean",
                    Value::Number(_) => "a number",
                    Value::String(_) => "a string",
                    Value::Sequence(_) => "a sequence",
                    _ => "a tagged value",
                };
                let message = format!(
                    "an OpenAPI snippet must be a mapping at the top level, found {}",
                    found
                );
                return Err(source_mapped(&snippet, serde::de::Error::custom(message)));
            }
        };

        if is_root(&map) {
            if root.is_some() {
                return Err(Error::MultipleRootsFound);
            }
            root = Some(Value::Mapping(map));
        } else {
            others.push(Value::Mapping(map));
        }
    }

    // Relaxed Mode: If no root found, start with empty mapping
    let mut root = root.unwrap_or_else(|| Value::Mapping(Mapping::new()));

    for other in others {
        deep_merge(&mut root, other);
    }

    Ok(root)
}

/// Wraps an error with the snippet's location and its first lines as context.
fn source_mapped(snippet: &Snippet, source: serde_yaml_ng::Error) -> Error {
    let context: String = snippet
        .content
        .lines()
        .take(5)
        .enumerate()
        .map(|(idx, line)| format!("    {:02} | {}", idx + snippet.line_number, line))
        .collect::<Vec<_>>()
        .join("\n");

    Error::SourceMapped {
        file: snippet.file_path.clone(),
        line: snippet.line_number,
        source,
        context,
    }
}

fn is_root(map: &Mapping) -> bool {
    map.contains_key("openapi") && map.contains_key("info")
}

/// Recursive deep merge.
/// - Arrays: Appended.
/// - Maps: Merged recursively.
/// - Scalars: Overwritten by the source (right-hand side).
fn deep_merge(target: &mut Value, source: Value) {
    match (target, source) {
        (Value::Mapping(t_map), Value::Mapping(s_map)) => {
            for (key, s_val) in s_map {
                match t_map.get_mut(&key) {
                    Some(t_val) => deep_merge(t_val, s_val),
                    None => {
                        t_map.insert(key, s_val);
                    }
                }
            }
        }
        (Value::Sequence(t_seq), Value::Sequence(s_seq)) => {
            t_seq.extend(s_seq);
            // Deduplicate preserving order
            let mut seen = std::collections::HashSet::new();
            let mut unique = Vec::new();
            for item in t_seq.drain(..) {
                // We use the string representation for deduping to handle potential Hash/Eq oddities with YAML Values widely
                // But serde_yaml_ng::Value does impl Hash/Eq.
                // However, let's trust serde_yaml_ng's Hash implementation.
                if seen.insert(item.clone()) {
                    unique.push(item);
                }
            }
            *t_seq = unique;
        }
        (t, s) => {
            *t = s;
        }
    }
}
