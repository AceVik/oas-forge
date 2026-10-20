//! The generator: configuration, the pipeline run and its result.

use super::attrs::validate::resolve_patterns;
use super::config::Config;
use super::diagnostics::{Code, Diagnostic, Diagnostics, Location, Severity};
use super::document::for_each_operation;
use super::dsl::{self, RouteSource};
use super::error::{Error, Result};
use super::extract::{self, Item};
use super::lint;
use super::merge::{Policy, merge};
use super::output::{self, Format, OutputKind};
use super::schema::TypeMapper;
use super::schema::dialect::{self, Dialect};
use super::source::{DEFAULT_MAX_FILE_SIZE, DiscoverOptions, SourceFile, SourceKind, SourceSet};
use super::template::generics::Instantiator;
use super::template::lexical::{self, EXTEND_KEY, Variables};
use super::template::refs::{self, KnownSchemas, Targets};
use super::template::{Blueprint, Fragment, Registry, extend};
use super::yaml;
use serde_json::{Map, Value, json};
use std::collections::BTreeMap;
use std::path::PathBuf;

/// Builds OpenAPI documents from Rust sources.
///
/// ```no_run
/// oas_forge::Generator::new()
///     .input("src")
///     .output("openapi.yaml")
///     .generate()
///     .expect("OpenAPI generation failed");
/// ```
#[derive(Debug, Clone)]
pub struct Generator {
    inputs: Vec<PathBuf>,
    includes: Vec<PathBuf>,
    outputs: Vec<(OutputKind, PathBuf)>,
    variables: Variables,
    deny_warnings: bool,
    max_file_size: u64,
    cargo_directives: bool,
    types: TypeMapper,
    infer: bool,
}

impl Default for Generator {
    fn default() -> Self {
        let mut variables = Variables::new();
        variables.insert(
            "CARGO_PKG_VERSION".into(),
            std::env::var("CARGO_PKG_VERSION").unwrap_or_else(|_| "0.0.0".into()),
        );
        Self {
            inputs: Vec::new(),
            includes: Vec::new(),
            outputs: Vec::new(),
            variables,
            deny_warnings: false,
            max_file_size: DEFAULT_MAX_FILE_SIZE,
            cargo_directives: false,
            types: TypeMapper::new(),
            infer: true,
        }
    }
}

impl Generator {
    /// A generator without inputs and outputs.
    pub fn new() -> Self {
        Self::default()
    }

    /// Adds the inputs and outputs of `config`.
    pub fn with_config(mut self, config: Config) -> Self {
        let Config {
            input,
            include,
            output,
            output_schemas,
            output_paths,
            output_fragments,
            output_json_schema,
            output_openapi_30,
            output_swagger2,
            output_asyncapi,
            types,
            ..
        } = config;
        for (name, schema) in types.unwrap_or_default() {
            self.types = self.types.with_type(name, schema);
        }
        self.inputs.extend(input.unwrap_or_default());
        self.includes.extend(include.unwrap_or_default());
        for (kind, paths) in [
            (OutputKind::Full, output),
            (OutputKind::Schemas, output_schemas),
            (OutputKind::Paths, output_paths),
            (OutputKind::Fragments, output_fragments),
            (OutputKind::JsonSchema, output_json_schema),
            (OutputKind::OpenApi30, output_openapi_30),
            (OutputKind::Swagger20, output_swagger2),
            (OutputKind::AsyncApi, output_asyncapi),
        ] {
            self.outputs
                .extend(paths.unwrap_or_default().into_iter().map(|p| (kind, p)));
        }
        self
    }

    /// Adds a directory that is scanned recursively.
    pub fn input(mut self, path: impl Into<PathBuf>) -> Self {
        self.inputs.push(path.into());
        self
    }

    /// Adds a single YAML/JSON file.
    pub fn include(mut self, path: impl Into<PathBuf>) -> Self {
        self.includes.push(path.into());
        self
    }

    /// Writes the full specification to `path`.
    pub fn output(self, path: impl Into<PathBuf>) -> Self {
        self.output_as(OutputKind::Full, path)
    }

    /// Writes only `components.schemas` to `path`.
    pub fn output_schemas(self, path: impl Into<PathBuf>) -> Self {
        self.output_as(OutputKind::Schemas, path)
    }

    /// Writes only `paths` to `path`.
    pub fn output_paths(self, path: impl Into<PathBuf>) -> Self {
        self.output_as(OutputKind::Paths, path)
    }

    /// Writes the headless specification for other crates to `path`.
    pub fn output_fragments(self, path: impl Into<PathBuf>) -> Self {
        self.output_as(OutputKind::Fragments, path)
    }

    /// Writes the component schemas as a JSON Schema 2020-12 bundle to `path`.
    pub fn output_json_schema(self, path: impl Into<PathBuf>) -> Self {
        self.output_as(OutputKind::JsonSchema, path)
    }

    /// Writes the specification lowered to OpenAPI 3.0.3 to `path`.
    pub fn output_openapi_30(self, path: impl Into<PathBuf>) -> Self {
        self.output_as(OutputKind::OpenApi30, path)
    }

    /// Writes the specification converted to Swagger 2.0 to `path`.
    pub fn output_swagger2(self, path: impl Into<PathBuf>) -> Self {
        self.output_as(OutputKind::Swagger20, path)
    }

    /// Writes the AsyncAPI document (from `@asyncapi` and `@channel` blocks)
    /// to `path`.
    pub fn output_asyncapi(self, path: impl Into<PathBuf>) -> Self {
        self.output_as(OutputKind::AsyncApi, path)
    }

    /// Writes the output strategy `kind` to `path`.
    pub fn output_as(mut self, kind: OutputKind, path: impl Into<PathBuf>) -> Self {
        self.outputs.push((kind, path.into()));
        self
    }

    /// Sets the value of a `{{name}}` placeholder.
    pub fn var(mut self, name: impl Into<String>, value: impl Into<String>) -> Self {
        self.variables.insert(name.into(), value.into());
        self
    }

    /// Documents the Rust type `name` (or path `my_crate::Name`) with `schema`
    /// instead of the built-in mapping or a `$ref`.
    ///
    /// ```
    /// let generator = oas_forge::Generator::new()
    ///     .map_type("UserId", serde_json::json!({ "type": "string", "format": "uuid" }));
    /// ```
    pub fn map_type(mut self, name: impl Into<String>, schema: Value) -> Self {
        self.types = self.types.with_type(name, schema);
        self
    }

    /// Fills gaps of `@route` blocks from the handler signature (axum and
    /// actix-web extractors such as `Path<T>`, `Query<T>`, `Json<T>`); on by
    /// default. A single route opts out with `@no-infer`.
    pub fn infer_signatures(mut self, infer: bool) -> Self {
        self.infer = infer;
        self
    }

    /// Treats warnings as errors.
    pub fn deny_warnings(mut self, deny: bool) -> Self {
        self.deny_warnings = deny;
        self
    }

    /// Sets the size limit per input file in bytes.
    pub fn max_file_size(mut self, bytes: u64) -> Self {
        self.max_file_size = bytes;
        self
    }

    /// In build scripts: print `cargo:warning=` for diagnostics and
    /// `cargo:rerun-if-changed=` for every input when generating.
    pub fn emit_cargo_directives(mut self, emit: bool) -> Self {
        self.cargo_directives = emit;
        self
    }

    /// Runs the pipeline without writing files.
    ///
    /// # Errors
    /// Fatal problems (unreadable inputs, invalid Rust, invalid YAML, multiple
    /// roots). Error-level diagnostics are returned in the [`Spec`].
    pub fn build(&self) -> Result<Spec> {
        let mut diagnostics = Diagnostics::new();
        let options = DiscoverOptions::default()
            .with_exclude(self.outputs.iter().map(|(_, p)| p.clone()).collect())
            .with_max_file_size(self.max_file_size);
        let sources =
            SourceSet::discover(&self.inputs, &self.includes, &options, &mut diagnostics)?;
        let settings = Settings {
            variables: &self.variables,
            types: &self.types,
            infer: self.infer,
        };
        let documents = run(&sources, settings, &mut diagnostics)?;
        let mut spec = Spec {
            document: documents.document,
            canonical: documents.canonical,
            asyncapi: documents.asyncapi,
            registry: documents.registry,
            outputs: self.outputs.clone(),
            diagnostics,
            watched: sources.watched().to_vec(),
            rendered: BTreeMap::new(),
        };
        // Configured outputs are rendered now, so their findings (e.g. what
        // an OpenAPI 3.0 export cannot express) are part of the run.
        let mut kinds: Vec<OutputKind> = self.outputs.iter().map(|(kind, _)| *kind).collect();
        kinds.sort_unstable();
        kinds.dedup();
        for kind in kinds {
            if let Ok((value, findings)) = spec.render_with_diagnostics(kind) {
                spec.diagnostics.extend(findings);
                spec.rendered.insert(kind, value);
            }
        }
        if self.deny_warnings {
            spec.diagnostics.deny_warnings();
        }
        Ok(spec)
    }

    /// Runs the pipeline and writes every configured output.
    ///
    /// # Errors
    /// [`Error::NoOutput`] without outputs, any [`Generator::build`] error,
    /// [`Error::Diagnostics`] when a diagnostic has error severity, and
    /// [`Error::NoRootFound`] for a full output without root document.
    pub fn generate(&self) -> Result<()> {
        if self.outputs.is_empty() {
            return Err(Error::NoOutput);
        }
        let spec = self.build()?;
        if self.cargo_directives {
            spec.print_cargo_directives();
        }
        if spec.diagnostics.has_errors() {
            return Err(Error::Diagnostics(spec.diagnostics.into_vec()));
        }
        spec.write()?;
        Ok(())
    }
}

/// The result of a pipeline run.
#[derive(Debug, Clone)]
pub struct Spec {
    document: Value,
    canonical: Value,
    asyncapi: Value,
    registry: Registry,
    outputs: Vec<(OutputKind, PathBuf)>,
    diagnostics: Diagnostics,
    watched: Vec<PathBuf>,
    rendered: BTreeMap<OutputKind, Value>,
}

impl Spec {
    /// The merged document (it has no root when none was defined).
    pub fn document(&self) -> &Value {
        &self.document
    }

    /// The templates and fragments known to this run.
    pub fn registry(&self) -> &Registry {
        &self.registry
    }

    /// Every finding of the run.
    pub fn diagnostics(&self) -> &Diagnostics {
        &self.diagnostics
    }

    /// Inputs whose changes affect the result.
    pub fn watched(&self) -> &[PathBuf] {
        &self.watched
    }

    /// The value an output strategy receives.
    ///
    /// # Errors
    /// [`Error::NoRootFound`] for [`OutputKind::Full`] without root document.
    pub fn render(&self, kind: OutputKind) -> Result<Value> {
        if let Some(value) = self.rendered.get(&kind) {
            return Ok(value.clone());
        }
        self.render_with_diagnostics(kind).map(|(value, _)| value)
    }

    /// The value an output strategy receives and what it could not express
    /// (e.g. constructs an OpenAPI 3.0 export approximates).
    ///
    /// # Errors
    /// [`Error::NoRootFound`] for outputs that need a root document.
    pub fn render_with_diagnostics(&self, kind: OutputKind) -> Result<(Value, Diagnostics)> {
        let input = output::RenderInput {
            document: &self.document,
            canonical: &self.canonical,
            asyncapi: &self.asyncapi,
            registry: &self.registry,
        };
        let mut diagnostics = Diagnostics::new();
        let value = output::render(kind, &input, &mut diagnostics)?;
        Ok((value, diagnostics))
    }

    /// Writes every configured output (skipping files whose content is
    /// unchanged) and returns the configured paths.
    ///
    /// # Errors
    /// [`Error::NoRootFound`] or [`Error::Io`].
    pub fn write(&self) -> Result<Vec<PathBuf>> {
        let mut rendered = Vec::new();
        for (kind, path) in &self.outputs {
            let value = self.render(*kind)?;
            rendered.push((path, Format::of(path).render(&value)));
        }
        for (path, text) in &rendered {
            output::write_if_changed(path, text)?;
        }
        Ok(rendered.into_iter().map(|(p, _)| p.clone()).collect())
    }

    /// Prints diagnostics as `cargo:warning=` and the inputs as
    /// `cargo:rerun-if-changed=` (for build scripts).
    pub fn print_cargo_directives(&self) {
        for path in &self.watched {
            println!("cargo:rerun-if-changed={}", path.display());
        }
        for diagnostic in &self.diagnostics {
            if diagnostic.severity() != Severity::Note {
                println!("cargo:warning={diagnostic}");
            }
        }
    }
}

/// A parsed document and where it came from.
struct Snippet {
    value: Value,
    location: Location,
    /// Part of the AsyncAPI document instead of the OpenAPI document.
    asyncapi: bool,
}

impl Snippet {
    fn openapi(value: Value, location: Location) -> Self {
        Self {
            value,
            location,
            asyncapi: false,
        }
    }
}

/// A piece of text that still has to go through the lexical stage.
enum Pending {
    Document {
        text: String,
        location: Location,
        tags: Vec<String>,
    },
    NamedSchema {
        name: String,
        text: String,
        location: Location,
    },
    Route(Box<extract::RouteItem>),
    File {
        text: String,
        location: Location,
    },
    AsyncApi {
        text: String,
        location: Location,
    },
    Channel(Box<extract::ChannelItem>),
}

/// Runs every stage on `sources`; returns the merged document and registry.
/// The settings of a pipeline run.
#[derive(Clone, Copy)]
pub(crate) struct Settings<'a> {
    pub(crate) variables: &'a Variables,
    pub(crate) types: &'a TypeMapper,
    pub(crate) infer: bool,
}

pub(crate) fn run(
    sources: &SourceSet,
    settings: Settings<'_>,
    diagnostics: &mut Diagnostics,
) -> Result<Documents> {
    let Settings { types, .. } = settings;
    let mut registry = Registry::new();
    let mut patterns = BTreeMap::new();
    let mut known = KnownSchemas::default();
    let mut snippets: Vec<Snippet> = Vec::new();
    let mut pending: Vec<(usize, Pending)> = Vec::new();

    // Pass 1: extraction and template registration.
    for file in sources.files() {
        match file.kind() {
            SourceKind::Rust => {
                for item in extract::extract(file, types, diagnostics)? {
                    if let Item::Pattern(pattern) = item {
                        patterns.insert(pattern.name, pattern.pattern);
                        continue;
                    }
                    register(
                        item,
                        &mut registry,
                        &mut known,
                        &mut snippets,
                        &mut pending,
                        diagnostics,
                    );
                }
            }
            SourceKind::Yaml | SourceKind::Json => {
                let location = file.location(1);
                if let Ok(mut value) = yaml::parse(file.text()) {
                    registry.import(&mut value, &location, &mut Diagnostics::new());
                    known.collect_from(&value);
                }
                pending.push((
                    snippets.len(),
                    Pending::File {
                        text: file.text().to_string(),
                        location,
                    },
                ));
            }
        }
    }

    // Pass 2: lexical expansion, parsing, `@extend`, DSL.
    let mut ordered: Vec<(usize, Snippet)> = snippets
        .into_iter()
        .enumerate()
        .map(|(i, s)| (i * 2, s))
        .collect();
    for (position, item) in pending {
        if let Some(snippet) = materialize(item, &registry, settings, diagnostics)? {
            known.collect_from(&snippet.value);
            ordered.push((position * 2 + 1, snippet));
        }
    }
    ordered.sort_by_key(|(position, _)| *position);
    let mut snippets: Vec<Snippet> = ordered.into_iter().map(|(_, s)| s).collect();

    // Pass 3: generic instantiation.
    let mut instantiator = Instantiator::new(&registry);
    for snippet in &mut snippets {
        instantiator.apply(&mut snippet.value, &snippet.location, diagnostics);
    }
    let concrete = instantiator.into_concrete();
    for name in concrete.keys() {
        known.insert(name.clone());
    }

    // Pass 4: smart references and pattern constants.
    for snippet in &mut snippets {
        let targets = Targets {
            known: &known,
            registry: &registry,
            types,
        };
        refs::resolve(&mut snippet.value, targets, &snippet.location, diagnostics);
        resolve_patterns(
            &mut snippet.value,
            &patterns,
            &snippet.location,
            diagnostics,
        );
    }
    let mut generated = Vec::new();
    for (name, (mut body, location)) in concrete {
        let targets = Targets {
            known: &known,
            registry: &registry,
            types,
        };
        refs::resolve(&mut body, targets, &location, diagnostics);
        resolve_patterns(&mut body, &patterns, &location, diagnostics);
        generated.push(Snippet::openapi(
            json!({ "components": { "schemas": { name: body } } }),
            location,
        ));
    }

    // Pass 5: merge.
    let (asyncapi, snippets): (Vec<Snippet>, Vec<Snippet>) =
        snippets.into_iter().partition(|s| s.asyncapi);
    let mut asyncapi = merge_documents(asyncapi, Vec::new(), is_asyncapi_root, diagnostics)?;
    refs::unescape(&mut asyncapi);
    let mut document = merge_documents(snippets, generated, output::is_root, diagnostics)?;
    refs::unescape(&mut document);
    output::strip_transport(&mut document);
    normalize(&mut document);
    dsl::expand::expand_parameters(&mut document, diagnostics);
    let canonical = document.clone();
    if let Some(dialect) = Dialect::of_document(&document) {
        dialect::lower(&mut document, dialect, diagnostics);
    }
    lint::check(&document, diagnostics);
    if !asyncapi.as_object().is_some_and(Map::is_empty) {
        output::asyncapi::add_shared_schemas(&mut asyncapi, &canonical);
        lint::check(&asyncapi, diagnostics);
    }
    Ok(Documents {
        document,
        canonical,
        asyncapi,
        registry,
    })
}

/// Whether a snippet is the root of the AsyncAPI document.
fn is_asyncapi_root(value: &Value) -> bool {
    value
        .as_object()
        .is_some_and(|m| m.contains_key("asyncapi") && m.contains_key("info"))
}

/// The documents of a pipeline run.
pub(crate) struct Documents {
    /// The merged document in the dialect of its root.
    pub(crate) document: Value,
    /// The merged document before version-specific lowering.
    pub(crate) canonical: Value,
    /// The merged AsyncAPI document (empty without `@asyncapi`/`@channel`).
    pub(crate) asyncapi: Value,
    /// The templates of the run.
    pub(crate) registry: Registry,
}

fn register(
    item: Item,
    registry: &mut Registry,
    known: &mut KnownSchemas,
    snippets: &mut Vec<Snippet>,
    pending: &mut Vec<(usize, Pending)>,
    diagnostics: &mut Diagnostics,
) {
    match item {
        Item::Schema(schema) => {
            if known.contains(&schema.name) {
                diagnostics.push(
                    Diagnostic::new(
                        Code::DuplicateSchema,
                        format!(
                            "schema `{}` is defined more than once; the definitions are merged",
                            schema.name
                        ),
                    )
                    .at(schema.location.clone()),
                );
            }
            known.insert(schema.name.clone());
            registry.add_alias(&schema.rust_name, &schema.name);
            snippets.push(Snippet::openapi(
                json!({ "components": { "schemas": { schema.name: schema.schema } } }),
                schema.location,
            ));
        }
        Item::Blueprint(blueprint) => {
            registry.add_alias(&blueprint.rust_name, &blueprint.name);
            registry.add_blueprint(
                blueprint.name,
                Blueprint {
                    params: blueprint.params,
                    schema: blueprint.schema,
                },
            );
        }
        Item::Fragment(fragment) => registry.add_fragment(
            fragment.name,
            Fragment {
                params: fragment.params,
                body: fragment.body,
            },
        ),
        Item::Document(document) => pending.push((
            snippets.len(),
            Pending::Document {
                text: document.text,
                location: document.location,
                tags: document.tags,
            },
        )),
        Item::NamedSchema(named) => {
            known.insert(named.name.clone());
            pending.push((
                snippets.len(),
                Pending::NamedSchema {
                    name: named.name,
                    text: named.text,
                    location: named.location,
                },
            ));
        }
        Item::Route(route) => pending.push((snippets.len(), Pending::Route(Box::new(route)))),
        Item::AsyncApi(document) => pending.push((
            snippets.len(),
            Pending::AsyncApi {
                text: document.text,
                location: document.location,
            },
        )),
        Item::Channel(channel) => {
            pending.push((snippets.len(), Pending::Channel(Box::new(channel))));
        }
        // Collected by the caller before registration.
        Item::Pattern(_) => {}
    }
}

/// Lexical expansion and parsing of one pending text.
fn materialize(
    pending: Pending,
    registry: &Registry,
    settings: Settings<'_>,
    diagnostics: &mut Diagnostics,
) -> Result<Option<Snippet>> {
    let Settings {
        variables,
        types,
        infer,
    } = settings;
    match pending {
        Pending::Document {
            text,
            location,
            tags,
        } => {
            let Some(mut value) = parse_text(&text, registry, variables, &location, diagnostics)?
            else {
                return Ok(None);
            };
            apply_tags(&mut value, &tags);
            Ok(Some(Snippet::openapi(value, location)))
        }
        Pending::NamedSchema {
            name,
            text,
            location,
        } => {
            let Some(value) = parse_any(&text, registry, variables, &location, diagnostics)? else {
                return Ok(None);
            };
            Ok(Some(Snippet::openapi(
                json!({ "components": { "schemas": { name: value } } }),
                location,
            )))
        }
        Pending::File { text, location } => {
            let Some(mut value) = parse_text(&text, registry, variables, &location, diagnostics)?
            else {
                return Ok(None);
            };
            output::strip_transport(&mut value);
            // A file with an `asyncapi` root belongs to the AsyncAPI document.
            let asyncapi = value.get("asyncapi").is_some();
            Ok(Some(Snippet {
                value,
                location,
                asyncapi,
            }))
        }
        Pending::AsyncApi { text, location } => {
            let Some(value) = parse_text(&text, registry, variables, &location, diagnostics)?
            else {
                return Ok(None);
            };
            Ok(Some(Snippet {
                value,
                location,
                asyncapi: true,
            }))
        }
        Pending::Channel(channel) => {
            let text = channel.lines.join("\n");
            let expanded =
                lexical::expand(&text, registry, variables, &channel.location, diagnostics);
            let lines: Vec<String> = expanded.lines().map(str::to_string).collect();
            let compiled = dsl::compile_channel(
                dsl::ChannelSource::new(&lines, &channel.location)
                    .line_numbers(&channel.line_numbers)
                    .operation_id(channel.operation_id.as_deref())
                    .types(types),
                diagnostics,
            );
            let Some(compiled) = compiled else {
                return Ok(None);
            };
            let mut value = compiled.document;
            extend::apply(&mut value, registry, &channel.location, diagnostics);
            Ok(Some(Snippet {
                value,
                location: channel.location,
                asyncapi: true,
            }))
        }
        Pending::Route(route) => {
            let text = route.lines.join("\n");
            let expanded =
                lexical::expand(&text, registry, variables, &route.location, diagnostics);
            let lines: Vec<String> = expanded.lines().map(str::to_string).collect();
            let compiled = dsl::compile(
                RouteSource::new(&lines, &route.location)
                    .line_numbers(&route.line_numbers)
                    .operation_id(route.operation_id.as_deref())
                    .default_tags(&route.tags)
                    .types(types)
                    .defaults(&route.defaults)
                    .signature(route.signature.as_ref().filter(|_| infer)),
                diagnostics,
            );
            let Some(compiled) = compiled else {
                return Ok(None);
            };
            let mut value = compiled.document;
            extend::apply(&mut value, registry, &route.location, diagnostics);
            Ok(Some(Snippet::openapi(value, route.location)))
        }
    }
}

/// Expands and parses a document text; it must be a mapping (or empty).
fn parse_text(
    text: &str,
    registry: &Registry,
    variables: &Variables,
    location: &Location,
    diagnostics: &mut Diagnostics,
) -> Result<Option<Value>> {
    let Some(value) = parse_any(text, registry, variables, location, diagnostics)? else {
        return Ok(None);
    };
    let found = match &value {
        Value::Object(_) => return Ok(Some(value)),
        Value::Bool(_) => "a boolean",
        Value::Number(_) => "a number",
        Value::String(_) => "a string",
        _ => "a sequence",
    };
    Err(Error::Yaml {
        location: location.clone(),
        message: format!("an OpenAPI snippet must be a mapping at the top level, found {found}"),
        context: context(text, location, 1),
    })
}

/// Expands and parses any YAML value; `None` for empty documents.
fn parse_any(
    text: &str,
    registry: &Registry,
    variables: &Variables,
    location: &Location,
    diagnostics: &mut Diagnostics,
) -> Result<Option<Value>> {
    let expanded = lexical::expand(text, registry, variables, location, diagnostics);
    let mut value = yaml::parse(&expanded).map_err(|e| Error::Yaml {
        location: location.offset(e.line - 1),
        message: e.message,
        context: context(&expanded, location, e.line),
    })?;
    if value.is_null() {
        return Ok(None);
    }
    extend::apply(&mut value, registry, location, diagnostics);
    Ok(Some(value))
}

/// Up to five lines around line `focus` (1-based) of `text`, numbered as in
/// the source file that starts at `location`; the focus line is marked.
fn context(text: &str, location: &Location, focus: usize) -> String {
    let first = focus.saturating_sub(3);
    text.lines()
        .enumerate()
        .skip(first)
        .take(5)
        .map(|(i, line)| {
            let marker = if i + 1 == focus { '>' } else { ' ' };
            format!("  {marker} {:02} | {line}", location.line() + i)
        })
        .collect::<Vec<_>>()
        .join("\n")
}

/// Module tags for operations of a document that have no tags.
fn apply_tags(document: &mut Value, tags: &[String]) {
    if tags.is_empty() {
        return;
    }
    for_each_operation(document, |_, operation| {
        if !operation.contains_key("tags") {
            let mut with_tags = Map::with_capacity(operation.len() + 1);
            with_tags.insert("tags".into(), json!(tags));
            with_tags.extend(std::mem::take(operation));
            *operation = with_tags;
        }
    });
}

/// Merges all snippets onto the root document (exactly one root allowed).
fn merge_documents(
    snippets: Vec<Snippet>,
    generated: Vec<Snippet>,
    is_root: fn(&Value) -> bool,
    diagnostics: &mut Diagnostics,
) -> Result<Value> {
    let roots: Vec<Location> = snippets
        .iter()
        .filter(|s| is_root(&s.value))
        .map(|s| s.location.clone())
        .collect();
    if roots.len() > 1 {
        return Err(Error::MultipleRootsFound { locations: roots });
    }
    let (root, others): (Vec<Snippet>, Vec<Snippet>) =
        snippets.into_iter().partition(|s| is_root(&s.value));
    let mut document = root
        .into_iter()
        .next()
        .map_or_else(|| Value::Object(Map::new()), |s| s.value);
    for snippet in others.into_iter().chain(generated) {
        for pointer in merge(&mut document, snippet.value, Policy::Document) {
            diagnostics.push(
                Diagnostic::new(
                    Code::MergeConflict,
                    format!("the value at `{pointer}` is set differently by several annotations; the later one wins"),
                )
                .at(snippet.location.clone()),
            );
        }
    }
    Ok(document)
}

/// Final clean-ups of the merged document.
fn normalize(document: &mut Value) {
    // `tags: [A, B]` declared on modules: tag objects are required at the top level.
    if let Some(Value::Array(tags)) = document.get_mut("tags") {
        for tag in tags.iter_mut() {
            if let Value::String(name) = tag {
                *tag = json!({ "name": name });
            }
        }
        let mut seen = std::collections::HashSet::new();
        tags.retain(|t| seen.insert(t.get("name").map(Value::to_string).unwrap_or_default()));
    }
    remove_extend_markers(document);
}

fn remove_extend_markers(value: &mut Value) {
    match value {
        Value::Object(map) => {
            map.retain(|k, _| !k.starts_with(EXTEND_KEY));
            map.values_mut().for_each(remove_extend_markers);
        }
        Value::Array(items) => items.iter_mut().for_each(remove_extend_markers),
        _ => {}
    }
}

/// Runs the pipeline on in-memory sources (for tests and tools).
pub fn generate_from_sources(
    files: Vec<SourceFile>,
    variables: &Variables,
) -> Result<(Value, Diagnostics)> {
    let mut diagnostics = Diagnostics::new();
    let settings = Settings {
        variables,
        types: TypeMapper::builtin(),
        infer: true,
    };
    let documents = run(&SourceSet::from_files(files), settings, &mut diagnostics)?;
    Ok((documents.document, diagnostics))
}
