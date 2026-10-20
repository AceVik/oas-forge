//! Moving component references between documents
//! (`#/components/schemas/X` → `#/$defs/X`).

use serde_json::Value;

/// Replaces the prefix `from` with `to` in every `$ref` and in every
/// `discriminator.mapping` value below `value`.
pub(super) fn rewrite_refs(value: &mut Value, from: &str, to: &str) {
    match value {
        Value::Object(map) => {
            for (key, child) in map.iter_mut() {
                match (key.as_str(), child) {
                    ("$ref", Value::String(target)) => replace_prefix(target, from, to),
                    ("discriminator", Value::Object(discriminator)) => {
                        if let Some(Value::Object(mapping)) = discriminator.get_mut("mapping") {
                            for target in mapping.values_mut() {
                                if let Value::String(target) = target {
                                    replace_prefix(target, from, to);
                                }
                            }
                        }
                    }
                    (_, child) => rewrite_refs(child, from, to),
                }
            }
        }
        Value::Array(items) => items.iter_mut().for_each(|i| rewrite_refs(i, from, to)),
        _ => {}
    }
}

fn replace_prefix(target: &mut String, from: &str, to: &str) {
    if let Some(rest) = target.strip_prefix(from) {
        *target = format!("{to}{rest}");
    }
}
