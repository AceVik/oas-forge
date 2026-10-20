//! `src/extract/`: annotations found in Rust source, split by the kind of
//! Rust item they are attached to.

mod aliases;
mod constants;
mod enums;
mod file_blocks;
mod functions;
mod item_blocks;
mod modules;
mod signatures;
mod structs;

use crate::common::Project;
use oas_forge::diagnostics::{Diagnostics, Location};
use oas_forge::error::Error;
use oas_forge::extract::{
    BlueprintItem, DocumentItem, FragmentItem, Item, Options, RouteItem, extract,
};
use oas_forge::source::{SourceFile, SourceKind};

/// A compact description of every item: `schema:Name`, `named:Name`,
/// `document`, `fragment:Name`, `blueprint:Name`, `route:<operation id>`
/// (`route:-` for file-level routes without one).
pub(super) fn kinds(items: &[Item]) -> Vec<String> {
    items
        .iter()
        .map(|item| match item {
            Item::Schema(s) => format!("schema:{}", s.name),
            Item::NamedSchema(s) => format!("named:{}", s.name),
            Item::Document(_) => "document".into(),
            Item::Fragment(f) => format!("fragment:{}", f.name),
            Item::Blueprint(b) => format!("blueprint:{}", b.name),
            Item::Route(r) => format!("route:{}", r.operation_id.as_deref().unwrap_or("-")),
            Item::Pattern(p) => format!("pattern:{}", p.name),
            Item::AsyncApi(_) => "asyncapi".into(),
            Item::Channel(c) => format!("channel:{}", c.operation_id.as_deref().unwrap_or("-")),
            other => panic!("unexpected item {other:?}"),
        })
        .collect()
}

/// Every free document item.
pub(super) fn document_items(items: &[Item]) -> Vec<&DocumentItem> {
    items
        .iter()
        .filter_map(|item| match item {
            Item::Document(d) => Some(d),
            _ => None,
        })
        .collect()
}

/// Every route item.
pub(super) fn routes(items: &[Item]) -> Vec<&RouteItem> {
    items
        .iter()
        .filter_map(|item| match item {
            Item::Route(r) => Some(r),
            _ => None,
        })
        .collect()
}

/// The fragment called `name`.
pub(super) fn fragment<'a>(items: &'a [Item], name: &str) -> &'a FragmentItem {
    items
        .iter()
        .find_map(|item| match item {
            Item::Fragment(f) if f.name == name => Some(f),
            _ => None,
        })
        .unwrap_or_else(|| panic!("no fragment {name} in {items:#?}"))
}

/// The blueprint called `name`.
pub(super) fn blueprint<'a>(items: &'a [Item], name: &str) -> &'a BlueprintItem {
    items
        .iter()
        .find_map(|item| match item {
            Item::Blueprint(b) if b.name == name => Some(b),
            _ => None,
        })
        .unwrap_or_else(|| panic!("no blueprint {name} in {items:#?}"))
}

#[test]
fn extracts_from_a_source_file() {
    let project = Project::new();
    project.file("lib.rs", "\n/// @openapi\npub struct User { pub id: i32 }");
    let source = SourceFile::new(
        project.path("lib.rs"),
        SourceKind::Rust,
        project.read("lib.rs"),
    );

    let items = extract(&source, Options::new(), &mut Diagnostics::new()).unwrap();

    assert_eq!(kinds(&items), ["schema:User"]);
    assert_eq!(
        items[0].location(),
        &Location::new(project.path("lib.rs"), 2)
    );
}

#[test]
fn every_item_kind_has_a_location() {
    let items = crate::common::extract(
        r#"
        //! @route GET /virtual
        //! @openapi-type Named
        //! type: string

        /// @openapi
        /// paths: {}
        fn doc() {}
        /// @openapi-fragment F
        /// a: 1
        fn frag() {}
        /// @openapi<T>
        struct Page<T> { items: Vec<T> }
        /// @openapi
        struct S;
        "#,
    );
    let lines: Vec<_> = items.iter().map(|i| i.location().line()).collect();
    assert_eq!(
        kinds(&items),
        [
            "route:-",
            "named:Named",
            "document",
            "fragment:F",
            "blueprint:Page",
            "schema:S"
        ]
    );
    assert_eq!(lines, [2, 4, 7, 9, 12, 14]);
}

#[test]
fn invalid_rust_is_a_parse_error_with_position() {
    let source = SourceFile::new("broken.rs", SourceKind::Rust, "fn ok() {}\nfn {");
    match extract(&source, Options::new(), &mut Diagnostics::new()) {
        Err(Error::Parse { location, message }) => {
            assert_eq!(location.file(), std::path::Path::new("broken.rs"));
            assert_eq!(location.line(), 2);
            assert_eq!(location.column(), Some(4));
            assert!(!message.is_empty());
        }
        other => panic!("expected parse error, got {other:?}"),
    }
}
