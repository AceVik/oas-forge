//! `src/template/aliases.rs`: which type alias without `@openapi` a name
//! means where it is used. The lookup is internal; it is tested through the
//! pipeline.

use crate::common::{codes, pipeline};
use oas_forge::diagnostics::Code;
use serde_json::json;

#[test]
fn a_renamed_documented_type_wins_over_an_alias_of_another_file() {
    let (document, diagnostics) = pipeline(&[
        ("a.rs", "pub type Owner = Team;"),
        (
            "b.rs",
            "/// @openapi rename \"OwnerDto\"\npub struct Owner { pub id: u8 }\n/// @openapi\npub struct Repo { pub owner: Owner }",
        ),
    ]);
    assert!(diagnostics.is_empty(), "{diagnostics:?}");
    assert_eq!(
        document["components"]["schemas"]["Repo"]["properties"]["owner"],
        json!({ "$ref": "#/components/schemas/OwnerDto" })
    );
}

#[test]
fn a_documented_template_wins_over_a_generic_alias_of_another_file() {
    let (document, diagnostics) = pipeline(&[
        ("a.rs", "pub type Page<T> = Vec<T>;"),
        (
            "b.rs",
            "/// @openapi\npub struct Page<T> { pub items: Vec<T> }\n/// @openapi\npub struct Repo { pub page: Page<u8> }",
        ),
    ]);
    assert!(diagnostics.is_empty(), "{diagnostics:?}");
    let schemas = &document["components"]["schemas"];
    assert_eq!(
        schemas["Repo"]["properties"]["page"],
        json!({ "$ref": "#/components/schemas/Page_u8" })
    );
    assert_eq!(schemas["Page_u8"]["type"], "object");
}

#[test]
fn a_documented_type_wins_over_an_ambiguous_name() {
    let (document, diagnostics) = pipeline(&[
        ("a.rs", "type Owner = Team;"),
        ("b.rs", "type Owner = u8;"),
        (
            "c.rs",
            "/// @openapi\npub struct Owner { pub id: u8 }\n/// @openapi\npub struct Repo { pub owner: Owner }",
        ),
    ]);
    assert_eq!(codes(&diagnostics), [], "{diagnostics:?}");
    assert_eq!(
        document["components"]["schemas"]["Repo"]["properties"]["owner"],
        json!({ "$ref": "#/components/schemas/Owner" })
    );
}

#[test]
fn an_ambiguous_name_is_reported_once_per_use_site() {
    let (_, diagnostics) = pipeline(&[
        ("a.rs", "type Code = u8;"),
        ("b.rs", "type Code = String;"),
        (
            "c.rs",
            "/// @openapi\npub struct A { pub a: Code, pub b: Vec<Code> }\n/// @openapi\npub struct B { pub c: Option<Code> }",
        ),
    ]);
    let lines: Vec<usize> = diagnostics
        .iter()
        .filter(|d| d.code() == Code::AmbiguousAlias)
        .filter_map(|d| d.location().map(|l| l.line()))
        .collect();
    assert_eq!(lines, [1, 3]);
    assert!(
        !codes(&diagnostics).contains(&Code::UnresolvedReference),
        "{diagnostics:?}"
    );
}

/// A chain of `n` type aliases, `A0` → `A1` → … → `String`, and a type
/// that uses its start directly and as a type argument.
fn alias_chain(n: usize) -> String {
    let mut text = String::from(
        "/// @openapi\npub struct Page<T> { pub items: Vec<T> }\n\
         /// @openapi\npub struct Repo { pub direct: A0, pub page: Page<A0> }\n",
    );
    for i in 0..n - 1 {
        text.push_str(&format!("pub type A{i} = A{};\n", i + 1));
    }
    text.push_str(&format!("pub type A{} = String;\n", n - 1));
    text
}

#[test]
fn alias_chains_up_to_the_limit_are_resolved() {
    let (document, diagnostics) = pipeline(&[("a.rs", &alias_chain(64))]);
    assert!(diagnostics.is_empty(), "{diagnostics:?}");
    let schemas = &document["components"]["schemas"];
    assert_eq!(
        schemas["Repo"]["properties"]["direct"],
        json!({ "type": "string" })
    );
    assert_eq!(
        schemas["Repo"]["properties"]["page"],
        json!({ "$ref": "#/components/schemas/Page_String" })
    );
}

/// Every link was resolved by a recursive call: 20 000 links overflowed
/// the stack.
#[test]
fn longer_alias_chains_are_reported_not_followed() {
    for n in [65, 20_000] {
        let (_, diagnostics) = pipeline(&[("a.rs", &alias_chain(n))]);
        let found: Vec<String> = diagnostics
            .iter()
            .filter(|d| d.code() == Code::AliasDepthLimit)
            .map(ToString::to_string)
            .collect();
        assert!(!found.is_empty(), "{n}: {diagnostics:?}");
        assert!(found[0].contains("64"), "{found:?}");
    }
}
