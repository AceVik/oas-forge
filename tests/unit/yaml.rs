//! `src/yaml.rs`: the pinned YAML semantics, tested through the pipeline.
//!
//! The YAML library is private; these fixtures fix how YAML text in doc
//! comments and included files is read, whatever library implements it.

use crate::common::{pipeline, try_pipeline};
use oas_forge::error::Error;
use oas_forge::output::Format;
use serde_json::{Value, json};

fn read(text: &str) -> Value {
    pipeline(&[("doc.yaml", text)]).0
}

fn error(text: &str) -> (usize, String) {
    match try_pipeline(&[("doc.yaml", text)]) {
        Err(Error::Yaml {
            location, message, ..
        }) => (location.line(), message),
        other => panic!("expected a YAML error for {text:?}, got {other:?}"),
    }
}

#[test]
fn only_true_and_false_are_booleans() {
    assert_eq!(
        read("x: { a: yes, b: no, c: on, d: Off, e: y, f: true, g: false }")["x"],
        json!({ "a": "yes", "b": "no", "c": "on", "d": "Off", "e": "y", "f": true, "g": false })
    );
}

#[test]
fn numbers_and_null() {
    assert_eq!(
        read(
            "x: { i: 42, n: -7, f: 1.10, e: 1e3, hex: 0x1F, oct: 0o17, big: 18446744073709551615, z: ~, empty: }"
        )["x"],
        json!({ "i": 42, "n": -7, "f": 1.1, "e": 1000.0, "hex": 31, "oct": 15, "big": 18446744073709551615u64, "z": null, "empty": null })
    );
}

#[test]
fn non_finite_floats_become_strings() {
    assert_eq!(
        read("x: [.inf, -.inf, .nan]")["x"],
        json!([".inf", "-.inf", ".nan"])
    );
}

#[test]
fn dates_and_times_stay_strings() {
    assert_eq!(
        read("x: { d: 2024-01-01, t: '12:30:00' }")["x"],
        json!({ "d": "2024-01-01", "t": "12:30:00" })
    );
}

#[test]
fn scalar_keys_become_text() {
    assert_eq!(
        read("responses: { 200: { description: OK }, 4XX: { description: Bad }, default: {} }")["responses"],
        json!({ "200": { "description": "OK" }, "4XX": { "description": "Bad" }, "default": {} })
    );
    assert_eq!(
        read("x: { 1.5: a, true: b }")["x"],
        json!({ "1.5": "a", "true": "b" })
    );
}

#[test]
fn merge_keys_are_expanded_and_local_keys_win() {
    let doc =
        read("base: &b { type: object, description: Base }\nx:\n  <<: *b\n  description: Local");
    assert_eq!(
        doc["x"],
        json!({ "description": "Local", "type": "object" })
    );
}

#[test]
fn anchors_and_aliases_are_resolved() {
    assert_eq!(read("a: &list [1, 2]\nb: *list")["b"], json!([1, 2]));
}

#[test]
fn tags_are_ignored() {
    assert_eq!(
        read("x: { a: !Custom bar, b: !!str 12 }")["x"],
        json!({ "a": "bar", "b": "12" })
    );
}

#[test]
fn block_scalars_keep_their_line_breaks() {
    assert_eq!(read("x: |\n  one\n\n  two\n")["x"], "one\n\ntwo\n");
    assert_eq!(read("x: >-\n  one\n  two")["x"], "one two");
}

#[test]
fn key_order_is_preserved() {
    let keys: Vec<_> = read("x: { z: 1, a: 2, m: 3 }")["x"]
        .as_object()
        .unwrap()
        .keys()
        .cloned()
        .collect();
    assert_eq!(keys, ["z", "a", "m"]);
}

#[test]
fn json_is_yaml() {
    assert_eq!(
        read("{\"x\": [1, 2.5, \"s\", null, true]}")["x"],
        json!([1, 2.5, "s", null, true])
    );
}

#[test]
fn duplicate_keys_are_errors_at_their_line() {
    let (line, message) = error("a: 1\nb: 2\na: 3");
    assert_eq!(line, 3);
    assert_eq!(message, "duplicate mapping key: a");
}

#[test]
fn syntax_errors_point_at_the_offending_line() {
    assert_eq!(error("x:\n  y: 1\n  z: : 2").0, 3);
    assert_eq!(error("x: [unclosed").0, 1);
}

#[test]
fn multiple_documents_are_errors() {
    assert!(
        error("a: 1\n---\nb: 2")
            .1
            .contains("multiple YAML documents")
    );
}

#[test]
fn null_keys_and_complex_keys_are_errors() {
    error("~: x");
    error("? [a, b]\n: x");
}

#[test]
fn alias_bombs_and_deep_nesting_are_rejected() {
    let mut bomb = String::from("a0: &a0 [x, x, x, x, x, x, x, x, x, x]\n");
    for i in 1..8 {
        let refs = vec![format!("*a{}", i - 1); 10].join(", ");
        bomb.push_str(&format!("a{i}: &a{i} [{refs}]\n"));
    }
    error(&bomb);
    error(&format!("x: {}{}", "[".repeat(10_000), "]".repeat(10_000)));
}

#[test]
fn empty_documents_are_nothing() {
    for text in ["", "# only a comment", "~", "---\n"] {
        assert_eq!(read(text), json!({}), "{text:?}");
    }
}

#[test]
fn ambiguous_strings_are_quoted_in_output() {
    let value = json!({
        "yes": "yes", "no": "no", "version": "1.10", "null": "null", "empty": "",
        "comment": "#x", "anchor": "*x", "colon": "a: b", "lead": " x", "date": "2024-01-01",
        "tilde": "~", "tab": "\tx", "multi": "a\nb\n",
    });
    let text = Format::Yaml.render(&value);
    assert_eq!(crate::common::yaml(&text), value, "{text}");
}

#[test]
fn output_has_no_trailing_whitespace_unless_it_is_content() {
    let value = json!({ "a": { "description": "one\n\ntwo" }, "b": "x\n   \ny\n" });
    let text = Format::Yaml.render(&value);
    assert_eq!(crate::common::yaml(&text), value, "{text}");
    let description_block: Vec<_> = text.lines().take_while(|l| !l.starts_with("b:")).collect();
    assert!(
        description_block.iter().all(|l| l.trim_end() == *l),
        "trailing whitespace in:\n{text}"
    );
}
