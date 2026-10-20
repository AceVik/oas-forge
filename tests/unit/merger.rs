use crate::common::snippet;
use oas_forge::error::Error;
use oas_forge::merger::merge_openapi;
use oas_forge::scanner::Snippet;
use serde_yaml_ng::Value;
use std::path::PathBuf;

fn merge(contents: &[&str]) -> Value {
    merge_openapi(contents.iter().map(|c| snippet(c)).collect()).expect("merge should succeed")
}

fn strings(value: &Value) -> Vec<&str> {
    value
        .as_sequence()
        .expect("sequence")
        .iter()
        .map(|v| v.as_str().expect("string item"))
        .collect()
}

const ROOT: &str = "openapi: 3.1.0\ninfo:\n  title: Root\n  version: '1'";

#[test]
fn empty_input_yields_empty_mapping() {
    let merged = merge(&[]);
    assert_eq!(merged, Value::Mapping(Default::default()));
}

#[test]
fn root_is_used_as_base_for_fragments() {
    let merged = merge(&[
        ROOT,
        "paths:\n  /bar:\n    post:\n      description: fragment",
    ]);

    assert_eq!(merged["openapi"], "3.1.0");
    assert_eq!(merged["info"]["title"], "Root");
    assert_eq!(merged["paths"]["/bar"]["post"]["description"], "fragment");
}

#[test]
fn root_is_detected_regardless_of_snippet_order() {
    let merged = merge(&["paths:\n  /a: {}", ROOT, "paths:\n  /b: {}"]);

    assert_eq!(merged["info"]["title"], "Root");
    assert!(merged["paths"].get("/a").is_some());
    assert!(merged["paths"].get("/b").is_some());
}

#[test]
fn fragments_override_root_scalars_even_when_listed_first() {
    // Non-root snippets are always merged on top of the root.
    let merged = merge(&["info:\n  title: Fragment", ROOT]);
    assert_eq!(merged["info"]["title"], "Fragment");
    assert_eq!(merged["info"]["version"], "1");
}

#[test]
fn without_root_fragments_merge_into_empty_document() {
    let merged = merge(&["paths: {}"]);

    assert!(merged.get("paths").is_some());
    assert!(merged.get("openapi").is_none());
    assert!(merged.get("info").is_none());
}

#[test]
fn a_document_needs_both_openapi_and_info_to_be_root() {
    // Two snippets with only one of the root keys each are plain fragments,
    // so they must not trigger the multiple-roots check.
    let merged = merge(&["openapi: 3.1.0", "info:\n  title: Only Info"]);
    assert_eq!(merged["openapi"], "3.1.0");
    assert_eq!(merged["info"]["title"], "Only Info");
}

#[test]
fn multiple_roots_are_rejected() {
    let result = merge_openapi(vec![
        snippet("openapi: 3.0.0\ninfo: {title: A}"),
        snippet("openapi: 3.0.0\ninfo: {title: B}"),
    ]);
    assert!(matches!(result, Err(Error::MultipleRootsFound)));
}

#[test]
fn nested_mappings_are_merged_recursively() {
    let merged = merge(&[
        "components:\n  schemas:\n    User:\n      type: object\n      properties:\n        id: {type: integer}",
        "components:\n  schemas:\n    User:\n      properties:\n        username: {type: string}",
    ]);

    let user = &merged["components"]["schemas"]["User"];
    assert_eq!(user["type"], "object");
    assert_eq!(user["properties"]["id"]["type"], "integer");
    assert_eq!(user["properties"]["username"]["type"], "string");
}

#[test]
fn sequences_are_concatenated() {
    let merged = merge(&["required: [id]", "required: [username]"]);
    assert_eq!(strings(&merged["required"]), ["id", "username"]);
}

#[test]
fn sequences_are_deduplicated_preserving_first_occurrence() {
    let merged = merge(&[ROOT, "tags: [A, B]", "tags: [B, C, A]"]);
    assert_eq!(strings(&merged["tags"]), ["A", "B", "C"]);
}

#[test]
fn conflicting_scalars_are_won_by_the_last_snippet() {
    let merged = merge(&["info:\n  title: Title 1", "info:\n  title: Title 2"]);
    assert_eq!(merged["info"]["title"], "Title 2");
}

#[test]
fn type_mismatch_is_won_by_the_last_snippet() {
    let merged = merge(&["servers: [a]", "servers:\n  url: b"]);
    assert_eq!(merged["servers"]["url"], "b");
}

#[test]
fn empty_documents_are_skipped() {
    // Regression: an empty `/// @openapi` block parsed to null and replaced the
    // whole merged document, silently dropping everything merged before it.
    let merged = merge(&[
        ROOT,
        "paths:\n  /a: {}",
        "",
        "# only a comment",
        "~",
        "  \n",
    ]);
    assert_eq!(merged["info"]["title"], "Root");
    assert!(merged["paths"]["/a"].is_mapping());
}

#[test]
fn non_mapping_documents_are_rejected_with_their_location() {
    // Regression: a non-mapping snippet used to replace the whole merged document.
    for (content, kind) in [
        ("Health check", "a string"),
        ("- just\n- a list", "a sequence"),
        ("42", "a number"),
        ("true", "a boolean"),
        ("!tag value", "a tagged value"),
    ] {
        let stray = Snippet {
            content: content.to_string(),
            file_path: PathBuf::from("src/api.rs"),
            line_number: 7,
            operation_id: None,
        };

        match merge_openapi(vec![snippet(ROOT), stray]) {
            Err(Error::SourceMapped {
                file,
                line,
                source,
                context,
            }) => {
                assert_eq!(file, PathBuf::from("src/api.rs"));
                assert_eq!(line, 7);
                assert_eq!(
                    source.to_string(),
                    format!("an OpenAPI snippet must be a mapping at the top level, found {kind}")
                );
                assert!(context.starts_with("    07 | "), "{context}");
            }
            other => panic!("expected SourceMapped error for {content:?}, got {other:?}"),
        }
    }
}

#[test]
fn invalid_yaml_reports_source_location_and_context() {
    let bad = Snippet {
        content: "line one\ninvalid: : yaml\nthree\nfour\nfive\nsix".to_string(),
        file_path: PathBuf::from("src/bad.rs"),
        line_number: 10,
        operation_id: None,
    };

    match merge_openapi(vec![bad]) {
        Err(Error::SourceMapped {
            file,
            line,
            context,
            ..
        }) => {
            assert_eq!(file, PathBuf::from("src/bad.rs"));
            assert_eq!(line, 10);
            assert_eq!(
                context,
                "    10 | line one\n    11 | invalid: : yaml\n    12 | three\n    13 | four\n    14 | five",
                "context shows the first five lines numbered from the snippet start"
            );
        }
        other => panic!("expected SourceMapped error, got {other:?}"),
    }
}

#[test]
fn invalid_yaml_aborts_even_after_valid_snippets() {
    let result = merge_openapi(vec![snippet(ROOT), snippet("a: : b")]);
    assert!(matches!(result, Err(Error::SourceMapped { .. })));
}
