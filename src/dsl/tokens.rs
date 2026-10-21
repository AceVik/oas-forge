//! Whitespace tokenizer that keeps quoted text and brackets together.

/// Splits at whitespace outside double quotes and brackets. Quotes are kept,
/// so callers can tell `"description"` tokens apart; `example="a b"` and
/// `enum=[a, b]` stay one token each. Brackets in a `pattern=` value are
/// regular expression syntax and do not group. Inside quotes a backslash
/// escapes the next character (`\"` does not end the quote); the escapes
/// are kept in the token.
pub(super) fn tokenize(text: &str) -> Vec<String> {
    let mut tokens = Vec::new();
    let mut current = String::new();
    let mut quoted = false;
    let mut escaped = false;
    let mut depth = 0usize;
    for c in text.chars() {
        // A regular expression ends at whitespace; its brackets need not be
        // balanced (`pattern=[^(]+`).
        let literal = depth == 0 && current.starts_with("pattern=");
        if std::mem::take(&mut escaped) {
            current.push(c);
            continue;
        }
        match c {
            '\\' if quoted => {
                escaped = true;
                current.push(c);
            }
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

/// `"value"` without its quotes, escapes kept (for values such as
/// `pattern="…"`, where a backslash belongs to the value).
pub(super) fn unquote(token: &str) -> Option<&str> {
    token.strip_prefix('"')?.strip_suffix('"')
}

/// `"text"` as text: without its quotes, with `\"` and `\\` unescaped.
pub(super) fn quoted_text(token: &str) -> Option<String> {
    unquote(token).map(unescape)
}

/// `text` with `\"` and `\\` replaced by `"` and `\`; other backslashes
/// stay.
pub(super) fn unescape(text: &str) -> String {
    let mut unescaped = String::with_capacity(text.len());
    let mut chars = text.chars();
    while let Some(c) = chars.next() {
        if c != '\\' {
            unescaped.push(c);
            continue;
        }
        match chars.next() {
            Some(next @ ('"' | '\\')) => unescaped.push(next),
            next => {
                unescaped.push(c);
                unescaped.extend(next);
            }
        }
    }
    unescaped
}

/// Splits `Type "Description"` into the text before the trailing quoted
/// description and the description (unescaped, see [`unescape`]). Quotes
/// are read from the start, so a quoted value before the description and
/// escaped quotes inside it do not end it early.
pub(super) fn trailing_description(text: &str) -> (&str, Option<String>) {
    let text = text.trim();
    let Some(body) = text.strip_suffix('"') else {
        return (text, None);
    };
    // Where the quote that the final `"` closes opens.
    let mut open = None;
    let mut escaped = false;
    for (index, c) in body.char_indices() {
        if std::mem::take(&mut escaped) {
            continue;
        }
        match c {
            '\\' if open.is_some() => escaped = true,
            '"' => open = if open.is_some() { None } else { Some(index) },
            _ => {}
        }
    }
    let quoted = open
        .filter(|_| !escaped)
        .and_then(|start| Some((body.get(..start)?, body.get(start + 1..)?)));
    match quoted {
        Some((before, description)) => (before.trim(), Some(unescape(description))),
        // A final quote without opening quote is dropped.
        None => (body.trim(), None),
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
