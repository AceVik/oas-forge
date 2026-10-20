//! Tests for `src/visitor.rs`, split by the kind of Rust item being visited.

mod doc_blocks;
mod enums;
mod file_docs;
mod functions;
mod modules;
mod structs;
mod type_aliases;

use crate::common::Project;
use oas_forge::error::Error;
use oas_forge::visitor::{ExtractedItem, extract_from_file, json_merge};
use serde_json::json;

mod json_merge {
    use super::*;

    #[test]
    fn merges_objects_recursively() {
        let mut target = json!({ "a": { "x": 1, "y": 2 }, "keep": true });
        json_merge(&mut target, json!({ "a": { "y": 3, "z": 4 }, "new": "v" }));
        assert_eq!(
            target,
            json!({ "a": { "x": 1, "y": 3, "z": 4 }, "keep": true, "new": "v" })
        );
    }

    #[test]
    fn replaces_non_object_values() {
        let mut target = json!({ "list": [1, 2], "scalar": "old" });
        json_merge(
            &mut target,
            json!({ "list": [3], "scalar": { "now": "object" } }),
        );
        assert_eq!(
            target,
            json!({ "list": [3], "scalar": { "now": "object" } })
        );
    }

    #[test]
    fn replaces_the_whole_value_when_types_differ() {
        let mut target = json!("text");
        json_merge(&mut target, json!({ "k": "v" }));
        assert_eq!(target, json!({ "k": "v" }));
    }
}

mod extract_from_file {
    use super::*;

    #[test]
    fn returns_items_of_a_valid_file() {
        let project = Project::new();
        project.file("lib.rs", "/// @openapi\npub struct User { pub id: i32 }");

        let items = extract_from_file(project.path("lib.rs")).unwrap();

        assert!(matches!(
            items.as_slice(),
            [ExtractedItem::Schema { name: Some(n), line: 1, .. }] if n == "User"
        ));
    }

    #[test]
    fn missing_file_is_an_io_error() {
        let project = Project::new();
        let result = extract_from_file(project.path("missing.rs"));
        assert!(matches!(result, Err(Error::Io(_))));
    }

    #[test]
    fn invalid_rust_is_a_parse_error_naming_the_file() {
        let project = Project::new();
        project.file("broken.rs", "fn {");

        match extract_from_file(project.path("broken.rs")) {
            Err(Error::Parse { file, .. }) => assert_eq!(file, project.path("broken.rs")),
            other => panic!("expected parse error, got {other:?}"),
        }
    }
}
