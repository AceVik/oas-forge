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
//! strict_lints = true         # optional: unused components, missing descriptions
//! scopes = ["public", "admin"] # optional: snapshot the document of each scope
//!
//! allow_env = ["NAME"]        # optional `Generator::allow_env` names
//!
//! [variables]                 # optional `Generator::var` values
//! STAGE = "prod"
//!
//! [package]                   # optional `Generator::package` (the fallback root)
//! name = "shop"
//! version = "1.0.0"
//! ```

#[path = "../common/mod.rs"]
mod common;

use common::spec;
use oas_forge::output::{Format, OutputKind};
use oas_forge::{Generator, Package, Spec};
use serde::Deserialize;
use serde_json::{Map, Value, json};
use std::collections::{BTreeMap, BTreeSet};
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
    #[serde(default)]
    strict_lints: bool,
    #[serde(default)]
    scopes: Vec<String>,
    #[serde(default)]
    variables: BTreeMap<String, String>,
    #[serde(default)]
    allow_env: Vec<String>,
    #[serde(default)]
    package: Option<CasePackage>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct CasePackage {
    name: String,
    version: String,
    description: Option<String>,
    license: Option<String>,
    repository: Option<String>,
    homepage: Option<String>,
}

impl CasePackage {
    fn to_package(&self) -> Package {
        let mut package = Package::new(&self.name, &self.version);
        package.description = self.description.clone();
        package.license = self.license.clone();
        package.repository = self.repository.clone();
        package.homepage = self.homepage.clone();
        package
    }
}

fn default_inputs() -> Vec<String> {
    vec!["src".to_string()]
}

const STRATEGIES: [(OutputKind, &str); 8] = [
    (OutputKind::Full, "full"),
    (OutputKind::Schemas, "schemas"),
    (OutputKind::Paths, "paths"),
    (OutputKind::Fragments, "fragments"),
    (OutputKind::JsonSchema, "json_schema"),
    (OutputKind::OpenApi30, "openapi_30"),
    (OutputKind::Swagger20, "swagger_2"),
    (OutputKind::AsyncApi, "asyncapi"),
];

#[test]
fn corpus() {
    insta::glob!("cases/*/case.toml", |path| {
        let snapshot = run_case(path);
        insta::assert_json_snapshot!(snapshot);
        if let Some(text) = rendered_yaml(path) {
            insta::assert_snapshot!("yaml", text);
        }
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
            if !case.scopes.is_empty() {
                let mut scopes = Map::new();
                for scope in &case.scopes {
                    let mut outputs = Map::new();
                    for (kind, name) in STRATEGIES {
                        let entry = match spec.render_scoped(kind, scope) {
                            Ok(doc) => describe(kind, round_trip(doc)),
                            Err(error) => json!({ "error": normalize(&error.to_string(), &dir) }),
                        };
                        outputs.insert(name.into(), entry);
                    }
                    scopes.insert(scope.clone(), Value::Object(outputs));
                }
                result.insert("scopes".into(), Value::Object(scopes));
                return redact(canonical(Value::Object(result)));
            }
            for (kind, name) in STRATEGIES {
                let entry = match spec.render_with_diagnostics(kind) {
                    Ok((doc, findings)) => {
                        let mut entry = describe(kind, round_trip(doc));
                        if !findings.is_empty() {
                            let findings: Vec<String> = findings
                                .iter()
                                .map(|d| normalize(&d.to_string(), &dir))
                                .collect();
                            entry["diagnostics"] = json!(findings);
                        }
                        entry
                    }
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

/// The exact YAML file a user gets: the full spec, or the fragments output
/// when the case has no root document.
fn rendered_yaml(case_file: &Path) -> Option<String> {
    let dir = case_file.parent().unwrap();
    let case: Case = toml::from_str(&std::fs::read_to_string(case_file).unwrap()).unwrap();
    let spec = build(dir, &case).ok()?;
    if !case.scopes.is_empty() {
        let mut text = String::new();
        for scope in &case.scopes {
            let value = spec.render_scoped(OutputKind::Full, scope).ok()?;
            text.push_str(&format!("# full output of scope {scope}\n"));
            text.push_str(&redact_text(&Format::Yaml.render(&value)));
        }
        return Some(text);
    }
    let (kind, value) = match spec.render(OutputKind::Full) {
        Ok(value) => ("full", value),
        Err(_) => ("fragments", spec.render(OutputKind::Fragments).ok()?),
    };
    let text = Format::Yaml.render(&value);
    Some(format!("# {kind} output\n{}", redact_text(&text)))
}

fn build(dir: &Path, case: &Case) -> oas_forge::Result<Spec> {
    let mut generator = Generator::new();
    for input in &case.inputs {
        generator = generator.input(dir.join(input));
    }
    for include in &case.includes {
        generator = generator.include(dir.join(include));
    }
    for (name, value) in &case.variables {
        generator = generator.var(name, value);
    }
    for name in &case.allow_env {
        generator = generator.allow_env(name);
    }
    if let Some(package) = &case.package {
        generator = generator.package(package.to_package());
    }
    // Each scope's document is built (and its findings reported) as for a
    // configured output; nothing is written.
    for scope in &case.scopes {
        generator = generator.output_scoped(
            scope,
            OutputKind::Full,
            dir.join(format!("out/{scope}.yaml")),
        );
    }
    generator.strict_lints(case.strict_lints).build()
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
    let names: BTreeSet<String> = match kind {
        OutputKind::Schemas => doc
            .as_object()
            .map(|m| m.keys().cloned().collect())
            .unwrap_or_default(),
        _ => spec::component_schemas(&doc),
    };
    let refs = match kind {
        OutputKind::Paths => spec::Refs::Unknown,
        OutputKind::Schemas => spec::Refs::Schemas(&names),
        // Fragments may refer to schemas of other crates.
        OutputKind::Fragments => spec::Refs::Schemas(&names),
        _ => spec::Refs::SelfContained,
    };
    let mut entry = Map::new();
    if matches!(
        kind,
        OutputKind::Full
            | OutputKind::JsonSchema
            | OutputKind::OpenApi30
            | OutputKind::Swagger20
            | OutputKind::AsyncApi
    ) {
        entry.insert("validation".into(), json!(spec::validate(&doc)));
    }
    entry.insert("lints".into(), json!(spec::lint(&doc, refs)));
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
fn redact_text(text: &str) -> String {
    text.replace(env!("CARGO_PKG_VERSION"), "<pkg-version>")
}

/// [`redact_text`] for every string of a value.
fn redact(value: Value) -> Value {
    match value {
        Value::String(s) => Value::String(redact_text(&s)),
        Value::Array(items) => Value::Array(items.into_iter().map(redact).collect()),
        Value::Object(map) => Value::Object(map.into_iter().map(|(k, v)| (k, redact(v))).collect()),
        other => other,
    }
}
