//! The generator: configuration, the pipeline run and its result.

use super::attrs::validate::resolve_patterns;
use super::config::Config;
use super::diagnostics::{Code, Diagnostic, Diagnostics, Location, Severity};
use super::document::for_each_operation;
use super::dsl::{self, RouteSource};
use super::error::{Error, Result};
use super::extract::{self, Item};
use super::lint::{self, Lints};
use super::merge::{Policy, merge};
use super::output::{self, Format, OutputKind};
use super::package::Package;
use super::schema::TypeMapper;
use super::schema::dialect::{self, Dialect};
use super::source::{DEFAULT_MAX_FILE_SIZE, DiscoverOptions, SourceFile, SourceKind, SourceSet};
use super::template::generics::Instantiator;
use super::template::lexical::{self, EXTEND_KEY, Variables};
use super::template::refs::{self, KnownSchemas, Targets};
use super::template::{Blueprint, Fragment, Registry, extend};
use super::yaml;
use serde_json::{Map, Value, json};
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

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
    package: Option<Package>,
    strict_lints: bool,
    /// Configuration files the settings were read from (watched by Cargo).
    config_files: Vec<PathBuf>,
}

/// The package variables Cargo sets for build scripts.
const PACKAGE_VARIABLES: &[&str] = &[
    "CARGO_PKG_NAME",
    "CARGO_PKG_VERSION",
    "CARGO_PKG_VERSION_MAJOR",
    "CARGO_PKG_VERSION_MINOR",
    "CARGO_PKG_VERSION_PATCH",
    "CARGO_PKG_DESCRIPTION",
    "CARGO_PKG_AUTHORS",
    "CARGO_PKG_LICENSE",
    "CARGO_PKG_REPOSITORY",
    "CARGO_PKG_HOMEPAGE",
    "CARGO_PKG_RUST_VERSION",
];

impl Default for Generator {
    fn default() -> Self {
        // Cargo sets `CARGO_PKG_*` for build scripts; they are available as
        // `{{CARGO_PKG_NAME}}` and so on.
        let mut variables = Variables::new();
        for name in PACKAGE_VARIABLES {
            if let Ok(value) = std::env::var(name) {
                variables.insert((*name).to_string(), value);
            }
        }
        variables
            .entry("CARGO_PKG_VERSION".into())
            .or_insert_with(|| "0.0.0".into());
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
            package: Package::from_build_env(),
            strict_lints: false,
            config_files: Vec::new(),
        }
    }
}

impl Generator {
    /// A generator without inputs and outputs.
    pub fn new() -> Self {
        Self::default()
    }

    /// Adds the inputs and outputs of `config`; the files it was read from
    /// are watched like inputs ([`Spec::watched`]).
    pub fn with_config(mut self, config: Config) -> Self {
        for file in config.files() {
            if !self.config_files.contains(file) {
                self.config_files.push(file.clone());
            }
        }
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
            variables,
            ..
        } = config;
        for (name, schema) in types.unwrap_or_default() {
            self.types = self.types.with_type(name, schema);
        }
        self.variables.extend(variables.unwrap_or_default());
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

    /// Uses `package` for the root document when the sources define none
    /// and for the `{{CARGO_PKG_*}}` variables ([`Package::variables`]);
    /// [`Generator::var`] calls after this one win. In a build script the
    /// running package is used by default.
    pub fn package(mut self, package: Package) -> Self {
        self.variables.extend(
            package
                .variables()
                .into_iter()
                .map(|(name, value)| (name.to_string(), value)),
        );
        self.package = Some(package);
        self
    }

    /// Never generates a root document from package metadata.
    pub fn without_package(mut self) -> Self {
        self.package = None;
        self
    }

    /// Also reports unused components and operations or schemas without
    /// description.
    pub fn strict_lints(mut self, strict: bool) -> Self {
        self.strict_lints = strict;
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
            package: self.package.as_ref(),
            strict_lints: self.strict_lints,
        };
        let documents = run(&sources, settings, &mut diagnostics)?;
        let mut spec = Spec {
            document: documents.document,
            canonical: documents.canonical,
            generated_keys: documents.generated_keys,
            asyncapi: documents.asyncapi,
            registry: documents.registry,
            outputs: self.outputs.clone(),
            diagnostics,
            watched: sources
                .watched()
                .iter()
                .chain(&self.config_files)
                .cloned()
                .collect(),
            watched_env: sources
                .files()
                .iter()
                .flat_map(|file| lexical::environment_variables(file.text()))
                .map(str::to_string)
                .collect::<BTreeSet<_>>()
                .into_iter()
                .collect(),
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
    /// `None` when equal to `document`.
    canonical: Option<Value>,
    generated_keys: Vec<String>,
    asyncapi: Value,
    registry: Registry,
    outputs: Vec<(OutputKind, PathBuf)>,
    diagnostics: Diagnostics,
    watched: Vec<PathBuf>,
    watched_env: Vec<String>,
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

    /// Inputs and configuration files ([`Config::files`]) whose changes
    /// affect the result.
    pub fn watched(&self) -> &[PathBuf] {
        &self.watched
    }

    /// Environment variables the inputs read with `{{env:NAME}}`, sorted.
    pub fn watched_env(&self) -> &[String] {
        &self.watched_env
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
            canonical: self.canonical.as_ref().unwrap_or(&self.document),
            generated_keys: &self.generated_keys,
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
            rendered.push((path, self.text(*kind, path)?));
        }
        for (path, text) in &rendered {
            output::write_if_changed(path, text)?;
        }
        Ok(rendered.into_iter().map(|(p, _)| p.clone()).collect())
    }

    /// The configured output files whose content differs from what
    /// [`Spec::write`] would write (missing files included), e.g. for a CI
    /// check that the committed specification is up to date. Line endings
    /// do not count: a file checked out with `\r\n` is up to date.
    ///
    /// # Errors
    /// [`Error::NoRootFound`] or [`Error::NoAsyncApiRoot`] for outputs that
    /// cannot be rendered.
    pub fn check(&self) -> Result<Vec<PathBuf>> {
        let mut outdated = Vec::new();
        for (kind, path) in &self.outputs {
            if !output::is_current(path, &self.text(*kind, path)?) {
                outdated.push(path.clone());
            }
        }
        Ok(outdated)
    }

    /// The text the output file `path` of `kind` receives.
    fn text(&self, kind: OutputKind, path: &Path) -> Result<String> {
        let format = Format::of(path);
        match self.rendered.get(&kind) {
            Some(value) => Ok(format.render(value)),
            None => Ok(format.render(&self.render_with_diagnostics(kind)?.0)),
        }
    }

    /// Prints the [cargo directives](Spec::write_cargo_directives) of a
    /// build script to standard output.
    pub fn print_cargo_directives(&self) {
        // Cargo reads the directives from standard output; nothing to do if
        // it is closed.
        let _ = self.write_cargo_directives(&mut std::io::stdout().lock());
    }

    /// Writes the inputs as `cargo:rerun-if-changed=`, the environment
    /// variables they read as `cargo:rerun-if-env-changed=` and the warnings
    /// and errors as `cargo:warning=`, one per line.
    ///
    /// # Errors
    /// Errors of `out`.
    pub fn write_cargo_directives(&self, out: &mut impl std::io::Write) -> std::io::Result<()> {
        for path in &self.watched {
            writeln!(out, "cargo:rerun-if-changed={}", path.display())?;
        }
        for name in &self.watched_env {
            writeln!(out, "cargo:rerun-if-env-changed={name}")?;
        }
        for diagnostic in &self.diagnostics {
            if diagnostic.severity() != Severity::Note {
                writeln!(out, "cargo:warning={diagnostic}")?;
            }
        }
        Ok(())
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

/// The settings of a pipeline run.
#[derive(Clone, Copy)]
pub(crate) struct Settings<'a> {
    pub(crate) variables: &'a Variables,
    pub(crate) types: &'a TypeMapper,
    pub(crate) infer: bool,
    pub(crate) package: Option<&'a Package>,
    pub(crate) strict_lints: bool,
}

/// Runs every stage on `sources`; returns the merged documents and registry.
pub(crate) fn run(
    sources: &SourceSet,
    settings: Settings<'_>,
    diagnostics: &mut Diagnostics,
) -> Result<Documents> {
    let Settings {
        types,
        variables,
        package,
        ..
    } = settings;
    // Pass 1: extraction and template registration.
    let mut collected = Collected::default();
    for file in sources.files() {
        match file.kind() {
            SourceKind::Rust => {
                for item in extract::extract(
                    file,
                    extract::Options::new().types(types).variables(variables),
                    diagnostics,
                )? {
                    collected.register(item, diagnostics);
                }
            }
            SourceKind::Yaml | SourceKind::Json => {
                let location = file.location(1);
                if let Ok(mut value) = yaml::parse(file.text()) {
                    collected
                        .registry
                        .import(&mut value, &location, diagnostics);
                    collected.known.collect_from(&value);
                }
                collected.pending.push((
                    collected.snippets.len(),
                    Pending::File {
                        text: file.text().to_string(),
                        location,
                    },
                ));
            }
        }
    }
    let Collected {
        registry,
        patterns,
        mut known,
        snippets,
        pending,
    } = collected;

    // Pass 2: lexical expansion, parsing, `@extend`, DSL.
    let mut ordered: Vec<(usize, Snippet)> = snippets
        .into_iter()
        .enumerate()
        .map(|(i, s)| (i * 2, s))
        .collect();
    let mut defaults = DefaultsCache::new();
    for (position, item) in pending {
        if let Some(snippet) = materialize(item, &registry, settings, &mut defaults, diagnostics)? {
            known.collect_from(&snippet.value);
            ordered.push((position * 2 + 1, snippet));
        }
    }
    ordered.sort_by_key(|(position, _)| *position);
    let mut snippets: Vec<Snippet> = ordered.into_iter().map(|(_, s)| s).collect();

    // Pass 3: generic instantiation.
    let mut instantiator = Instantiator::new(&registry).with_types(types);
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
    let mut generated_keys = Vec::new();
    if !output::is_root(&document)
        && let Some(package) = package
    {
        let mut root = package.root_document();
        // What only the generated root defines; the fragments output, which
        // travels to other crates, leaves it out.
        generated_keys = root
            .as_object()
            .into_iter()
            .flat_map(Map::keys)
            .filter(|key| document.get(key.as_str()).is_none())
            .cloned()
            .collect();
        merge(&mut root, document, Policy::Document);
        document = root;
        diagnostics.push(Diagnostic::new(
            Code::GeneratedRoot,
            format!(
                "no `openapi`/`info` root in the sources; it was generated from package `{}`",
                package.name
            ),
        ));
    }
    refs::unescape(&mut document);
    output::strip_transport(&mut document);
    normalize(&mut document, diagnostics);
    dsl::expand::expand_parameters(&mut document, diagnostics);
    // OpenAPI 3.2 is the canonical dialect: only older roots need a copy.
    let canonical = match Dialect::of_document(&document) {
        None | Some(Dialect::OpenApi32) => None,
        Some(dialect) => {
            let canonical = document.clone();
            dialect::lower(&mut document, dialect, diagnostics);
            Some(canonical)
        }
    };
    let lints = Lints {
        strict: settings.strict_lints,
    };
    lint::check(&document, lints, diagnostics);
    if let Value::Object(map) = &mut asyncapi
        && !map.is_empty()
    {
        output::asyncapi::add_shared_schemas(map, canonical.as_ref().unwrap_or(&document));
        lint::check(&asyncapi, Lints::default(), diagnostics);
    }
    Ok(Documents {
        document,
        canonical,
        generated_keys,
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
    /// The merged document before version-specific lowering; `None` when
    /// it equals `document` (OpenAPI 3.2 and documents without root).
    pub(crate) canonical: Option<Value>,
    /// Top-level keys that only the root generated from the package defines.
    pub(crate) generated_keys: Vec<String>,
    /// The merged AsyncAPI document (empty without `@asyncapi`/`@channel`).
    pub(crate) asyncapi: Value,
    /// The templates of the run.
    pub(crate) registry: Registry,
}

/// What pass 1 collects.
#[derive(Default)]
struct Collected {
    registry: Registry,
    /// Regular expression constants by name.
    patterns: BTreeMap<String, String>,
    known: KnownSchemas,
    /// Snippets that are complete after extraction (reflected schemas).
    snippets: Vec<Snippet>,
    /// Texts that are expanded once every template is known, with the
    /// position among `snippets` they belong to.
    pending: Vec<(usize, Pending)>,
}

impl Collected {
    fn register(&mut self, item: Item, diagnostics: &mut Diagnostics) {
        let Self {
            registry,
            patterns,
            known,
            snippets,
            pending,
        } = self;
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
            Item::Pattern(pattern) => {
                patterns.insert(pattern.name, pattern.pattern);
            }
        }
    }
}

/// Expanded `@route-defaults` blocks by file and header line.
type DefaultsCache = BTreeMap<(PathBuf, usize), Vec<String>>;

/// Lexical expansion and parsing of one pending text.
fn materialize(
    pending: Pending,
    registry: &Registry,
    settings: Settings<'_>,
    defaults_cache: &mut DefaultsCache,
    diagnostics: &mut Diagnostics,
) -> Result<Option<Snippet>> {
    let Settings {
        variables,
        types,
        infer,
        ..
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
                lexical::expand_lines(&text, registry, variables, &channel.location, diagnostics);
            let line_numbers = source_lines(&expanded, &channel.line_numbers, &channel.location);
            let lines: Vec<String> = expanded.into_iter().map(|(line, _)| line).collect();
            let compiled = dsl::compile_channel(
                dsl::ChannelSource::new(&lines, &channel.location)
                    .line_numbers(&line_numbers)
                    .operation_id(channel.operation_id.as_deref())
                    .types(types),
                diagnostics,
            );
            let Some(compiled) = compiled else {
                return Ok(None);
            };
            let mut value = compiled.document;
            extend::apply(
                &mut value,
                registry,
                variables,
                &channel.location,
                diagnostics,
            );
            Ok(Some(Snippet {
                value,
                location: channel.location,
                asyncapi: true,
            }))
        }
        Pending::Route(route) => {
            let text = route.lines.join("\n");
            let expanded =
                lexical::expand_lines(&text, registry, variables, &route.location, diagnostics);
            let line_numbers = source_lines(&expanded, &route.line_numbers, &route.location);
            let lines: Vec<String> = expanded.into_iter().map(|(line, _)| line).collect();
            let defaults =
                expand_defaults(&route, registry, variables, defaults_cache, diagnostics);
            let compiled = dsl::compile(
                RouteSource::new(&lines, &route.location)
                    .line_numbers(&line_numbers)
                    .operation_id(route.operation_id.as_deref())
                    .default_tags(&route.tags)
                    .types(types)
                    .defaults(&defaults)
                    .signature(route.signature.as_ref().filter(|_| infer)),
                diagnostics,
            );
            let Some(compiled) = compiled else {
                return Ok(None);
            };
            let mut value = compiled.document;
            extend::apply(
                &mut value,
                registry,
                variables,
                &route.location,
                diagnostics,
            );
            Ok(Some(Snippet::openapi(value, route.location)))
        }
    }
}

/// The expanded `@route-defaults` blocks of a route. Each module's block is
/// expanded once for all routes of the module, so its findings are reported
/// once, at its own lines.
fn expand_defaults(
    route: &extract::RouteItem,
    registry: &Registry,
    variables: &Variables,
    cache: &mut DefaultsCache,
    diagnostics: &mut Diagnostics,
) -> Vec<String> {
    let mut starts: Vec<usize> = route
        .defaults
        .iter()
        .enumerate()
        .filter(|(i, line)| *i == 0 || line.trim() == "@route-defaults")
        .map(|(i, _)| i)
        .collect();
    starts.push(route.defaults.len());
    let mut expanded = Vec::new();
    for range in starts.windows(2) {
        let (start, end) = match range {
            [start, end] => (*start, *end),
            _ => continue,
        };
        let block = route.defaults.get(start..end).unwrap_or_default();
        let line = route.default_line_numbers.get(start).copied();
        let location = line.map_or_else(
            || route.location.clone(),
            |line| Location::new(route.location.file(), line),
        );
        let mut expand = || -> Vec<String> {
            let text = block.join("\n");
            let text = lexical::expand(&text, registry, variables, &location, diagnostics);
            text.lines().map(str::to_string).collect()
        };
        match line {
            Some(line) => {
                let key = (route.location.file().to_path_buf(), line);
                expanded.extend(cache.entry(key).or_insert_with(expand).iter().cloned());
            }
            None => expanded.extend(expand()),
        }
    }
    expanded
}

/// The source line of each expanded line (`lexical::expand_lines` offsets
/// into the block's `line_numbers`).
fn source_lines(expanded: &[(String, usize)], numbers: &[usize], at: &Location) -> Vec<usize> {
    expanded
        .iter()
        .map(|(_, offset)| numbers.get(*offset).copied().unwrap_or_else(|| at.line()))
        .collect()
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
    extend::apply(&mut value, registry, variables, location, diagnostics);
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
fn normalize(document: &mut Value, diagnostics: &mut Diagnostics) {
    if let Some(Value::Array(tags)) = document.get_mut("tags") {
        *tags = merge_tags(std::mem::take(tags), diagnostics);
    }
    remove_extend_markers(document);
}

/// Top-level tags as unique objects: `tags: [A, B]` declared on modules
/// become `{ name: A }`, and definitions of the same name are merged in
/// order (later values win, conflicts are reported).
fn merge_tags(tags: Vec<Value>, diagnostics: &mut Diagnostics) -> Vec<Value> {
    let mut merged: Vec<Value> = Vec::with_capacity(tags.len());
    for tag in tags {
        let tag = match tag {
            Value::String(name) => json!({ "name": name }),
            other => other,
        };
        if !tag.is_object() {
            if !merged.contains(&tag) {
                merged.push(tag);
            }
            continue;
        }
        let name = tag.get("name").cloned();
        let existing = merged
            .iter_mut()
            .find(|t| t.is_object() && t.get("name") == name.as_ref());
        let Some(existing) = existing else {
            merged.push(tag);
            continue;
        };
        let label = name.as_ref().map_or_else(String::new, |n| {
            n.as_str().map_or_else(|| n.to_string(), str::to_string)
        });
        for pointer in merge(existing, tag, Policy::Document) {
            diagnostics.push(Diagnostic::new(
                Code::MergeConflict,
                format!(
                    "the value at `/tags/{label}{pointer}` is set differently by several annotations; the later one wins"
                ),
            ));
        }
    }
    merged
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
        package: None,
        strict_lints: false,
    };
    let documents = run(&SourceSet::from_files(files), settings, &mut diagnostics)?;
    Ok((documents.document, diagnostics))
}
