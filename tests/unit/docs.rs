//! `src/docs.rs`: doc comments as descriptions (through extraction).

use crate::common::{extract, schema};

/// The description of a struct `S` documented with `doc` (lines of `///`).
fn described(doc: &[&str]) -> String {
    let mut source: String = doc.iter().map(|line| format!("///{line}\n")).collect();
    source.push_str("/// @openapi\npub struct S { pub a: u8 }\n");
    schema(&extract(&source), "S")["description"]
        .as_str()
        .unwrap_or_default()
        .to_string()
}

#[test]
fn paragraph_lines_are_joined_with_spaces() {
    assert_eq!(
        described(&[" One", " two.", "", " Three."]),
        "One two.\n\nThree."
    );
}

#[test]
fn headings_and_the_line_after_them_keep_their_breaks() {
    assert_eq!(
        described(&[" Intro", " # Usage", " Call it.", " More."]),
        "Intro\n# Usage\nCall it. More."
    );
    assert_eq!(described(&[" Intro", " #", " text"]), "Intro\n#\ntext");
}

#[test]
fn hashes_without_a_space_or_more_than_six_are_text() {
    assert_eq!(described(&[" Tags", " #rust is fun"]), "Tags #rust is fun");
    assert_eq!(described(&[" A", " ####### b", " c"]), "A ####### b c");
}

#[test]
fn list_items_keep_their_breaks_and_indentation() {
    assert_eq!(
        described(&[" Intro", " - one", "   more", " - two"]),
        "Intro\n- one\n  more\n- two"
    );
    // Indentation shared by every line is not part of the list.
    assert_eq!(
        described(&["   - one", "     more", "   - two"]),
        "- one\n  more\n- two"
    );
}

#[test]
fn numbered_items_use_a_dot_or_a_parenthesis() {
    assert_eq!(
        described(&[" Steps", " 1. a", " 2. b"]),
        "Steps\n1. a\n2. b"
    );
    assert_eq!(
        described(&[" Steps", " 1) a", " 2) b"]),
        "Steps\n1) a\n2) b"
    );
}

#[test]
fn lines_that_only_look_like_items_are_text() {
    assert_eq!(
        described(&[" Released in", " 2024 with fixes."]),
        "Released in 2024 with fixes."
    );
    assert_eq!(described(&[" Ends", " . here"]), "Ends . here");
    assert_eq!(described(&[" Ends", " ) here"]), "Ends ) here");
}

#[test]
fn fenced_code_is_kept_verbatim() {
    assert_eq!(
        described(&[" Example", " ```", " let  x = 1;", "   y", " ```", " Done."]),
        "Example\n```\nlet  x = 1;\n  y\n```\nDone."
    );
}

#[test]
fn single_line_block_comments_keep_their_emphasis() {
    let items = extract("/** *Important* user. */\n/// @openapi\npub struct S { pub a: u8 }\n");
    assert_eq!(schema(&items, "S")["description"], "*Important* user.");
}

#[test]
fn javadoc_columns_are_removed() {
    let items = extract(
        "/**\n * A user.\n * Second line.\n */\n/// @openapi\npub struct S { pub a: u8 }\n",
    );
    assert_eq!(schema(&items, "S")["description"], "A user. Second line.");
}
