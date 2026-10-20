//! Black-box corpus: every case under `cases/<name>/` is run through the public
//! `Generator` once per output strategy. The canonical result (documents,
//! errors, log warnings, spec validation and lints) is snapshotted with `insta`.
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
use oas_forge::Generator;
use serde::Deserialize;
use serde_json::{Map, Value, json};
use std::cell::RefCell;
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

#[derive(Clone, Copy)]
enum Strategy {
    Full,
    Schemas,
    Paths,
    Fragments,
}

impl Strategy {
    const ALL: [Self; 4] = [Self::Full, Self::Schemas, Self::Paths, Self::Fragments];

    fn name(self) -> &'static str {
        match self {
            Self::Full => "full",
            Self::Schemas => "schemas",
            Self::Paths => "paths",
            Self::Fragments => "fragments",
        }
    }
}

#[test]
fn corpus() {
    capture::install();
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

    let mut log = Vec::<String>::new();
    for strategy in Strategy::ALL {
        let out = tempfile::tempdir().unwrap();
        let (outcome, messages) =
            capture::during(|| generate(&dir, &case, strategy, &out.path().join("out.json")));
        for message in messages {
            let message = normalize(&message, &dir, out.path());
            if !log.contains(&message) {
                log.push(message);
            }
        }
        let entry = match outcome {
            Ok(doc) => describe(strategy, doc),
            Err(error) => json!({ "error": normalize(&error, &dir, out.path()) }),
        };
        result.insert(strategy.name().into(), entry);
    }
    result.insert("log".into(), json!(log));
    redact(canonical(Value::Object(result)))
}

fn generate(dir: &Path, case: &Case, strategy: Strategy, out: &Path) -> Result<Value, String> {
    let mut generator = Generator::new();
    for input in &case.inputs {
        generator = generator.input(dir.join(input));
    }
    for include in &case.includes {
        generator = generator.include(dir.join(include));
    }
    generator = match strategy {
        Strategy::Full => generator.output(out),
        Strategy::Schemas => generator.output_schemas(out),
        Strategy::Paths => generator.output_paths(out),
        Strategy::Fragments => generator.output_fragments(out),
    };
    generator.generate().map_err(|e| e.to_string())?;
    let text = std::fs::read_to_string(out).map_err(|e| e.to_string())?;
    serde_json::from_str(&text).map_err(|e| format!("output is not JSON: {e}"))
}

/// Attaches spec validation (full documents) and lints to a generated output.
fn describe(strategy: Strategy, doc: Value) -> Value {
    let known: Option<BTreeSet<String>> = match strategy {
        Strategy::Full | Strategy::Fragments => Some(spec::component_schemas(&doc)),
        Strategy::Schemas => doc.as_object().map(|m| m.keys().cloned().collect()),
        Strategy::Paths => None,
    };
    let mut entry = Map::new();
    if let Strategy::Full = strategy {
        entry.insert("validation".into(), json!(spec::validate(&doc)));
    }
    entry.insert("lints".into(), json!(spec::lint(&doc, known.as_ref())));
    entry.insert("document".into(), doc);
    Value::Object(entry)
}

/// Replaces machine-specific paths so snapshots are portable.
fn normalize(message: &str, case_dir: &Path, out_dir: &Path) -> String {
    let mut text = message.replace('\\', "/");
    for (path, placeholder) in [(out_dir, "<out>"), (case_dir, "<case>")] {
        for variant in path_variants(path) {
            text = text.replace(&variant, placeholder);
        }
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

/// Captures `log` warnings and errors emitted on the current thread.
mod capture {
    use super::RefCell;
    use log::{Level, Log, Metadata, Record};

    thread_local! {
        static MESSAGES: RefCell<Option<Vec<String>>> = const { RefCell::new(None) };
    }

    struct Capture;

    impl Log for Capture {
        fn enabled(&self, metadata: &Metadata) -> bool {
            metadata.level() <= Level::Warn
        }

        fn log(&self, record: &Record) {
            if self.enabled(record.metadata()) {
                MESSAGES.with(|m| {
                    if let Some(messages) = m.borrow_mut().as_mut() {
                        messages.push(format!("{}: {}", record.level(), record.args()));
                    }
                });
            }
        }

        fn flush(&self) {}
    }

    pub fn install() {
        static LOGGER: Capture = Capture;
        if log::set_logger(&LOGGER).is_ok() {
            log::set_max_level(log::LevelFilter::Warn);
        }
    }

    pub fn during<T>(f: impl FnOnce() -> T) -> (T, Vec<String>) {
        MESSAGES.with(|m| *m.borrow_mut() = Some(Vec::new()));
        let result = f();
        let messages = MESSAGES.with(|m| m.borrow_mut().take().unwrap_or_default());
        (result, messages)
    }
}
