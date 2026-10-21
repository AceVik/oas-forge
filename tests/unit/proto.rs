//! `src/proto.rs`: the index of `.proto` files.

use crate::common::codes;
use oas_forge::diagnostics::{Code, Diagnostics};
use oas_forge::proto::{PROTOBUF_MEDIA_TYPE, ProtoIndex};
use oas_forge::source::{SourceFile, SourceKind};
use std::path::Path;

/// Indexes `files` (`(path, text)`) and returns the index and the findings.
fn index(files: &[(&str, &str)]) -> (ProtoIndex, Diagnostics) {
    let mut index = ProtoIndex::new();
    let mut diagnostics = Diagnostics::new();
    for (path, text) in files {
        index.add(
            &SourceFile::new(*path, SourceKind::Proto, *text),
            &mut diagnostics,
        );
    }
    (index, diagnostics)
}

/// The findings of indexing `text` as `line: message`.
fn problems(text: &str) -> Vec<String> {
    let (index, diagnostics) = index(&[("a.proto", text)]);
    assert!(
        diagnostics.iter().all(|d| d.code() == Code::InvalidProto),
        "{diagnostics:#?}"
    );
    if !diagnostics.is_empty() {
        assert_eq!(index, ProtoIndex::new(), "a broken file is left out");
    }
    diagnostics
        .iter()
        .map(|d| format!("{}: {}", d.location().map_or(0, |l| l.line()), d.message()))
        .collect()
}

const CHAT: &str = r#"// Chat messages.
syntax = "proto3";

/* The package
   of every message. */
package baylee.v1;

import "google/protobuf/timestamp.proto";
option java_package = "com.example.{chat}"; // braces in strings do not count

message Envelope {
  oneof body {
    Hello hello = 1;
    Bye bye = 2;
  }
  message Meta { string trace = 1; }
  map<string, string> labels = 3;
}

enum Kind { KIND_UNSPECIFIED = 0; }

message Hello { string name = 1; string message = 2; }
message Bye {}

service Chat { rpc Send(Envelope) returns (Envelope); }
"#;

#[test]
fn top_level_messages_are_indexed_with_their_package() {
    let (index, diagnostics) = index(&[("proto/chat.proto", CHAT)]);
    assert!(diagnostics.is_empty(), "{diagnostics:#?}");
    let file = index.message("baylee.v1.Envelope").expect("indexed");
    assert_eq!(file.path(), Path::new("proto/chat.proto"));
    assert_eq!(file.package(), Some("baylee.v1"));
    assert_eq!(file.messages(), ["Envelope", "Hello", "Bye"]);
    assert_eq!(file.text(), CHAT);
    assert_eq!(
        file.schema_format(),
        "application/vnd.google.protobuf;version=3"
    );
    for name in ["baylee.v1.Hello", "baylee.v1.Bye"] {
        assert!(index.message(name).is_some(), "{name}");
    }
    for name in [
        "Envelope",
        "baylee.v1.Meta",
        "baylee.v1.Kind",
        "baylee.v1.Chat",
        "v1.Hello",
    ] {
        assert!(index.message(name).is_none(), "{name}");
    }
    assert_eq!(PROTOBUF_MEDIA_TYPE, "application/x-protobuf");
}

#[test]
fn the_syntax_decides_the_schema_format() {
    let (index, diagnostics) = index(&[
        ("a.proto", "syntax = 'proto2';\nmessage A {}"),
        ("b.proto", "message B {}"),
        ("c.proto", "edition = \"2023\";\nmessage C {}"),
    ]);
    assert!(diagnostics.is_empty(), "{diagnostics:#?}");
    let format = |name: &str| index.message(name).map(|f| f.schema_format());
    assert_eq!(
        format("A"),
        Some("application/vnd.google.protobuf;version=2")
    );
    assert_eq!(
        format("B"),
        Some("application/vnd.google.protobuf;version=3")
    );
    assert_eq!(
        format("C"),
        Some("application/vnd.google.protobuf;version=3")
    );
}

#[test]
fn escapes_and_comment_markers_inside_strings_are_text() {
    let text = "option a = \"say \\\"hi\\\" // not a comment\";\noption b = \"/* nor this */\";\nmessage M {}\n";
    assert!(problems(text).is_empty());
    let (index, _) = index(&[("a.proto", text)]);
    assert!(index.message("M").is_some());
}

#[test]
fn unreadable_files_are_reported_and_left_out() {
    assert_eq!(
        problems("message A {\n  string a = 1;\n"),
        ["2: a `{` is not closed; the file is not indexed"]
    );
    assert_eq!(
        problems("message A {}\n}\n"),
        ["2: `}` without `{`; the file is not indexed"]
    );
    assert_eq!(
        problems("message A {}\n/* open\n\n"),
        ["2: a `/*` comment is not closed; the file is not indexed"]
    );
    assert_eq!(
        problems("option a = \"open;\nmessage A {}\n"),
        ["1: a string is not closed on its line; the file is not indexed"]
    );
    assert_eq!(
        problems("package a;\npackage b;\n"),
        ["2: a second `package`; the file is not indexed"]
    );
    for text in ["package;", "package a b;", "package a..b;", "package 1a;"] {
        assert_eq!(
            problems(text),
            ["1: `package` needs `name.name;`; the file is not indexed"],
            "{text}"
        );
    }
    for text in ["message {}", "message 1A {}", "message A;", "message"] {
        assert_eq!(
            problems(text),
            ["1: `message` needs `Name {`; the file is not indexed"],
            "{text}"
        );
    }
    for text in ["syntax proto3;", "syntax = proto3;", "\nedition;"] {
        assert_eq!(problems(text).len(), 1, "{text}");
    }
    assert!(problems("syntax = \"proto3\";\n// only a comment").is_empty());
}

#[test]
fn a_message_defined_twice_keeps_the_first_file() {
    let (index, diagnostics) = index(&[
        ("a.proto", "package p;\nmessage M {}\nmessage A {}"),
        ("b.proto", "package p;\nmessage B {}\nmessage M {}"),
    ]);
    assert_eq!(codes(&diagnostics), [Code::InvalidProto]);
    let finding = diagnostics.iter().next().unwrap();
    assert_eq!(
        finding.message(),
        "message `p.M` is also defined in a.proto; that definition is used"
    );
    assert_eq!(
        finding.location().map(|l| (l.file(), l.line())),
        Some((Path::new("b.proto"), 3))
    );
    assert_eq!(
        index.message("p.M").map(|f| f.path()),
        Some(Path::new("a.proto"))
    );
    // The other messages of the second file are indexed.
    let b = index.message("p.B").expect("indexed");
    assert_eq!(b.path(), Path::new("b.proto"));
    assert_eq!(b.messages(), ["B"]);
}

#[test]
fn a_message_declared_twice_in_one_file_keeps_the_first() {
    let (index, diagnostics) = index(&[("a.proto", "message M {}\nmessage N {}\nmessage M {}")]);
    assert_eq!(codes(&diagnostics), [Code::InvalidProto]);
    let finding = diagnostics.iter().next().unwrap();
    assert_eq!(
        finding.message(),
        "message `M` is declared twice; the first is used"
    );
    assert_eq!(finding.location().map(|l| l.line()), Some(3));
    assert_eq!(
        index.message("M").map(|f| f.messages()),
        Some(&["M".to_string(), "N".to_string()][..])
    );
}

#[test]
fn messages_without_package_are_found_by_their_name() {
    let (index, _) = index(&[("a.proto", "message _Private1 {}")]);
    assert!(index.message("_Private1").is_some());
    assert_eq!(index.message("_Private1").and_then(|f| f.package()), None);
}

#[test]
fn comments_hide_keywords_braces_and_stars() {
    let text = "// message Fake {\n/* package other;\n * message Hidden { */\npackage p; // }\nmessage Real {}\n";
    assert!(problems(text).is_empty());
    let (index, _) = index(&[("a.proto", text)]);
    let file = index.message("p.Real").expect("indexed");
    assert_eq!(file.messages(), ["Real"]);
    assert!(index.message("p.Fake").is_none());
    assert!(index.message("p.Hidden").is_none());
}

#[test]
fn lines_count_inside_block_comments() {
    assert_eq!(
        problems("/* one\n   two\n*/\nmessage {\n"),
        ["4: `message` needs `Name {`; the file is not indexed"]
    );
}

#[test]
fn an_escaped_quote_does_not_end_a_string() {
    let text = "option a = \"\\\"; message Fake {\";\noption b = 'it\\'s';\nmessage Real {}\n";
    assert!(problems(text).is_empty());
    let (index, _) = index(&[("a.proto", text)]);
    assert!(index.message("Fake").is_none());
    assert!(index.message("Real").is_some());
}

#[test]
fn names_are_identifiers() {
    let (index, diagnostics) = index(&[("a.proto", "package my_pkg.v_1;\nmessage My_Message2 {}")]);
    assert!(diagnostics.is_empty(), "{diagnostics:#?}");
    assert!(index.message("my_pkg.v_1.My_Message2").is_some());
    assert_eq!(
        problems("message A.B {}"),
        ["1: `message` needs `Name {`; the file is not indexed"]
    );
}
