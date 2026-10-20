//! Operation-level directives: `@security`, `@external-docs` and `@server`.

use super::tokens::{tokenize, trailing_description};
use serde_json::{Map, Value, json};

/// One `@security` line: a requirement (schemes joined with `+` must all be
/// met), or `none` for a public operation (`security: []`).
pub(super) enum Security {
    /// `@security none`
    None,
    /// `@security oauth(read) + api_key`
    Requirement(Map<String, Value>),
}

/// Parses the text after `@security`; `None` when it names no scheme.
pub(super) fn security(rest: &str) -> Option<Security> {
    let rest = rest.trim();
    if rest == "none" {
        return Some(Security::None);
    }
    let mut requirement = Map::new();
    for part in rest.split('+') {
        let part = part.trim();
        let (scheme, scopes) = match part.split_once('(') {
            Some((scheme, args)) => {
                let inner = args.strip_suffix(')').unwrap_or(args);
                let scopes: Vec<String> = inner
                    .split(',')
                    .map(|s| s.trim().trim_matches('"').to_string())
                    .filter(|s| !s.is_empty())
                    .collect();
                (scheme.trim(), scopes)
            }
            None => (part, Vec::new()),
        };
        if scheme.is_empty() {
            continue;
        }
        requirement.insert(scheme.to_string(), json!(scopes));
    }
    (!requirement.is_empty()).then_some(Security::Requirement(requirement))
}

/// `@external-docs URL ["description"]`
pub(super) fn external_docs(rest: &str) -> Option<Value> {
    url_with_description(rest)
}

/// `@server URL ["description"]`
pub(super) fn server(rest: &str) -> Option<Value> {
    url_with_description(rest)
}

fn url_with_description(rest: &str) -> Option<Value> {
    let (url, description) = trailing_description(rest);
    let tokens = tokenize(url);
    let [url] = tokens.as_slice() else {
        return None;
    };
    let mut value = Map::new();
    value.insert("url".into(), json!(url));
    if let Some(description) = description {
        value.insert("description".into(), json!(description));
    }
    Some(Value::Object(value))
}
