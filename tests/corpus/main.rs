//! Black-box corpus: every case under `cases/<name>/` is built once with the
//! public `Generator` and rendered with every output strategy. The canonical
//! result (documents, fatal error, diagnostics, spec validation and lints) is
//! snapshotted with `insta`.
//!
//! The corpus is the behavioural safety net for refactoring: a snapshot may only
//! change on purpose. Review changes with `cargo insta review`.
//!
//! A case is a directory with a `case.toml` and its sources (default `src/`):
//!
//! ```toml
//! description = "What the case covers"
//! inputs = ["src"]            # optional, default ["src"]
//! includes = ["shared.yaml"]  # optional files passed to `Generator::include`
//! known_bug = "…"             # optional: the snapshot documents wrong behaviour
//! ```

#[path = "../common/mod.rs"]
mod common;

use common::spec;
use oas_forge::output::{Format, OutputKind};
use oas_forge::{Generator, Spec};
use serde::Deserialize;
use serde_json::{Map, Value, json};
use std::collections::BTreeSet;
use std::path::Path;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Case {
    description: String,
    #[serde(default = "default_inputs")]
    inputs: Vec<String>,
    #[serde(default)]
    includes: Vec<String>,
    #[serde(default)]
    known_bug: Option<String>,
}

fn default_inputs() -> Vec<String> {
    vec!["src".to_string()]
}

const STRATEGIES: [(OutputKind, &str); 4] = [
    (OutputKind::Full, "full"),
    (OutputKind::Schemas, "schemas"),
    (OutputKind::Paths, "paths"),
    (OutputKind::Fragments, "fragments"),
];

#[test]
fn corpus() {
    insta::glob!("cases/*/case.toml", |path| {
        let snapshot = run_case(path);
        insta::assert_json_snapshot!(snapshot);
    });
}

fn run_case(case_file: &Path) -> Value {
    let dir = case_file.parent().unwrap().to_path_buf();
    let case: Case = toml::from_str(&std::fs::read_to_string(case_file).unwrap())
        .unwrap_or_else(|e| panic!("invalid {}: {e}", case_file.display()));

    let mut result = Map::new();
    result.insert("description".into(), json!(case.description));
    if let Some(bug) = &case.known_bug {
        result.insert("known_bug".into(), json!(bug));
    }

    match build(&dir, &case) {
        Ok(spec) => {
            let diagnostics: Vec<String> = spec
                .diagnostics()
                .iter()
                .map(|d| normalize(&d.to_string(), &dir))
                .collect();
            result.insert("diagnostics".into(), json!(diagnostics));
            for (kind, name) in STRATEGIES {
                let entry = match spec.render(kind) {
                    Ok(doc) => describe(kind, round_trip(doc)),
                    Err(error) => json!({ "error": normalize(&error.to_string(), &dir) }),
                };
                result.insert(name.into(), entry);
            }
        }
        Err(error) => {
            result.insert("error".into(), json!(normalize(&error.to_string(), &dir)));
        }
    }
    redact(canonical(Value::Object(result)))
}

fn build(dir: &Path, case: &Case) -> oas_forge::Result<Spec> {
    let mut generator = Generator::new();
    for input in &case.inputs {
        generator = generator.input(dir.join(input));
    }
    for include in &case.includes {
        generator = generator.include(dir.join(include));
    }
    generator.build()
}

/// Every output goes through both file formats; both must preserve the document.
fn round_trip(doc: Value) -> Value {
    let from_json: Value = serde_json::from_str(&Format::Json.render(&doc)).unwrap();
    let from_yaml = common::yaml(&Format::Yaml.render(&doc));
    assert_eq!(from_json, doc, "JSON output changes the document");
    assert_eq!(from_yaml, doc, "YAML output changes the document");
    doc
}

/// Attaches spec validation (full documents) and lints to a generated output.
fn describe(kind: OutputKind, doc: Value) -> Value {
    let known: Option<BTreeSet<String>> = match kind {
        OutputKind::Schemas => doc.as_object().map(|m| m.keys().cloned().collect()),
        OutputKind::Paths => None,
        _ => Some(spec::component_schemas(&doc)),
    };
    let mut entry = Map::new();
    if kind == OutputKind::Full {
        entry.insert("validation".into(), json!(spec::validate(&doc)));
    }
    entry.insert("lints".into(), json!(spec::lint(&doc, known.as_ref())));
    entry.insert("document".into(), doc);
    Value::Object(entry)
}

/// Replaces machine-specific paths so snapshots are portable.
fn normalize(message: &str, case_dir: &Path) -> String {
    let mut text = message.replace('\\', "/");
    for variant in path_variants(case_dir) {
        text = text.replace(&variant, "<case>");
    }
    text
}

fn path_variants(path: &Path) -> Vec<String> {
    let mut variants = vec![path.display().to_string().replace('\\', "/")];
    if let Ok(canonical) = path.canonicalize() {
        variants.push(canonical.display().to_string().replace('\\', "/"));
    }
    // Longest first so a canonical /private/var/… path is replaced before /var/….
    variants.sort_by_key(|v| std::cmp::Reverse(v.len()));
    variants
}

/// Sorts object keys recursively so snapshots do not depend on map order.
fn canonical(value: Value) -> Value {
    match value {
        Value::Object(map) => {
            let mut entries: Vec<_> = map.into_iter().collect();
            entries.sort_by(|a, b| a.0.cmp(&b.0));
            Value::Object(
                entries
                    .into_iter()
                    .map(|(k, v)| (k, canonical(v)))
                    .collect(),
            )
        }
        Value::Array(items) => Value::Array(items.into_iter().map(canonical).collect()),
        other => other,
    }
}

/// The crate version is substituted for `{{CARGO_PKG_VERSION}}`; keep snapshots
/// stable across version bumps.
fn redact(value: Value) -> Value {
    let version = env!("CARGO_PKG_VERSION");
    match value {
        Value::String(s) => Value::String(s.replace(version, "<pkg-version>")),
        Value::Array(items) => Value::Array(items.into_iter().map(redact).collect()),
        Value::Object(map) => Value::Object(map.into_iter().map(|(k, v)| (k, redact(v))).collect()),
        other => other,
    }
}
