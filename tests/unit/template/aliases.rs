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

/// Type aliases whose target names the next alias twice, `depth` deep:
/// `form` is `tuple` (`type T0<X> = (T1<X>, T1<X>);`), `plain` (the same
/// without parameters) or `generic` (`type T0<X> = Pair<T1<X>, T1<X>>;`,
/// a generic argument).
fn doubling_aliases(form: &str, depth: usize) -> String {
    let mut text = String::from("/// @openapi\npub struct User { pub id: u64 }\n");
    text.push_str(match form {
        "plain" => "/// @openapi\npub struct S { pub a: T0 }\n",
        "tuple" => "/// @openapi\npub struct S { pub a: T0<u8> }\n",
        _ => "/// @openapi\n/// properties: { a: { $ref: '$T0<User>' } }\npub struct S;\n",
    });
    for i in 0..depth {
        let next = i + 1;
        text.push_str(&match form {
            "plain" => format!("pub type T{i} = (T{next}, T{next});\n"),
            "tuple" => format!("pub type T{i}<X> = (T{next}<X>, T{next}<X>);\n"),
            _ => format!("pub type T{i}<X> = Pair<T{next}<X>, T{next}<X>>;\n"),
        });
    }
    text.push_str(&if form == "plain" {
        format!("pub type T{depth} = u8;\n")
    } else {
        format!("pub type T{depth}<X> = X;\n")
    });
    text
}

#[test]
fn doubling_aliases_resolve_within_the_budget() {
    for form in ["plain", "tuple", "generic"] {
        let (document, diagnostics) = pipeline(&[("a.rs", &doubling_aliases(form, 4))]);
        assert!(
            !codes(&diagnostics).contains(&Code::InstantiationLimit),
            "{form}: {diagnostics:?}"
        );
        assert!(document["components"]["schemas"]["S"].is_object(), "{form}");
    }
}

/// Each link doubled the work: 20 links ran for minutes and wrote 700 MB.
/// The targets share the size budget of generic instantiation, across the
/// generics and the references stage, so running out is reported once.
/// (Not deeper: mutation testing runs this without the budget.)
#[test]
fn doubling_aliases_stop_at_the_budget() {
    for form in ["plain", "tuple", "generic"] {
        let started = std::time::Instant::now();
        let (_, diagnostics) = pipeline(&[("a.rs", &doubling_aliases(form, 20))]);
        assert!(
            started.elapsed() < std::time::Duration::from_secs(60),
            "{form}: {:?}",
            started.elapsed()
        );
        let limits = codes(&diagnostics)
            .into_iter()
            .filter(|code| *code == Code::InstantiationLimit)
            .count();
        assert_eq!(limits, 1, "{form}: {diagnostics:?}");
    }
}

/// A large target counts with its size each time it replaces an alias.
#[test]
fn alias_targets_count_with_their_size() {
    let tuple = vec!["String"; 2000].join(", ");
    for (uses, over) in [(100, false), (1000, true)] {
        let fields: String = (0..uses).map(|i| format!("pub f{i}: Big, ")).collect();
        let text =
            format!("pub type Big = ({tuple});\n/// @openapi\npub struct S {{ {fields} }}\n");
        let (document, diagnostics) = pipeline(&[("a.rs", &text)]);
        assert_eq!(
            codes(&diagnostics).contains(&Code::InstantiationLimit),
            over,
            "{uses}: {diagnostics:?}"
        );
        if !over {
            let field = &document["components"]["schemas"]["S"]["properties"]["f99"];
            assert_eq!(field["prefixItems"].as_array().map(Vec::len), Some(2000));
        }
    }
}

/// The argument of an alias that is a type argument (`Page<Wrap<…>>`)
/// counts with its length once, not per byte of the alias.
#[test]
fn generic_alias_arguments_count_with_their_length() {
    let wide = vec!["User"; 50_000].join(", ");
    let text = format!(
        "/// @openapi\npub struct User {{ pub id: u8 }}\n/// @openapi\npub struct Page<T> {{ pub items: Vec<T> }}\npub type Wrap<X> = Pair<X, X>;\n/// @openapi\n/// properties: {{ a: {{ $ref: '$Page<Wrap<Tuple<{wide}>>>' }} }}\npub struct S;\n"
    );
    let (_, diagnostics) = pipeline(&[("a.rs", &text)]);
    assert!(
        !codes(&diagnostics).contains(&Code::InstantiationLimit),
        "{diagnostics:?}"
    );
}

/// Each ambiguous use is a finding of its own; checking a new one against
/// all findings so far made 16 000 uses take seconds and 40 000 minutes.
#[test]
fn many_ambiguous_uses_are_reported_quickly() {
    let structs: String = (0..30_000)
        .map(|i| format!("/// @openapi\npub struct S{i} {{ pub a: Code }}\n"))
        .collect();
    let started = std::time::Instant::now();
    let (_, diagnostics) = pipeline(&[
        ("a.rs", "type Code = u8;"),
        ("b.rs", "type Code = String;"),
        ("c.rs", &structs),
    ]);
    assert!(
        started.elapsed() < std::time::Duration::from_secs(30),
        "{:?}",
        started.elapsed()
    );
    let ambiguous = codes(&diagnostics)
        .into_iter()
        .filter(|code| *code == Code::AmbiguousAlias)
        .count();
    assert_eq!(ambiguous, 30_000);
}
