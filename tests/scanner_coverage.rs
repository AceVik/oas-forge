use oas_forge::index::Registry;
use oas_forge::scanner::{Snippet, preprocess_macros};
use std::path::PathBuf;

#[test]
fn test_macro_vec_expansion() {
    let mut registry = Registry::new();
    let snippet = Snippet {
        content: "tags: $Vec<Tag>".to_string(),
        file_path: PathBuf::from("test.rs"),
        line_number: 1,
        operation_id: None,
    };
    let processed = preprocess_macros(&snippet, &mut registry);
    assert!(processed.content.contains("type: array"));
    assert!(processed.content.contains("items:"));
    assert!(
        processed
            .content
            .contains("$ref: \"#/components/schemas/Tag\"")
    );
}

// Tests for @return expansion removed (Feature disabled to fix bug)

#[test]
fn test_macro_insert_shorthand() {
    let mut registry = Registry::new();
    // Simulate finding a fragment?
    // registry.fragments check is in preprocessor pass 2b?
    // preprocess_macros is Pass 2a.
    // It checks registry to decide if it should expand to Ref or not?
    // Code: if !registry.fragments.contains_key(name) -> emit $ref

    let snippet = Snippet {
        content: "  @insert MissingFrag".to_string(),
        file_path: PathBuf::from("test.rs"),
        line_number: 1,
        operation_id: None,
    };
    let processed = preprocess_macros(&snippet, &mut registry);

    // Expect ref injection because it's missing in registry (assumed param ref)
    assert!(
        processed
            .content
            .contains("$ref: \"#/components/parameters/MissingFrag\"")
    );
}

#[test]
fn test_macro_extend_auto_quote() {
    let mut registry = Registry::new();
    let snippet = Snippet {
        content: "  @extend 'User'".to_string(),
        file_path: PathBuf::from("test.rs"),
        line_number: 1,
        operation_id: None,
    };
    let processed = preprocess_macros(&snippet, &mut registry);

    // Expect x-openapi-extend
    assert!(processed.content.contains("x-openapi-extend: '''User'''"));
}
