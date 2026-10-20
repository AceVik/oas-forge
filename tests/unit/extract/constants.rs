//! Regular expression constants for `regex(path = ..)` / `pattern(..)`.

use crate::common::extract;
use oas_forge::extract::Item;

fn patterns(code: &str) -> Vec<(String, String, usize)> {
    extract(code)
        .into_iter()
        .filter_map(|item| match item {
            Item::Pattern(p) => Some((p.name, p.pattern, p.location.line())),
            _ => None,
        })
        .collect()
}

#[test]
fn statics_and_consts_with_regex_constructors() {
    let found = patterns(
        r##"
static SLUG: LazyLock<Regex> = LazyLock::new(|| Regex::new("^[a-z]+$").unwrap());
static RAW: once_cell::sync::Lazy<regex::Regex> =
    once_cell::sync::Lazy::new(|| regex::Regex::new(r"^\d{3}$").expect("valid"));
static BUILT: LazyLock<Regex> = LazyLock::new(|| RegexBuilder::new(r#"^"q"$"#).build().unwrap());
const NOT_A_REGEX: &str = "^x$";
static NO_LITERAL: LazyLock<Regex> = LazyLock::new(|| Regex::new(PATTERN).unwrap());
static OTHER: u8 = new(5);
"##,
    );
    assert_eq!(
        found,
        [
            ("SLUG".into(), "^[a-z]+$".into(), 2),
            ("RAW".into(), r"^\d{3}$".into(), 3),
            ("BUILT".into(), "^\"q\"$".into(), 5),
        ]
    );
}

#[test]
fn lazy_static_blocks() {
    let found = patterns(
        r#"
lazy_static::lazy_static! {
    static ref A: Regex = Regex::new("a+").unwrap();
    static ref COUNT: usize = 5;
    static ref B: regex::Regex = regex::Regex::new("b+").unwrap();
}
other_macro! { static ref C: Regex = Regex::new("c").unwrap(); }
lazy_static! { static ref }
"#,
    );
    let names: Vec<_> = found.iter().map(|(n, p, _)| format!("{n}={p}")).collect();
    assert_eq!(names, ["A=a+", "B=b+"]);
}

#[test]
fn nested_definitions_are_found() {
    let found = patterns(
        "fn f() { static INNER: LazyLock<Regex> = LazyLock::new(|| Regex::new(\"i\").unwrap()); }",
    );
    assert_eq!(found[0].0, "INNER");
}
