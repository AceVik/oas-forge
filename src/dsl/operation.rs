//! Operation-level directives: `@security`, `@external-docs` and `@server`.

use super::tokens::{tokenize, trailing_description};
use serde_json::{Map, Value, json};
use std::collections::{BTreeMap, BTreeSet};

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
    // The scopes of each scheme so far: a list of 100 000 scopes that was
    // scanned for each scope took a minute.
    let mut seen: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
    for part in security_parts(rest, '+') {
        let part = part.trim();
        let (scheme, scopes) = match part.split_once('(') {
            Some((scheme, args)) => {
                let inner = args.strip_suffix(')').unwrap_or(args);
                let scopes: Vec<String> = security_parts(inner, ',')
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
        let Value::Array(existing) = requirement
            .entry(scheme.to_string())
            .or_insert_with(|| json!([]))
        else {
            continue;
        };
        let seen = seen.entry(scheme.to_string()).or_default();
        for scope in scopes {
            if seen.insert(scope.clone()) {
                existing.push(json!(scope));
            }
        }
    }
    (!requirement.is_empty()).then_some(Security::Requirement(requirement))
}

/// Splits a security expression outside quoted scopes and parentheses.
fn security_parts(text: &str, separator: char) -> impl Iterator<Item = &str> {
    let mut quoted = false;
    let mut depth = 0_usize;
    text.split(move |c| {
        match c {
            '"' => quoted = !quoted,
            '(' if !quoted => depth += 1,
            ')' if !quoted => depth = depth.saturating_sub(1),
            c if c == separator && !quoted && depth == 0 => return true,
            _ => {}
        }
        false
    })
}

/// `URL ["description"]` of `@external-docs` and `@server`; the URL may be
/// a Markdown autolink (`<https://…>`).
pub(super) fn url_with_description(rest: &str) -> Option<Value> {
    let (url, description) = trailing_description(rest);
    let tokens = tokenize(url);
    let [url] = tokens.as_slice() else {
        return None;
    };
    // A Markdown autolink: `<https://example.com>`.
    let url = url
        .strip_prefix('<')
        .and_then(|u| u.strip_suffix('>'))
        .unwrap_or(url);
    let mut value = Map::new();
    value.insert("url".into(), json!(url));
    if let Some(description) = description {
        value.insert("description".into(), json!(description));
    }
    Some(Value::Object(value))
}
