//! Specification checks shared by the suites: validation against the vendored
//! official schemas (`tests/schemas/`) and lints for generator mistakes that
//! the official schemas cannot see (dangling references, leftover markers).

use serde_json::Value;
use std::collections::{BTreeMap, BTreeSet};
use std::path::PathBuf;
use std::sync::OnceLock;

/// The specification a document claims to follow.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum SpecKind {
    Swagger20,
    OpenApi30,
    OpenApi31,
    OpenApi32,
    AsyncApi30,
    AsyncApi31,
}

impl SpecKind {
    /// Detects the specification from the version field of a document.
    pub fn detect(doc: &Value) -> Option<Self> {
        let version = |key: &str| doc.get(key).and_then(Value::as_str);
        if version("swagger") == Some("2.0") {
            return Some(Self::Swagger20);
        }
        if let Some(v) = version("openapi") {
            return match v.get(..3)? {
                "3.0" => Some(Self::OpenApi30),
                "3.1" => Some(Self::OpenApi31),
                "3.2" => Some(Self::OpenApi32),
                _ => None,
            };
        }
        match version("asyncapi")?.get(..3)? {
            "3.0" => Some(Self::AsyncApi30),
            "3.1" => Some(Self::AsyncApi31),
            _ => None,
        }
    }

    fn root_uri(self) -> &'static str {
        match self {
            Self::Swagger20 => "https://spec.openapis.org/oas/2.0/schema/2017-08-27",
            Self::OpenApi30 => "https://spec.openapis.org/oas/3.0/schema/2024-10-18",
            Self::OpenApi31 => "https://spec.openapis.org/oas/3.1/schema-base/2026-08-03",
            Self::OpenApi32 => "https://spec.openapis.org/oas/3.2/schema-base/2026-08-30",
            Self::AsyncApi30 => "https://asyncapi.com/definitions/3.0.0.json",
            Self::AsyncApi31 => "https://asyncapi.com/definitions/3.1.0.json",
        }
    }
}

fn schema_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/schemas")
}

/// Maps schema URIs to the vendored files listed in `tests/schemas/manifest.json`.
struct LocalRetriever {
    files: BTreeMap<String, PathBuf>,
}

impl LocalRetriever {
    fn load() -> Self {
        let manifest: Value = serde_json::from_str(
            &std::fs::read_to_string(schema_dir().join("manifest.json")).expect("manifest"),
        )
        .expect("manifest is JSON");
        let files = manifest["files"]
            .as_object()
            .expect("files map")
            .iter()
            .map(|(uri, file)| (uri.clone(), schema_dir().join(file.as_str().unwrap())))
            .collect();
        Self { files }
    }

    fn read(&self, uri: &str) -> Option<Value> {
        let path = self.files.get(uri.trim_end_matches('#'))?;
        serde_json::from_str(&std::fs::read_to_string(path).ok()?).ok()
    }
}

impl jsonschema::Retrieve for LocalRetriever {
    fn retrieve(
        &self,
        uri: &jsonschema::Uri<String>,
    ) -> Result<Value, Box<dyn std::error::Error + Send + Sync>> {
        // AsyncAPI bundles refer to their own parts over http.
        let uri = uri
            .as_str()
            .replacen("http://asyncapi.com", "https://asyncapi.com", 1);
        self.read(&uri)
            .ok_or_else(|| format!("schema {uri} is not vendored in tests/schemas").into())
    }
}

fn validator(kind: SpecKind) -> &'static jsonschema::Validator {
    static VALIDATORS: OnceLock<BTreeMap<SpecKind, jsonschema::Validator>> = OnceLock::new();
    let all = VALIDATORS.get_or_init(|| {
        let retriever = LocalRetriever::load();
        [
            SpecKind::Swagger20,
            SpecKind::OpenApi30,
            SpecKind::OpenApi31,
            SpecKind::OpenApi32,
            SpecKind::AsyncApi30,
            SpecKind::AsyncApi31,
        ]
        .into_iter()
        .map(|kind| {
            let schema = retriever
                .read(kind.root_uri())
                .expect("vendored root schema");
            let validator = jsonschema::options()
                .with_retriever(LocalRetriever::load())
                .should_validate_formats(true)
                .build(&schema)
                .unwrap_or_else(|e| panic!("cannot build {kind:?} validator: {e}"));
            (kind, validator)
        })
        .collect()
    });
    &all[&kind]
}

/// Validates a complete document against the official schema of the
/// specification it declares (a JSON Schema bundle against the 2020-12
/// meta-schema, with every reference resolved). Returns one message per
/// violation.
pub fn validate(doc: &Value) -> Vec<String> {
    if doc.get("$schema").is_some() {
        let mut errors = Vec::new();
        if let Err(e) = jsonschema::meta::validate(doc) {
            errors.push(format!("{}: {}", e.instance_path(), e.masked()));
        }
        if let Err(e) = jsonschema::validator_for(doc) {
            errors.push(format!("bundle: {e}"));
        }
        return errors;
    }
    let Some(kind) = SpecKind::detect(doc) else {
        return vec!["document declares no supported specification version".to_string()];
    };
    let mut errors: Vec<String> = validator(kind)
        .iter_errors(doc)
        .map(|e| format!("{}: {}", e.instance_path(), e.masked()))
        .collect();
    errors.sort();
    errors.dedup();
    errors
}

/// Whether a document passes validation against its declared specification.
pub fn is_valid(doc: &Value) -> bool {
    SpecKind::detect(doc).is_some_and(|kind| validator(kind).is_valid(doc))
}

/// What local references of a document can point to.
#[derive(Debug, Clone, Copy)]
pub enum Refs<'a> {
    /// The document is complete: every `#/...` reference must resolve in it.
    SelfContained,
    /// A part of a document: `#/components/schemas/<name>` must name one of these.
    Schemas(&'a BTreeSet<String>),
    /// A part of a document whose schemas are unknown (paths-only output).
    Unknown,
}

/// Generator-specific lints: dangling and unresolved references, leftover
/// markers, duplicate operationIds.
pub fn lint(doc: &Value, refs: Refs<'_>) -> Vec<String> {
    let mut findings = BTreeSet::new();
    walk(doc, doc, &mut String::new(), refs, &mut findings);

    let mut operation_ids = BTreeMap::<&str, usize>::new();
    if let Some(paths) = doc.get("paths").and_then(Value::as_object) {
        for item in paths.values().filter_map(Value::as_object) {
            for op in item.values().filter_map(Value::as_object) {
                if let Some(id) = op.get("operationId").and_then(Value::as_str) {
                    *operation_ids.entry(id).or_default() += 1;
                }
            }
        }
    }
    for (id, count) in operation_ids {
        if count > 1 {
            findings.insert(format!("duplicate operationId `{id}` ({count}x)"));
        }
    }
    findings.into_iter().collect()
}

/// Schema names defined by a full or headless document.
pub fn component_schemas(doc: &Value) -> BTreeSet<String> {
    doc.pointer("/components/schemas")
        .and_then(Value::as_object)
        .map(|m| m.keys().cloned().collect())
        .unwrap_or_default()
}

fn walk(
    root: &Value,
    value: &Value,
    pointer: &mut String,
    refs: Refs<'_>,
    findings: &mut BTreeSet<String>,
) {
    match value {
        Value::Object(map) => {
            for (key, child) in map {
                let len = pointer.len();
                pointer.push('/');
                pointer.push_str(&key.replace('~', "~0").replace('/', "~1"));
                if key == "x-openapi-extend" {
                    findings.insert(format!("{pointer}: leftover @extend marker"));
                }
                if key.starts_with("x-oas-forge-") {
                    // Template transport: bodies are unexpanded on purpose.
                } else if key == "$ref" {
                    if let Some(target) = child.as_str() {
                        check_ref(root, target, pointer, refs, findings);
                    }
                } else {
                    walk(root, child, pointer, refs, findings);
                }
                pointer.truncate(len);
            }
        }
        Value::Array(items) => {
            for (i, child) in items.iter().enumerate() {
                let len = pointer.len();
                pointer.push_str(&format!("/{i}"));
                walk(root, child, pointer, refs, findings);
                pointer.truncate(len);
            }
        }
        Value::String(s) => {
            if s.starts_with('$') && s[1..].starts_with(|c: char| c.is_alphabetic() || c == '_') {
                findings.insert(format!("{pointer}: unresolved smart reference `{s}`"));
            }
            for marker in ["@insert", "@extend", "{{"] {
                if s.contains(marker) {
                    findings.insert(format!("{pointer}: leftover `{marker}` in `{s}`"));
                }
            }
        }
        _ => {}
    }
}

fn check_ref(
    root: &Value,
    target: &str,
    pointer: &str,
    refs: Refs<'_>,
    findings: &mut BTreeSet<String>,
) {
    if let Some(local) = target.strip_prefix('#') {
        let dangling = match refs {
            Refs::SelfContained => root.pointer(local).is_none(),
            Refs::Schemas(known) => target
                .strip_prefix("#/components/schemas/")
                .is_some_and(|name| !known.contains(name)),
            Refs::Unknown => false,
        };
        if dangling {
            findings.insert(format!("{pointer}: dangling $ref `{target}`"));
        }
    } else if !target.starts_with("#/")
        && !target.contains("://")
        && !target.starts_with("./")
        && !target.starts_with("../")
    {
        findings.insert(format!("{pointer}: unresolved $ref `{target}`"));
    }
}
