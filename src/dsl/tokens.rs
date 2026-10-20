//! Whitespace tokenizer that keeps quoted text and brackets together.

/// Splits at whitespace outside double quotes and brackets. Quotes are kept,
/// so callers can tell `"description"` tokens apart; `example="a b"` and
/// `enum=[a, b]` stay one token each. Brackets in a `pattern=` value are
/// regular expression syntax and do not group.
pub(super) fn tokenize(text: &str) -> Vec<String> {
    let mut tokens = Vec::new();
    let mut current = String::new();
    let mut quoted = false;
    let mut depth = 0usize;
    for c in text.chars() {
        // A regular expression ends at whitespace; its brackets need not be
        // balanced (`pattern=[^(]+`).
        let literal = depth == 0 && current.starts_with("pattern=");
        match c {
            '"' => {
                quoted = !quoted;
                current.push(c);
            }
            '[' | '{' | '(' | '<' if !quoted && !literal => {
                depth += 1;
                current.push(c);
            }
            ']' | '}' | ')' | '>' if !quoted && !literal => {
                depth = depth.saturating_sub(1);
                current.push(c);
            }
            c if c.is_whitespace() && !quoted && depth == 0 => {
                if !current.is_empty() {
                    tokens.push(std::mem::take(&mut current));
                }
            }
            c => current.push(c),
        }
    }
    if !current.is_empty() {
        tokens.push(current);
    }
    tokens
}

/// `"text"` without its quotes.
pub(super) fn unquote(token: &str) -> Option<&str> {
    token.strip_prefix('"')?.strip_suffix('"')
}

/// Splits `Type "Description"` into the text before the trailing quoted
/// description and the description.
pub(super) fn trailing_description(text: &str) -> (&str, Option<String>) {
    let text = text.trim();
    match text.strip_suffix('"') {
        Some(body) => match body.rsplit_once('"') {
            Some((before, description)) => (before.trim(), Some(description.to_string())),
            None => (body.trim(), None),
        },
        None => (text, None),
    }
}

/// Whether a token is a media type such as `application/json` or `text/*`.
pub(super) fn is_media_type(token: &str) -> bool {
    let Some((kind, sub)) = token.split_once('/') else {
        return false;
    };
    let valid = |part: &str| {
        !part.is_empty()
            && part
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || "!#$&^_.+-*;=".contains(c))
    };
    valid(kind) && valid(sub)
}
