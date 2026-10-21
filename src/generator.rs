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
use super::proto::ProtoIndex;
use super::schema::TypeMapper;
use super::schema::dialect::{self, Dialect};
use super::scope;
use super::source::{DEFAULT_MAX_FILE_SIZE, DiscoverOptions, SourceFile, SourceKind, SourceSet};
use super::template::generics::Instantiator;
use super::template::lexical::{self, EXTEND_KEY, Variables};
use super::template::refs::{self, KnownSchemas, Targets};
use super::template::{Blueprint, Fragment, Registry, extend};
use super::yaml;
use serde_json::{Map, Value, json};
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::sync::LazyLock;

/// Builds OpenAPI documents from Rust sources.
///
/// # Examples
///
/// In a build script, with paths relative to the package root:
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
    /// Directories scanned recursively, in the order they were added.
    inputs: Vec<PathBuf>,
    /// Single YAML/JSON files, read after the directories.
    includes: Vec<PathBuf>,
    /// Output strategies and their files, in the order they were added,
    /// with the scope of [`Generator::output_scoped`] (`None`: the default
    /// scope).
    outputs: Vec<(Option<String>, OutputKind, PathBuf)>,
    /// The scope of [`Generator::scope`]: of the outputs without their own
    /// and of [`Spec::document`]; `None` merges every snippet.
    scope: Option<String>,
    /// Values of `{{NAME}}` placeholders, `CARGO_PKG_*` included.
    variables: Variables,
    /// Whether warnings are raised to errors after the run.
    deny_warnings: bool,
    /// The size limit per input file in bytes.
    max_file_size: u64,
    /// Whether [`Generator::generate`] prints cargo directives.
    cargo_directives: bool,
    /// The built-in type table plus the custom mappings.
    types: TypeMapper,
    /// Whether `@route` blocks are completed from handler signatures.
    infer: bool,
    /// The package a root document is generated from when the sources
    /// define none; `None` never generates one.
    package: Option<Package>,
    /// Whether the strict lints (unused components, missing descriptions)
    /// run.
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
            scope: None,
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
    ///
    /// # Examples
    ///
    /// ```
    /// use oas_forge::{Error, Generator};
    ///
    /// // Without an output there is nothing to do.
    /// assert!(matches!(Generator::new().generate(), Err(Error::NoOutput)));
    /// ```
    pub fn new() -> Self {
        Self::default()
    }

    /// Adds the inputs and outputs of `config`; the files it was read from
    /// are watched like inputs ([`Spec::watched`]).
    ///
    /// # Examples
    ///
    /// ```
    /// use oas_forge::{Config, Generator};
    /// use serde_json::json;
    ///
    /// let src = tempfile::tempdir()?;
    /// std::fs::write(src.path().join("lib.rs"), "/// @openapi\npub struct User { pub id: UserId }\n")?;
    /// let config = Config::from_toml_str(
    ///     r#"
    ///     [types]
    ///     UserId = { type = "string", format = "uuid" }
    ///     "#,
    /// )?;
    ///
    /// let spec = Generator::new()
    ///     .with_config(config)
    ///     .input(src.path())
    ///     .without_package()
    ///     .build()?;
    /// assert_eq!(
    ///     spec.document()["components"]["schemas"]["User"]["properties"]["id"],
    ///     json!({ "type": "string", "format": "uuid" })
    /// );
    /// # Ok::<(), Box<dyn std::error::Error>>(())
    /// ```
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
            scope,
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
            self.outputs.extend(
                paths
                    .unwrap_or_default()
                    .into_iter()
                    .map(|p| (None, kind, p)),
            );
        }
        if scope.is_some() {
            self.scope = scope;
        }
        self
    }

    /// Adds a directory that is scanned recursively for Rust (`.rs`), YAML
    /// (`.yaml`, `.yml`), JSON (`.json`) and Protocol Buffers (`.proto`)
    /// files; other files are ignored.
    ///
    /// # Examples
    ///
    /// ```
    /// use oas_forge::Generator;
    ///
    /// let src = tempfile::tempdir()?;
    /// std::fs::write(src.path().join("lib.rs"), "/// @openapi\npub struct Tag { pub name: String }\n")?;
    /// std::fs::write(src.path().join("notes.txt"), "not an input")?;
    ///
    /// let spec = Generator::new().input(src.path()).without_package().build()?;
    /// assert_eq!(spec.document()["components"]["schemas"]["Tag"]["type"], "object");
    /// # Ok::<(), Box<dyn std::error::Error>>(())
    /// ```
    pub fn input(mut self, path: impl Into<PathBuf>) -> Self {
        self.inputs.push(path.into());
        self
    }

    /// Adds a single YAML, JSON or `.proto` file. A missing file is reported
    /// ([`Code::MissingInclude`]) and skipped.
    ///
    /// # Examples
    ///
    /// ```
    /// use oas_forge::Generator;
    ///
    /// let dir = tempfile::tempdir()?;
    /// let root = dir.path().join("root.yaml");
    /// std::fs::write(&root, "openapi: 3.1.0\ninfo:\n  title: Shop\n  version: 1.0.0\n")?;
    ///
    /// let spec = Generator::new().include(&root).build()?;
    /// assert_eq!(spec.document()["info"]["title"], "Shop");
    /// # Ok::<(), Box<dyn std::error::Error>>(())
    /// ```
    pub fn include(mut self, path: impl Into<PathBuf>) -> Self {
        self.includes.push(path.into());
        self
    }

    /// Writes the full specification to `path`, as JSON for `.json` and as
    /// YAML otherwise ([`Format::of`]).
    ///
    /// # Examples
    ///
    /// ```
    /// use oas_forge::{Generator, Package};
    ///
    /// let src = tempfile::tempdir()?;
    /// std::fs::write(src.path().join("lib.rs"), "/// @openapi\npub struct Tag { pub name: String }\n")?;
    /// let out = tempfile::tempdir()?;
    /// let path = out.path().join("openapi.yaml");
    ///
    /// Generator::new()
    ///     .input(src.path())
    ///     .package(Package::new("shop", "1.0.0"))
    ///     .output(&path)
    ///     .generate()?;
    /// let text = std::fs::read_to_string(&path)?;
    /// assert!(text.starts_with("openapi: \"3.1.1\"\ninfo:\n  title: shop\n"));
    /// # Ok::<(), Box<dyn std::error::Error>>(())
    /// ```
    pub fn output(self, path: impl Into<PathBuf>) -> Self {
        self.output_as(OutputKind::Full, path)
    }

    /// Writes only `components.schemas` to `path`.
    ///
    /// # Examples
    ///
    /// ```
    /// use oas_forge::Generator;
    ///
    /// let src = tempfile::tempdir()?;
    /// std::fs::write(src.path().join("lib.rs"), "/// @openapi\npub struct Tag { pub name: String }\n")?;
    /// let out = tempfile::tempdir()?;
    /// let path = out.path().join("schemas.json");
    ///
    /// Generator::new().input(src.path()).output_schemas(&path).generate()?;
    /// let schemas: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(&path)?)?;
    /// assert_eq!(schemas["Tag"]["type"], "object");
    /// # Ok::<(), Box<dyn std::error::Error>>(())
    /// ```
    pub fn output_schemas(self, path: impl Into<PathBuf>) -> Self {
        self.output_as(OutputKind::Schemas, path)
    }

    /// Writes only `paths` to `path`.
    ///
    /// # Examples
    ///
    /// ```
    /// use oas_forge::Generator;
    ///
    /// let src = tempfile::tempdir()?;
    /// std::fs::write(
    ///     src.path().join("lib.rs"),
    ///     "/// List tags\n/// @route GET /tags\n/// @return 200: Vec<String>\npub async fn list_tags() {}\n",
    /// )?;
    /// let out = tempfile::tempdir()?;
    /// let path = out.path().join("paths.json");
    ///
    /// Generator::new().input(src.path()).output_paths(&path).generate()?;
    /// let paths: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(&path)?)?;
    /// assert_eq!(paths["/tags"]["get"]["operationId"], "list_tags");
    /// # Ok::<(), Box<dyn std::error::Error>>(())
    /// ```
    pub fn output_paths(self, path: impl Into<PathBuf>) -> Self {
        self.output_as(OutputKind::Paths, path)
    }

    /// Writes the headless specification for other crates to `path`: no
    /// `openapi`, `info` or `servers`, but the templates of this crate
    /// ([`OutputKind::Fragments`]).
    ///
    /// # Examples
    ///
    /// ```
    /// use oas_forge::{Generator, Package};
    ///
    /// let src = tempfile::tempdir()?;
    /// std::fs::write(src.path().join("lib.rs"), "/// @openapi\npub struct Tag { pub name: String }\n")?;
    /// let out = tempfile::tempdir()?;
    /// let path = out.path().join("fragments.json");
    ///
    /// Generator::new()
    ///     .input(src.path())
    ///     .package(Package::new("shop", "1.0.0"))
    ///     .output_fragments(&path)
    ///     .generate()?;
    /// let fragments: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(&path)?)?;
    /// assert!(fragments.get("openapi").is_none());
    /// assert_eq!(fragments["components"]["schemas"]["Tag"]["type"], "object");
    /// # Ok::<(), Box<dyn std::error::Error>>(())
    /// ```
    pub fn output_fragments(self, path: impl Into<PathBuf>) -> Self {
        self.output_as(OutputKind::Fragments, path)
    }

    /// Writes the component schemas as a JSON Schema 2020-12 bundle to `path`.
    ///
    /// # Examples
    ///
    /// ```
    /// use oas_forge::Generator;
    ///
    /// let src = tempfile::tempdir()?;
    /// std::fs::write(src.path().join("lib.rs"), "/// @openapi\npub struct Tag { pub name: String }\n")?;
    /// let out = tempfile::tempdir()?;
    /// let path = out.path().join("schemas.schema.json");
    ///
    /// Generator::new().input(src.path()).output_json_schema(&path).generate()?;
    /// let bundle: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(&path)?)?;
    /// assert_eq!(bundle["$schema"], "https://json-schema.org/draft/2020-12/schema");
    /// assert_eq!(bundle["$defs"]["Tag"]["type"], "object");
    /// # Ok::<(), Box<dyn std::error::Error>>(())
    /// ```
    pub fn output_json_schema(self, path: impl Into<PathBuf>) -> Self {
        self.output_as(OutputKind::JsonSchema, path)
    }

    /// Writes the specification lowered to OpenAPI 3.0.3 to `path`.
    ///
    /// # Examples
    ///
    /// ```
    /// use oas_forge::{Generator, Package};
    ///
    /// let src = tempfile::tempdir()?;
    /// std::fs::write(src.path().join("lib.rs"), "/// @openapi\npub struct Tag { pub name: String }\n")?;
    /// let out = tempfile::tempdir()?;
    /// let path = out.path().join("openapi-3.0.json");
    ///
    /// Generator::new()
    ///     .input(src.path())
    ///     .package(Package::new("shop", "1.0.0"))
    ///     .output_openapi_30(&path)
    ///     .generate()?;
    /// let document: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(&path)?)?;
    /// assert_eq!(document["openapi"], "3.0.3");
    /// # Ok::<(), Box<dyn std::error::Error>>(())
    /// ```
    pub fn output_openapi_30(self, path: impl Into<PathBuf>) -> Self {
        self.output_as(OutputKind::OpenApi30, path)
    }

    /// Writes the specification converted to Swagger 2.0 to `path`.
    ///
    /// # Examples
    ///
    /// ```
    /// use oas_forge::{Generator, Package};
    ///
    /// let src = tempfile::tempdir()?;
    /// std::fs::write(src.path().join("lib.rs"), "/// @openapi\npub struct Tag { pub name: String }\n")?;
    /// let out = tempfile::tempdir()?;
    /// let path = out.path().join("swagger.json");
    ///
    /// Generator::new()
    ///     .input(src.path())
    ///     .package(Package::new("shop", "1.0.0"))
    ///     .output_swagger2(&path)
    ///     .generate()?;
    /// let document: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(&path)?)?;
    /// assert_eq!(document["swagger"], "2.0");
    /// assert_eq!(document["definitions"]["Tag"]["type"], "object");
    /// # Ok::<(), Box<dyn std::error::Error>>(())
    /// ```
    pub fn output_swagger2(self, path: impl Into<PathBuf>) -> Self {
        self.output_as(OutputKind::Swagger20, path)
    }

    /// Writes the AsyncAPI document (from `@asyncapi` and `@channel` blocks)
    /// to `path`.
    ///
    /// # Examples
    ///
    /// ```
    /// use oas_forge::Generator;
    ///
    /// let src = tempfile::tempdir()?;
    /// std::fs::write(
    ///     src.path().join("lib.rs"),
    ///     r#"
    /// //! @asyncapi
    /// //! asyncapi: 3.0.0
    /// //! info:
    /// //!   title: Events
    /// //!   version: 1.0.0
    ///
    /// /// Tag changes
    /// /// @channel tags/changed
    /// /// @send TagChanged: { type: object }
    /// pub fn publish_tag_change() {}
    /// "#,
    /// )?;
    /// let out = tempfile::tempdir()?;
    /// let path = out.path().join("asyncapi.json");
    ///
    /// Generator::new().input(src.path()).output_asyncapi(&path).generate()?;
    /// let document: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(&path)?)?;
    /// assert_eq!(document["channels"]["tags_changed"]["address"], "tags/changed");
    /// assert_eq!(document["operations"]["publish_tag_change"]["action"], "send");
    /// # Ok::<(), Box<dyn std::error::Error>>(())
    /// ```
    pub fn output_asyncapi(self, path: impl Into<PathBuf>) -> Self {
        self.output_as(OutputKind::AsyncApi, path)
    }

    /// Writes the output strategy `kind` to `path`.
    ///
    /// # Examples
    ///
    /// ```
    /// use oas_forge::{Generator, OutputKind};
    ///
    /// let src = tempfile::tempdir()?;
    /// std::fs::write(src.path().join("lib.rs"), "/// @openapi\npub struct Tag { pub name: String }\n")?;
    /// let out = tempfile::tempdir()?;
    /// let path = out.path().join("schemas.yaml");
    ///
    /// Generator::new()
    ///     .input(src.path())
    ///     .output_as(OutputKind::Schemas, &path)
    ///     .generate()?;
    /// assert!(std::fs::read_to_string(&path)?.starts_with("Tag:\n  type: object\n"));
    /// # Ok::<(), Box<dyn std::error::Error>>(())
    /// ```
    pub fn output_as(mut self, kind: OutputKind, path: impl Into<PathBuf>) -> Self {
        self.outputs.push((None, kind, path.into()));
        self
    }

    /// Writes the output strategy `kind` of the document of `scope` to
    /// `path`: the snippets of that scope (`@scope` in route, webhook and
    /// channel blocks, module defaults and YAML blocks) and the shared ones
    /// (without `@scope`), with only the components and tags they use. One
    /// run can write the documents of several scopes; sources are read once.
    ///
    /// # Examples
    ///
    /// ```
    /// use oas_forge::{Generator, OutputKind};
    ///
    /// let src = tempfile::tempdir()?;
    /// std::fs::write(
    ///     src.path().join("lib.rs"),
    ///     r#"
    /// /// @openapi
    /// /// @scope public
    /// /// openapi: 3.1.0
    /// /// info: { title: Public API, version: 1.0.0 }
    /// fn public_root() {}
    ///
    /// /// @openapi
    /// /// @scope admin
    /// /// openapi: 3.1.0
    /// /// info: { title: Admin API, version: 1.0.0 }
    /// fn admin_root() {}
    ///
    /// /// Health
    /// /// @route GET /health
    /// /// @scope public
    /// pub async fn health() {}
    ///
    /// /// Health with details
    /// /// @route GET /health
    /// /// @scope admin
    /// pub async fn admin_health() {}
    /// "#,
    /// )?;
    /// let out = tempfile::tempdir()?;
    ///
    /// Generator::new()
    ///     .input(src.path())
    ///     .output_scoped("public", OutputKind::Full, out.path().join("public.json"))
    ///     .output_scoped("admin", OutputKind::Full, out.path().join("admin.json"))
    ///     .generate()?;
    /// let admin: serde_json::Value =
    ///     serde_json::from_str(&std::fs::read_to_string(out.path().join("admin.json"))?)?;
    /// assert_eq!(admin["info"]["title"], "Admin API");
    /// assert_eq!(admin["paths"]["/health"]["get"]["operationId"], "admin_health");
    /// # Ok::<(), Box<dyn std::error::Error>>(())
    /// ```
    pub fn output_scoped(
        mut self,
        scope: impl Into<String>,
        kind: OutputKind,
        path: impl Into<PathBuf>,
    ) -> Self {
        self.outputs.push((Some(scope.into()), kind, path.into()));
        self
    }

    /// Selects the scope of [`Spec::document`], [`Spec::render`] and of
    /// every output configured without its own scope (see
    /// [`Generator::output_scoped`]). Without a scope, every snippet is
    /// merged into one document, `@scope` or not.
    ///
    /// # Examples
    ///
    /// ```
    /// use oas_forge::Generator;
    ///
    /// let src = tempfile::tempdir()?;
    /// std::fs::write(
    ///     src.path().join("lib.rs"),
    ///     r#"
    /// /// List users
    /// /// @route GET /users
    /// pub async fn list_users() {}
    ///
    /// /// Delete a user
    /// /// @route DELETE /users
    /// /// @scope admin
    /// pub async fn delete_users() {}
    ///
    /// /// Reindex
    /// /// @route POST /reindex
    /// /// @scope internal
    /// pub async fn reindex() {}
    /// "#,
    /// )?;
    ///
    /// let spec = Generator::new()
    ///     .input(src.path())
    ///     .without_package()
    ///     .scope("admin")
    ///     .build()?;
    /// let paths = &spec.document()["paths"];
    /// // The shared route and the admin route, not the internal one.
    /// assert!(paths["/users"]["get"].is_object());
    /// assert!(paths["/users"]["delete"].is_object());
    /// assert!(paths.get("/reindex").is_none());
    /// # Ok::<(), Box<dyn std::error::Error>>(())
    /// ```
    pub fn scope(mut self, name: impl Into<String>) -> Self {
        self.scope = Some(name.into());
        self
    }

    /// Sets the value of a `{{name}}` placeholder; a later value for the
    /// same name wins.
    ///
    /// # Examples
    ///
    /// ```
    /// use oas_forge::Generator;
    ///
    /// let dir = tempfile::tempdir()?;
    /// let root = dir.path().join("root.yaml");
    /// std::fs::write(
    ///     &root,
    ///     "openapi: 3.1.0\ninfo:\n  title: Shop\n  version: 1.0.0\nservers:\n  - url: \"{{API_URL}}\"\n",
    /// )?;
    ///
    /// let spec = Generator::new()
    ///     .include(&root)
    ///     .var("API_URL", "https://api.example.com")
    ///     .build()?;
    /// assert_eq!(spec.document()["servers"][0]["url"], "https://api.example.com");
    /// # Ok::<(), Box<dyn std::error::Error>>(())
    /// ```
    pub fn var(mut self, name: impl Into<String>, value: impl Into<String>) -> Self {
        self.variables.insert(name.into(), value.into());
        self
    }

    /// Documents the Rust type `name` (or path `my_crate::Name`) with `schema`
    /// instead of the built-in mapping or a `$ref`.
    ///
    /// # Examples
    ///
    /// ```
    /// use serde_json::json;
    ///
    /// let src = tempfile::tempdir()?;
    /// std::fs::write(src.path().join("lib.rs"), "/// @openapi\npub struct User { pub id: UserId }\n")?;
    ///
    /// let spec = oas_forge::Generator::new()
    ///     .map_type("UserId", json!({ "type": "string", "format": "uuid" }))
    ///     .input(src.path())
    ///     .without_package()
    ///     .build()?;
    /// assert_eq!(
    ///     spec.document()["components"]["schemas"]["User"]["properties"]["id"],
    ///     json!({ "type": "string", "format": "uuid" })
    /// );
    /// # Ok::<(), Box<dyn std::error::Error>>(())
    /// ```
    pub fn map_type(mut self, name: impl Into<String>, schema: Value) -> Self {
        self.types = self.types.with_type(name, schema);
        self
    }

    /// Fills gaps of `@route` blocks from the handler signature (axum and
    /// actix-web extractors such as `Path<T>`, `Query<T>`, `Json<T>`); on by
    /// default. A single route opts out with `@no-infer`.
    ///
    /// # Examples
    ///
    /// ```
    /// use oas_forge::Generator;
    /// use serde_json::json;
    ///
    /// let src = tempfile::tempdir()?;
    /// std::fs::write(
    ///     src.path().join("lib.rs"),
    ///     r#"
    /// /// Get a tag
    /// /// @route GET /tags/{name}
    /// pub async fn get_tag(Path(name): Path<String>) -> Json<String> { todo!() }
    /// "#,
    /// )?;
    ///
    /// let spec = Generator::new().input(src.path()).without_package().build()?;
    /// let operation = &spec.document()["paths"]["/tags/{name}"]["get"];
    /// assert_eq!(operation["parameters"][0]["schema"], json!({ "type": "string" }));
    /// assert_eq!(
    ///     operation["responses"]["200"]["content"]["application/json"]["schema"],
    ///     json!({ "type": "string" })
    /// );
    ///
    /// // Without inference the path parameter has no definition: an error.
    /// let spec = Generator::new()
    ///     .input(src.path())
    ///     .without_package()
    ///     .infer_signatures(false)
    ///     .build()?;
    /// assert!(spec.diagnostics().has_errors());
    /// # Ok::<(), Box<dyn std::error::Error>>(())
    /// ```
    pub fn infer_signatures(mut self, infer: bool) -> Self {
        self.infer = infer;
        self
    }

    /// Uses `package` for the root document when the sources define none
    /// and for the `{{CARGO_PKG_*}}` variables ([`Package::variables`]);
    /// [`Generator::var`] calls after this one win. In a build script the
    /// running package is used by default.
    ///
    /// # Examples
    ///
    /// ```
    /// use oas_forge::{Code, Generator, Package};
    /// use serde_json::json;
    ///
    /// let src = tempfile::tempdir()?;
    /// std::fs::write(src.path().join("lib.rs"), "/// @openapi\npub struct Tag { pub name: String }\n")?;
    ///
    /// let spec = Generator::new()
    ///     .input(src.path())
    ///     .package(Package::new("shop", "1.0.0"))
    ///     .build()?;
    /// assert_eq!(spec.document()["openapi"], "3.1.1");
    /// assert_eq!(spec.document()["info"], json!({ "title": "shop", "version": "1.0.0" }));
    /// let first = spec.diagnostics().iter().next().map(|d| d.code());
    /// assert_eq!(first, Some(Code::GeneratedRoot));
    /// # Ok::<(), Box<dyn std::error::Error>>(())
    /// ```
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
    ///
    /// # Examples
    ///
    /// ```
    /// use oas_forge::{Generator, OutputKind};
    ///
    /// let src = tempfile::tempdir()?;
    /// std::fs::write(src.path().join("lib.rs"), "/// @openapi\npub struct Tag { pub name: String }\n")?;
    ///
    /// let spec = Generator::new().input(src.path()).without_package().build()?;
    /// assert!(spec.document().get("openapi").is_none());
    /// // The full document needs a root.
    /// assert!(spec.render(OutputKind::Full).is_err());
    /// # Ok::<(), Box<dyn std::error::Error>>(())
    /// ```
    pub fn without_package(mut self) -> Self {
        self.package = None;
        self
    }

    /// Also reports unused components and operations or schemas without
    /// description.
    ///
    /// # Examples
    ///
    /// ```
    /// use oas_forge::{Code, Generator};
    ///
    /// let src = tempfile::tempdir()?;
    /// std::fs::write(src.path().join("lib.rs"), "/// @openapi\npub struct Tag { pub name: String }\n")?;
    ///
    /// let spec = Generator::new()
    ///     .input(src.path())
    ///     .without_package()
    ///     .strict_lints(true)
    ///     .build()?;
    /// let codes: Vec<Code> = spec.diagnostics().iter().map(|d| d.code()).collect();
    /// assert_eq!(codes, [Code::UnusedComponent, Code::MissingDescription]);
    /// # Ok::<(), Box<dyn std::error::Error>>(())
    /// ```
    pub fn strict_lints(mut self, strict: bool) -> Self {
        self.strict_lints = strict;
        self
    }

    /// Treats warnings as errors.
    ///
    /// # Examples
    ///
    /// ```
    /// use oas_forge::Generator;
    ///
    /// let src = tempfile::tempdir()?;
    /// std::fs::write(
    ///     src.path().join("lib.rs"),
    ///     "/// List tags\n/// @route GET /tags\n/// @return 200: $Missing\npub async fn list_tags() {}\n",
    /// )?;
    ///
    /// let lenient = Generator::new().input(src.path()).without_package().build()?;
    /// assert!(!lenient.diagnostics().is_empty());
    /// assert!(!lenient.diagnostics().has_errors());
    ///
    /// let strict = Generator::new()
    ///     .input(src.path())
    ///     .without_package()
    ///     .deny_warnings(true)
    ///     .build()?;
    /// assert!(strict.diagnostics().has_errors());
    /// # Ok::<(), Box<dyn std::error::Error>>(())
    /// ```
    pub fn deny_warnings(mut self, deny: bool) -> Self {
        self.deny_warnings = deny;
        self
    }

    /// Sets the size limit per input file in bytes (default 16 MiB). A
    /// larger file is skipped with an error ([`Code::FileTooLarge`]).
    ///
    /// # Examples
    ///
    /// ```
    /// use oas_forge::{Code, Generator};
    ///
    /// let src = tempfile::tempdir()?;
    /// std::fs::write(src.path().join("lib.rs"), "/// @openapi\npub struct Tag { pub name: String }\n")?;
    ///
    /// let spec = Generator::new()
    ///     .input(src.path())
    ///     .without_package()
    ///     .max_file_size(16)
    ///     .build()?;
    /// let finding = spec.diagnostics().iter().next().expect("the file is too large");
    /// assert_eq!(finding.code(), Code::FileTooLarge);
    /// assert!(finding.is_error());
    /// # Ok::<(), Box<dyn std::error::Error>>(())
    /// ```
    pub fn max_file_size(mut self, bytes: u64) -> Self {
        self.max_file_size = bytes;
        self
    }

    /// In build scripts: print `cargo:warning=` for diagnostics and
    /// `cargo:rerun-if-changed=` for every input when generating.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// // build.rs
    /// oas_forge::Generator::new()
    ///     .input("src")
    ///     .output("openapi.yaml")
    ///     .emit_cargo_directives(true)
    ///     .generate()?;
    /// # Ok::<(), oas_forge::Error>(())
    /// ```
    pub fn emit_cargo_directives(mut self, emit: bool) -> Self {
        self.cargo_directives = emit;
        self
    }

    /// Runs the pipeline without writing files.
    ///
    /// # Errors
    /// Fatal problems (unreadable inputs, invalid Rust, invalid YAML, multiple
    /// roots). Error-level diagnostics are returned in the [`Spec`].
    ///
    /// # Examples
    ///
    /// ```
    /// use oas_forge::Generator;
    /// use serde_json::json;
    ///
    /// let src = tempfile::tempdir()?;
    /// std::fs::write(
    ///     src.path().join("lib.rs"),
    ///     r#"
    /// /// A tag.
    /// /// @openapi
    /// pub struct Tag { pub name: String }
    ///
    /// /// List tags
    /// /// @route GET /tags
    /// /// @return 200: Vec<$Tag>
    /// pub async fn list_tags() {}
    /// "#,
    /// )?;
    ///
    /// let spec = Generator::new().input(src.path()).without_package().build()?;
    /// let operation = &spec.document()["paths"]["/tags"]["get"];
    /// assert_eq!(operation["summary"], "List tags");
    /// assert_eq!(
    ///     operation["responses"]["200"]["content"]["application/json"]["schema"],
    ///     json!({ "type": "array", "items": { "$ref": "#/components/schemas/Tag" } })
    /// );
    /// assert!(spec.diagnostics().is_empty());
    /// # Ok::<(), Box<dyn std::error::Error>>(())
    /// ```
    pub fn build(&self) -> Result<Spec> {
        let mut diagnostics = Diagnostics::new();
        let options = DiscoverOptions::default()
            .with_exclude(self.outputs.iter().map(|(_, _, p)| p.clone()).collect())
            .with_max_file_size(self.max_file_size);
        let sources =
            SourceSet::discover(&self.inputs, &self.includes, &options, &mut diagnostics)?;
        let settings = Settings {
            variables: &self.variables,
            types: &self.types,
            infer: self.infer,
        };
        let prepared = prepare(&sources, settings, &mut diagnostics)?;
        // Outputs without their own scope use the default scope.
        let outputs: Vec<(Option<String>, OutputKind, PathBuf)> = self
            .outputs
            .iter()
            .map(|(scope, kind, path)| {
                (
                    scope.clone().or_else(|| self.scope.clone()),
                    *kind,
                    path.clone(),
                )
            })
            .collect();
        // The documents of the outputs' scopes, and of the default scope
        // unless only scoped outputs are configured.
        let mut scopes: BTreeSet<Option<String>> =
            outputs.iter().map(|(scope, _, _)| scope.clone()).collect();
        if scopes.is_empty() || self.scope.is_some() {
            scopes.insert(self.scope.clone());
        }
        let mut documents = BTreeMap::new();
        let mut findings = ScopeFindings::default();
        for scope in scopes {
            let mut found = Diagnostics::new();
            let finish = Finish {
                scope: scope.as_deref(),
                package: self.package.as_ref(),
                strict_lints: self.strict_lints,
            };
            documents.insert(scope.clone(), assemble(&prepared, finish, &mut found)?);
            findings.add(&mut diagnostics, found);
            findings.next_scope();
        }
        let mut spec = Spec {
            documents,
            scope: self.scope.clone(),
            prepared,
            package: self.package.clone(),
            strict_lints: self.strict_lints,
            outputs,
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
        let mut targets: Vec<(Option<String>, OutputKind)> = spec
            .outputs
            .iter()
            .map(|(scope, kind, _)| (scope.clone(), *kind))
            .collect();
        targets.sort();
        targets.dedup();
        let mut findings = ScopeFindings::default();
        let mut last: Option<Option<String>> = None;
        for (scope, kind) in targets {
            if last.as_ref() != Some(&scope) {
                findings.next_scope();
                last = Some(scope.clone());
            }
            if let Ok((value, found)) = spec.render_in(scope.as_deref(), kind) {
                findings.add(&mut spec.diagnostics, found);
                spec.rendered.insert((scope, kind), value);
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
    ///
    /// # Examples
    ///
    /// ```
    /// use oas_forge::{Error, Generator, Package};
    ///
    /// let src = tempfile::tempdir()?;
    /// std::fs::write(src.path().join("lib.rs"), "/// @openapi\npub struct Tag { pub name: String }\n")?;
    /// let out = tempfile::tempdir()?;
    /// let path = out.path().join("openapi.json");
    /// let generator = Generator::new()
    ///     .input(src.path())
    ///     .package(Package::new("shop", "1.0.0"))
    ///     .output(&path);
    ///
    /// generator.generate()?;
    /// let document: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(&path)?)?;
    /// assert_eq!(document["info"]["title"], "shop");
    ///
    /// // A route without a definition for its path parameter fails the run.
    /// std::fs::write(
    ///     src.path().join("broken.rs"),
    ///     "/// @route GET /tags/{id}\npub async fn get_tag() {}\n",
    /// )?;
    /// assert!(matches!(generator.generate(), Err(Error::Diagnostics(_))));
    /// # Ok::<(), Box<dyn std::error::Error>>(())
    /// ```
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
///
/// It holds the documents in memory: inspect them, render any output kind
/// or write the configured outputs.
///
/// # Examples
///
/// ```
/// use oas_forge::{Generator, OutputKind, Package};
///
/// let src = tempfile::tempdir()?;
/// std::fs::write(src.path().join("lib.rs"), "/// @openapi\npub struct Tag { pub name: String }\n")?;
/// let out = tempfile::tempdir()?;
///
/// let spec = Generator::new()
///     .input(src.path())
///     .package(Package::new("shop", "1.0.0"))
///     .output(out.path().join("openapi.yaml"))
///     .build()?;
/// for diagnostic in spec.diagnostics() {
///     eprintln!("{diagnostic}");
/// }
/// assert_eq!(spec.render(OutputKind::Schemas)?["Tag"]["type"], "object");
/// spec.write()?;
/// # Ok::<(), Box<dyn std::error::Error>>(())
/// ```
#[derive(Debug, Clone)]
pub struct Spec {
    /// The documents built by [`Generator::build`], by scope (`None`: every
    /// snippet).
    documents: BTreeMap<Option<String>, Documents>,
    /// The default scope ([`Generator::scope`]).
    scope: Option<String>,
    /// The snippets and templates of the run, for the documents of scopes
    /// that were not built.
    prepared: Prepared,
    /// The package a root document is generated from.
    package: Option<Package>,
    /// Whether the strict lints run.
    strict_lints: bool,
    /// The configured outputs with their scope, in the order they were
    /// added.
    outputs: Vec<(Option<String>, OutputKind, PathBuf)>,
    /// Every finding, those of rendering the configured outputs included.
    diagnostics: Diagnostics,
    /// Inputs and configuration files, for `cargo:rerun-if-changed`.
    watched: Vec<PathBuf>,
    /// Environment variables read with `{{env:NAME}}`, sorted and unique.
    watched_env: Vec<String>,
    /// Each configured output kind and scope, rendered once by
    /// [`Generator::build`].
    rendered: BTreeMap<(Option<String>, OutputKind), Value>,
}

/// The empty document of [`Spec::document`] when the default scope was not
/// built.
static EMPTY: LazyLock<Value> = LazyLock::new(|| Value::Object(Map::new()));

impl Spec {
    /// The merged document (it has no root when none was defined).
    ///
    /// # Examples
    ///
    /// ```
    /// use oas_forge::Generator;
    ///
    /// let src = tempfile::tempdir()?;
    /// std::fs::write(src.path().join("lib.rs"), "/// @openapi\npub struct Tag { pub name: String }\n")?;
    ///
    /// let spec = Generator::new().input(src.path()).without_package().build()?;
    /// assert_eq!(spec.document()["components"]["schemas"]["Tag"]["type"], "object");
    /// assert!(spec.document().get("info").is_none());
    /// # Ok::<(), Box<dyn std::error::Error>>(())
    /// ```
    pub fn document(&self) -> &Value {
        self.documents
            .get(&self.scope)
            .map_or(&*EMPTY, |documents| &documents.document)
    }

    /// The templates and fragments known to this run.
    ///
    /// # Examples
    ///
    /// ```
    /// use oas_forge::Generator;
    ///
    /// let src = tempfile::tempdir()?;
    /// std::fs::write(
    ///     src.path().join("lib.rs"),
    ///     r#"
    /// /// @openapi-fragment Paging(max)
    /// /// - name: limit
    /// ///   in: query
    /// ///   schema: { type: integer, maximum: {{max}} }
    /// fn paging() {}
    /// "#,
    /// )?;
    ///
    /// let spec = Generator::new().input(src.path()).without_package().build()?;
    /// let paging = spec.registry().fragment("Paging").expect("a fragment");
    /// assert_eq!(paging.params, ["max"]);
    /// # Ok::<(), Box<dyn std::error::Error>>(())
    /// ```
    pub fn registry(&self) -> &Registry {
        &self.prepared.registry
    }

    /// Every finding of the run.
    ///
    /// # Examples
    ///
    /// ```
    /// use oas_forge::{Code, Generator};
    ///
    /// let src = tempfile::tempdir()?;
    /// std::fs::write(
    ///     src.path().join("lib.rs"),
    ///     "/// List tags\n/// @route GET /tags\n/// @return 200: $Missing\npub async fn list_tags() {}\n",
    /// )?;
    ///
    /// let spec = Generator::new().input(src.path()).without_package().build()?;
    /// let codes: Vec<Code> = spec.diagnostics().iter().map(|d| d.code()).collect();
    /// assert_eq!(codes, [Code::UnresolvedReference, Code::DanglingReference]);
    /// # Ok::<(), Box<dyn std::error::Error>>(())
    /// ```
    pub fn diagnostics(&self) -> &Diagnostics {
        &self.diagnostics
    }

    /// Inputs and configuration files ([`Config::files`]) whose changes
    /// affect the result.
    ///
    /// # Examples
    ///
    /// ```
    /// use oas_forge::Generator;
    ///
    /// let src = tempfile::tempdir()?;
    /// std::fs::write(src.path().join("lib.rs"), "/// @openapi\npub struct Tag { pub name: String }\n")?;
    ///
    /// let spec = Generator::new().input(src.path()).without_package().build()?;
    /// assert_eq!(spec.watched(), [src.path()]);
    /// # Ok::<(), Box<dyn std::error::Error>>(())
    /// ```
    pub fn watched(&self) -> &[PathBuf] {
        &self.watched
    }

    /// Environment variables the inputs read with `{{env:NAME}}`, sorted.
    ///
    /// # Examples
    ///
    /// ```
    /// use oas_forge::Generator;
    ///
    /// let src = tempfile::tempdir()?;
    /// std::fs::write(
    ///     src.path().join("servers.yaml"),
    ///     "servers:\n  - url: https://{{env:SHOP_API_HOST}}\n",
    /// )?;
    ///
    /// let spec = Generator::new().input(src.path()).without_package().build()?;
    /// assert_eq!(spec.watched_env(), ["SHOP_API_HOST"]);
    /// # Ok::<(), Box<dyn std::error::Error>>(())
    /// ```
    pub fn watched_env(&self) -> &[String] {
        &self.watched_env
    }

    /// The value an output strategy receives; any kind can be rendered,
    /// configured as an output or not.
    ///
    /// # Errors
    /// [`Error::NoRootFound`] for [`OutputKind::Full`],
    /// [`OutputKind::OpenApi30`] and [`OutputKind::Swagger20`] without root
    /// document, [`Error::NoAsyncApiRoot`] for [`OutputKind::AsyncApi`]
    /// without AsyncAPI root.
    ///
    /// # Examples
    ///
    /// ```
    /// use oas_forge::{Generator, OutputKind, Package};
    ///
    /// let src = tempfile::tempdir()?;
    /// std::fs::write(src.path().join("lib.rs"), "/// @openapi\npub struct Tag { pub name: String }\n")?;
    ///
    /// let spec = Generator::new()
    ///     .input(src.path())
    ///     .package(Package::new("shop", "1.0.0"))
    ///     .build()?;
    /// let swagger = spec.render(OutputKind::Swagger20)?;
    /// assert_eq!(swagger["swagger"], "2.0");
    /// assert_eq!(swagger["definitions"]["Tag"]["type"], "object");
    /// # Ok::<(), Box<dyn std::error::Error>>(())
    /// ```
    pub fn render(&self, kind: OutputKind) -> Result<Value> {
        self.cached(self.scope.as_deref(), kind)
    }

    /// The value an output strategy receives from the document of `scope`
    /// (see [`Generator::output_scoped`]); the scope need not be configured.
    ///
    /// # Errors
    /// As [`Spec::render`], and [`Error::MultipleRootsFound`] when the scope
    /// was not built and several of its snippets declare a root.
    ///
    /// # Examples
    ///
    /// ```
    /// use oas_forge::{Generator, OutputKind};
    ///
    /// let src = tempfile::tempdir()?;
    /// std::fs::write(
    ///     src.path().join("lib.rs"),
    ///     r#"
    /// /// Users.
    /// /// @openapi
    /// pub struct User { pub id: u64 }
    ///
    /// /// Audit entries.
    /// /// @openapi
    /// pub struct Entry { pub id: u64 }
    ///
    /// /// List users
    /// /// @route GET /users
    /// /// @return 200: Vec<$User>
    /// pub async fn list_users() {}
    ///
    /// /// Read the audit log
    /// /// @route GET /audit
    /// /// @scope admin
    /// /// @return 200: Vec<$Entry>
    /// pub async fn audit() {}
    /// "#,
    /// )?;
    /// let spec = Generator::new().input(src.path()).without_package().build()?;
    ///
    /// // Without a scope, everything is merged.
    /// assert_eq!(spec.render(OutputKind::Schemas)?.as_object().map(|s| s.len()), Some(2));
    /// // A scope keeps the schemas its operations use.
    /// let public = spec.render_scoped(OutputKind::Fragments, "public")?;
    /// assert!(public["paths"].get("/audit").is_none());
    /// assert!(public["components"]["schemas"].get("Entry").is_none());
    /// let admin = spec.render_scoped(OutputKind::Paths, "admin")?;
    /// assert!(admin["/audit"]["get"].is_object());
    /// # Ok::<(), Box<dyn std::error::Error>>(())
    /// ```
    pub fn render_scoped(&self, kind: OutputKind, scope: &str) -> Result<Value> {
        self.cached(Some(scope), kind)
    }

    /// The output `kind` of the document of `scope`, rendered by
    /// [`Generator::build`] or now.
    fn cached(&self, scope: Option<&str>, kind: OutputKind) -> Result<Value> {
        if let Some(value) = self.rendered.get(&(scope.map(str::to_string), kind)) {
            return Ok(value.clone());
        }
        self.render_in(scope, kind).map(|(value, _)| value)
    }

    /// The value an output strategy receives and what it could not express
    /// (e.g. constructs an OpenAPI 3.0 export approximates).
    ///
    /// # Errors
    /// As [`Spec::render`]: [`Error::NoRootFound`] or
    /// [`Error::NoAsyncApiRoot`] for outputs that need a root document.
    ///
    /// # Examples
    ///
    /// ```
    /// use oas_forge::{Code, Generator, OutputKind, Package};
    ///
    /// let src = tempfile::tempdir()?;
    /// std::fs::write(
    ///     src.path().join("lib.rs"),
    ///     "/// @openapi\n#[serde(untagged)]\npub enum Id { Number(u64), Text(String) }\n",
    /// )?;
    /// let spec = Generator::new()
    ///     .input(src.path())
    ///     .package(Package::new("shop", "1.0.0"))
    ///     .build()?;
    ///
    /// // Swagger 2.0 has no `anyOf`.
    /// let (swagger, findings) = spec.render_with_diagnostics(OutputKind::Swagger20)?;
    /// assert!(swagger["definitions"]["Id"]["x-anyOf"].is_array());
    /// let codes: Vec<Code> = findings.iter().map(|d| d.code()).collect();
    /// assert_eq!(codes, [Code::LossyLowering]);
    /// # Ok::<(), Box<dyn std::error::Error>>(())
    /// ```
    pub fn render_with_diagnostics(&self, kind: OutputKind) -> Result<(Value, Diagnostics)> {
        self.render_in(self.scope.as_deref(), kind)
    }

    /// Renders the output `kind` of the document of `scope`; a scope that
    /// [`Generator::build`] did not build is merged now (its findings are
    /// left out).
    fn render_in(&self, scope: Option<&str>, kind: OutputKind) -> Result<(Value, Diagnostics)> {
        let merged;
        let documents = match self.documents.get(&scope.map(str::to_string)) {
            Some(documents) => documents,
            None => {
                let finish = Finish {
                    scope,
                    package: self.package.as_ref(),
                    strict_lints: self.strict_lints,
                };
                merged = assemble(&self.prepared, finish, &mut Diagnostics::new())?;
                &merged
            }
        };
        let input = output::RenderInput {
            document: &documents.document,
            canonical: documents.canonical.as_ref().unwrap_or(&documents.document),
            generated_keys: &documents.generated_keys,
            asyncapi: &documents.asyncapi,
            registry: &self.prepared.registry,
        };
        let mut diagnostics = Diagnostics::new();
        let value = output::render(kind, &input, &mut diagnostics)?;
        Ok((value, diagnostics))
    }

    /// Writes every configured output (skipping files whose content is
    /// unchanged) and returns the configured paths.
    ///
    /// # Errors
    /// [`Error::NoRootFound`] or [`Error::NoAsyncApiRoot`] when an output
    /// cannot be rendered (then nothing is written), [`Error::Io`] when a
    /// file cannot be written.
    ///
    /// # Examples
    ///
    /// ```
    /// use oas_forge::Generator;
    ///
    /// let src = tempfile::tempdir()?;
    /// std::fs::write(src.path().join("lib.rs"), "/// @openapi\npub struct Tag { pub name: String }\n")?;
    /// let out = tempfile::tempdir()?;
    /// let path = out.path().join("schemas.yaml");
    ///
    /// let spec = Generator::new().input(src.path()).output_schemas(&path).build()?;
    /// assert_eq!(spec.write()?, [path.clone()]);
    /// assert!(path.is_file());
    /// # Ok::<(), Box<dyn std::error::Error>>(())
    /// ```
    pub fn write(&self) -> Result<Vec<PathBuf>> {
        let mut rendered = Vec::new();
        for (scope, kind, path) in &self.outputs {
            rendered.push((path, self.text(scope.as_deref(), *kind, path)?));
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
    ///
    /// # Examples
    ///
    /// ```
    /// use oas_forge::Generator;
    ///
    /// let src = tempfile::tempdir()?;
    /// std::fs::write(src.path().join("lib.rs"), "/// @openapi\npub struct Tag { pub name: String }\n")?;
    /// let out = tempfile::tempdir()?;
    /// let path = out.path().join("schemas.yaml");
    ///
    /// let spec = Generator::new().input(src.path()).output_schemas(&path).build()?;
    /// assert_eq!(spec.check()?, [path.clone()]); // not written yet
    /// spec.write()?;
    /// assert!(spec.check()?.is_empty());
    /// # Ok::<(), Box<dyn std::error::Error>>(())
    /// ```
    pub fn check(&self) -> Result<Vec<PathBuf>> {
        let mut outdated = Vec::new();
        for (scope, kind, path) in &self.outputs {
            if !output::is_current(path, &self.text(scope.as_deref(), *kind, path)?) {
                outdated.push(path.clone());
            }
        }
        Ok(outdated)
    }

    /// The text the output file `path` of `kind` of the document of `scope`
    /// receives.
    fn text(&self, scope: Option<&str>, kind: OutputKind, path: &Path) -> Result<String> {
        Ok(Format::of(path).render(&self.cached(scope, kind)?))
    }

    /// Prints the [cargo directives](Spec::write_cargo_directives) of a
    /// build script to standard output.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// // build.rs: generate, but let Cargo decide when to run again.
    /// let spec = oas_forge::Generator::new()
    ///     .input("src")
    ///     .output("openapi.yaml")
    ///     .build()?;
    /// spec.print_cargo_directives();
    /// spec.write()?;
    /// # Ok::<(), oas_forge::Error>(())
    /// ```
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
    ///
    /// # Examples
    ///
    /// ```
    /// use oas_forge::Generator;
    ///
    /// let src = tempfile::tempdir()?;
    /// std::fs::write(src.path().join("lib.rs"), "/// @openapi\npub struct Tag { pub name: String }\n")?;
    ///
    /// let spec = Generator::new().input(src.path()).without_package().build()?;
    /// let mut out = Vec::new();
    /// spec.write_cargo_directives(&mut out)?;
    /// assert_eq!(
    ///     String::from_utf8(out)?,
    ///     format!("cargo:rerun-if-changed={}\n", src.path().display())
    /// );
    /// # Ok::<(), Box<dyn std::error::Error>>(())
    /// ```
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
#[derive(Debug, Clone)]
struct Snippet {
    /// The parsed value, merged into the document.
    value: Value,
    /// Where the annotation or file starts; findings point here.
    location: Location,
    /// Part of the AsyncAPI document instead of the OpenAPI document.
    asyncapi: bool,
    /// The scopes the snippet belongs to (`@scope`); empty when it is
    /// shared by every scope.
    scopes: Vec<String>,
}

impl Snippet {
    /// A shared snippet of the OpenAPI document.
    fn openapi(value: Value, location: Location) -> Self {
        Self {
            value,
            location,
            asyncapi: false,
            scopes: Vec::new(),
        }
    }

    /// The same snippet in the scopes `scopes`.
    fn in_scopes(mut self, scopes: Vec<String>) -> Self {
        self.scopes = scopes;
        self
    }
}

/// A piece of text that still has to go through the lexical stage.
enum Pending {
    /// A free `@openapi` block.
    Document {
        /// The YAML text before expansion.
        text: String,
        /// Where the text starts.
        location: Location,
        /// Tags of the enclosing modules, for operations without tags.
        tags: Vec<String>,
    },
    /// A schema written as YAML (`@openapi-type Name`).
    NamedSchema {
        /// The schema name.
        name: String,
        /// The YAML text before expansion.
        text: String,
        /// Where the text starts.
        location: Location,
    },
    /// A `@route` or `@webhook` block, compiled after expansion.
    Route(Box<extract::RouteItem>),
    /// A YAML or JSON input file.
    File {
        /// The file content.
        text: String,
        /// Line 1 of the file.
        location: Location,
    },
    /// A free `@asyncapi` block.
    AsyncApi {
        /// The YAML text before expansion.
        text: String,
        /// Where the text starts.
        location: Location,
    },
    /// A `@channel` block, compiled after expansion.
    Channel(Box<extract::ChannelItem>),
}

/// The settings of a pipeline run.
#[derive(Clone, Copy)]
pub(crate) struct Settings<'a> {
    /// Values of `{{NAME}}` placeholders.
    pub(crate) variables: &'a Variables,
    /// How Rust types map to schemas.
    pub(crate) types: &'a TypeMapper,
    /// Whether `@route` blocks are completed from handler signatures.
    pub(crate) infer: bool,
}

/// The snippets of a run before they are merged: passes 1 to 4 are done
/// once, the merge and what follows once per scope ([`assemble`]).
#[derive(Debug, Clone)]
pub(crate) struct Prepared {
    /// Every snippet in source order, with generics instantiated and smart
    /// references resolved.
    snippets: Vec<Snippet>,
    /// The concrete schemas of generic instantiations (shared).
    generated: Vec<Snippet>,
    /// The templates of the run.
    registry: Registry,
}

/// What [`assemble`] needs besides the snippets.
#[derive(Clone, Copy)]
pub(crate) struct Finish<'a> {
    /// The scope whose document is merged; `None` merges every snippet.
    pub(crate) scope: Option<&'a str>,
    /// The package a root document is generated from when the snippets
    /// define none.
    pub(crate) package: Option<&'a Package>,
    /// Whether the strict lints run.
    pub(crate) strict_lints: bool,
}

/// Runs passes 1 to 4 on `sources`: extraction, expansion, generics and
/// smart references.
pub(crate) fn prepare(
    sources: &SourceSet,
    settings: Settings<'_>,
    diagnostics: &mut Diagnostics,
) -> Result<Prepared> {
    let Settings {
        types, variables, ..
    } = settings;
    // Pass 1: extraction and template registration.
    let mut collected = Collected::default();
    // `.proto` files, for `proto(package.Message)` payloads.
    let mut protos = ProtoIndex::new();
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
            SourceKind::Proto => protos.add(file, diagnostics),
        }
    }
    let Collected {
        registry,
        patterns,
        mut known,
        snippets,
        pending,
        modules,
    } = collected;

    // Pass 2: lexical expansion, parsing, `@extend`, DSL.
    let mut ordered: Vec<(usize, Snippet)> = snippets
        .into_iter()
        .enumerate()
        .map(|(i, s)| (i * 2, s))
        .collect();
    let mut defaults = RouteDefaults {
        modules,
        expanded: BTreeMap::new(),
    };
    for (position, item) in pending {
        let context = Context {
            registry: &registry,
            protos: &protos,
            settings,
        };
        if let Some(snippet) = materialize(item, context, &mut defaults, diagnostics)? {
            known.collect_from(&snippet.value);
            ordered.push((position * 2 + 1, snippet));
        }
    }
    ordered.sort_by_key(|(position, _)| *position);
    let mut snippets: Vec<Snippet> = ordered.into_iter().map(|(_, s)| s).collect();

    // Pass 3: generic instantiation.
    let mut instantiator = Instantiator::new(&registry)
        .with_types(types)
        .with_known(&known);
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
    Ok(Prepared {
        snippets,
        generated,
        registry,
    })
}

/// Merges the documents of one scope (pass 5) and finishes them: the root
/// from the package, normalization, parameter expansion, shared schemas,
/// pruning (scoped documents only), lowering and lints.
pub(crate) fn assemble(
    prepared: &Prepared,
    finish: Finish<'_>,
    diagnostics: &mut Diagnostics,
) -> Result<Documents> {
    let Finish {
        scope,
        package,
        strict_lints,
    } = finish;
    // Pass 5: merge.
    let (asyncapi, snippets): (Vec<Snippet>, Vec<Snippet>) = prepared
        .snippets
        .iter()
        .filter(|s| scope::includes(&s.scopes, scope))
        .cloned()
        .partition(|s| s.asyncapi);
    let mut asyncapi = merge_documents(asyncapi, Vec::new(), is_asyncapi_root, diagnostics)?;
    refs::unescape(&mut asyncapi);
    let mut document = merge_documents(
        snippets,
        prepared.generated.clone(),
        output::is_root,
        diagnostics,
    )?;
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
    // The AsyncAPI document shares the schemas its messages use, before
    // pruning removes those no operation of the scope uses.
    let has_asyncapi = asyncapi.as_object().is_some_and(|map| !map.is_empty());
    if let Value::Object(map) = &mut asyncapi
        && has_asyncapi
    {
        output::asyncapi::add_shared_schemas(map, &document);
    }
    if scope.is_some() {
        scope::prune(&mut document);
        scope::prune(&mut asyncapi);
    }
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
        strict: strict_lints,
    };
    lint::check(&document, lints, diagnostics);
    if has_asyncapi {
        lint::check(&asyncapi, Lints::default(), diagnostics);
    }
    Ok(Documents {
        document,
        canonical,
        generated_keys,
        asyncapi,
    })
}

/// Whether a snippet is the root of the AsyncAPI document.
fn is_asyncapi_root(value: &Value) -> bool {
    value
        .as_object()
        .is_some_and(|m| m.contains_key("asyncapi") && m.contains_key("info"))
}

/// The documents of one scope of a pipeline run.
#[derive(Debug, Clone)]
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
}

/// The identity of a finding, for reporting it once across scopes.
type FindingKey = (Code, Severity, String, Option<Location>);

/// The findings of several scopes, each reported once: a finding that an
/// earlier scope reported is left out; within a scope nothing is.
#[derive(Default)]
struct ScopeFindings {
    /// The findings of the scopes before the current one.
    earlier: BTreeSet<FindingKey>,
    /// The findings of the current scope.
    current: BTreeSet<FindingKey>,
}

impl ScopeFindings {
    /// Adds the findings `found` of the current scope to `diagnostics`,
    /// except those an earlier scope reported.
    fn add(&mut self, diagnostics: &mut Diagnostics, found: Diagnostics) {
        for finding in found {
            let key = (
                finding.code(),
                finding.severity(),
                finding.message().to_string(),
                finding.location().cloned(),
            );
            if !self.earlier.contains(&key) {
                self.current.insert(key);
                diagnostics.push(finding);
            }
        }
    }

    /// Starts the next scope.
    fn next_scope(&mut self) {
        self.earlier.append(&mut self.current);
    }
}

/// What pass 1 collects.
#[derive(Default)]
struct Collected {
    /// Fragments, blueprints and the schema names of renamed Rust types.
    registry: Registry,
    /// Regular expression constants by name.
    patterns: BTreeMap<String, String>,
    /// The schema names smart references may resolve to.
    known: KnownSchemas,
    /// Snippets that are complete after extraction (reflected schemas).
    snippets: Vec<Snippet>,
    /// Texts that are expanded once every template is known, with the
    /// position among `snippets` they belong to.
    pending: Vec<(usize, Pending)>,
    /// `mod name;` declarations by the file of the module they declare.
    modules: BTreeMap<PathBuf, Vec<extract::ModuleItem>>,
}

impl Collected {
    /// Files `item` by stage: reflected schemas become snippets, templates
    /// and patterns are registered, and texts wait in `pending` until every
    /// template is known. A schema name defined twice is reported.
    fn register(&mut self, item: Item, diagnostics: &mut Diagnostics) {
        let Self {
            registry,
            patterns,
            known,
            snippets,
            pending,
            modules,
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
                if let Some(default) = schema.schema.get("default") {
                    registry.add_default(&schema.name, default.clone());
                }
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
            Item::Module(module) => {
                for file in &module.files {
                    modules
                        .entry(file.clone())
                        .or_default()
                        .push(module.clone());
                }
            }
            Item::Alias(alias) => registry.add_transparent(alias),
        }
    }
}

/// What pass 2 reads besides the pending text.
#[derive(Clone, Copy)]
struct Context<'a> {
    /// The templates and fragments.
    registry: &'a Registry,
    /// The `.proto` files, for `proto(…)` payloads of channel messages.
    protos: &'a ProtoIndex,
    /// The settings of the run.
    settings: Settings<'a>,
}

/// How many `mod name;` declarations a route's defaults are looked up
/// through, a bound for chains Rust would not compile anyway.
const MAX_MODULE_DEPTH: usize = 64;

/// The `@route-defaults` of a run.
struct RouteDefaults {
    /// `mod name;` declarations by the file of the module they declare.
    modules: BTreeMap<PathBuf, Vec<extract::ModuleItem>>,
    /// Expanded blocks by file and header line, each line with its location.
    expanded: BTreeMap<(PathBuf, usize), Vec<(String, Location)>>,
}

/// Lexical expansion and parsing of one pending text.
fn materialize(
    pending: Pending,
    context: Context<'_>,
    route_defaults: &mut RouteDefaults,
    diagnostics: &mut Diagnostics,
) -> Result<Option<Snippet>> {
    let Context {
        registry,
        protos,
        settings,
    } = context;
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
            let scoped = take_scopes(&text, &location, diagnostics);
            let Some(mut value) =
                parse_text(&scoped.text, registry, variables, &location, diagnostics)?
            else {
                return Ok(None);
            };
            apply_tags(&mut value, &tags);
            Ok(Some(
                Snippet::openapi(value, location).in_scopes(scoped.names),
            ))
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
                scopes: Vec::new(),
            }))
        }
        Pending::AsyncApi { text, location } => {
            let scoped = take_scopes(&text, &location, diagnostics);
            let Some(value) =
                parse_text(&scoped.text, registry, variables, &location, diagnostics)?
            else {
                return Ok(None);
            };
            Ok(Some(Snippet {
                value,
                location,
                asyncapi: true,
                scopes: scoped.names,
            }))
        }
        Pending::Channel(channel) => {
            let text = channel.lines.join("\n");
            // A channel block has no embedded YAML: no placeholder is quoted.
            let expanded = lexical::expand_block(
                &text,
                registry,
                variables,
                &channel.location,
                diagnostics,
                |lines| vec![false; lines.len()],
            );
            let line_numbers = source_lines(&expanded, &channel.line_numbers, &channel.location);
            let lines: Vec<String> = expanded.into_iter().map(|(line, _)| line).collect();
            let compiled = dsl::compile_channel(
                dsl::ChannelSource::new(&lines, &channel.location)
                    .line_numbers(&line_numbers)
                    .operation_id(channel.operation_id.as_deref())
                    .types(types)
                    .defaults(&channel.defaults)
                    .protos(protos),
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
                scopes: compiled.scopes,
            }))
        }
        Pending::Route(route) => {
            let text = route.lines.join("\n");
            let expanded = lexical::expand_block(
                &text,
                registry,
                variables,
                &route.location,
                diagnostics,
                dsl::yaml_lines,
            );
            let line_numbers = source_lines(&expanded, &route.line_numbers, &route.location);
            let lines: Vec<String> = expanded.into_iter().map(|(line, _)| line).collect();
            let (defaults, default_locations): (Vec<String>, Vec<Location>) =
                expand_defaults(&route, registry, variables, route_defaults, diagnostics)
                    .into_iter()
                    .unzip();
            let compiled = dsl::compile(
                RouteSource::new(&lines, &route.location)
                    .line_numbers(&line_numbers)
                    .operation_id(route.operation_id.as_deref())
                    .default_tags(&route.tags)
                    .types(types)
                    .defaults(&defaults)
                    .default_locations(&default_locations)
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
            Ok(Some(
                Snippet::openapi(value, route.location).in_scopes(compiled.scopes),
            ))
        }
    }
}

/// Takes the `@scope` lines out of the YAML `text` of a block that starts
/// at `location`; a line without valid names is reported.
fn take_scopes(
    text: &str,
    location: &Location,
    diagnostics: &mut Diagnostics,
) -> scope::ScopeLines {
    let scoped = scope::take_lines(text);
    for line in &scoped.invalid {
        diagnostics.push(
            Diagnostic::new(Code::InvalidDirective, dsl::SCOPE_SYNTAX).at(location.offset(*line)),
        );
    }
    scoped
}

/// The expanded `@route-defaults` blocks of a route, outermost first: those
/// of the modules that declare the route's file (`mod name;`), up to the
/// crate root, then the route's own (of its file and its inline modules),
/// each line with its location. Each module's block is expanded once for all
/// routes of the module, so its findings are reported once, at its own
/// lines.
fn expand_defaults(
    route: &extract::RouteItem,
    registry: &Registry,
    variables: &Variables,
    defaults: &mut RouteDefaults,
    diagnostics: &mut Diagnostics,
) -> Vec<(String, Location)> {
    let RouteDefaults {
        modules,
        expanded: cache,
    } = defaults;
    // The file, defaults and line numbers of each part, innermost first.
    let mut parts = vec![(
        route.location.file(),
        &route.defaults[..],
        &route.default_line_numbers[..],
    )];
    let mut file = extract::normalize_path(route.location.file());
    let mut seen = BTreeSet::from([file.clone()]);
    // A file declared twice belongs to the first declaration, by location,
    // so that the order of the files does not matter.
    let declaration = |file: &PathBuf| {
        modules.get(file).and_then(|declarations| {
            declarations
                .iter()
                .min_by(|a, b| a.location.cmp(&b.location))
        })
    };
    for _ in 0..MAX_MODULE_DEPTH {
        let Some(module) = declaration(&file) else {
            break;
        };
        let parent = extract::normalize_path(module.location.file());
        // Rust rejects modules that contain themselves; the chain ends there.
        if !seen.insert(parent.clone()) {
            break;
        }
        parts.push((
            module.location.file(),
            &module.defaults[..],
            &module.default_line_numbers[..],
        ));
        file = parent;
    }
    let mut expanded = Vec::new();
    for (file, lines, numbers) in parts.into_iter().rev() {
        let mut starts: Vec<usize> = lines
            .iter()
            .enumerate()
            .filter(|(i, line)| *i == 0 || line.trim() == "@route-defaults")
            .map(|(i, _)| i)
            .collect();
        starts.push(lines.len());
        for range in starts.windows(2) {
            let (start, end) = match range {
                [start, end] => (*start, *end),
                _ => continue,
            };
            let block = lines.get(start..end).unwrap_or_default();
            let line = numbers.get(start).copied();
            let location =
                line.map_or_else(|| route.location.clone(), |line| Location::new(file, line));
            let mut expand = || -> Vec<(String, Location)> {
                let text = block.join("\n");
                let expanded = lexical::expand_block(
                    &text,
                    registry,
                    variables,
                    &location,
                    diagnostics,
                    dsl::yaml_lines,
                );
                let block_numbers = numbers.get(start..end).unwrap_or_default();
                let lines = source_lines(&expanded, block_numbers, &location);
                expanded
                    .into_iter()
                    .zip(lines)
                    .map(|((text, _), line)| (text, Location::new(file, line)))
                    .collect()
            };
            match line {
                Some(line) => {
                    let key = (file.to_path_buf(), line);
                    expanded.extend(cache.entry(key).or_insert_with(expand).iter().cloned());
                }
                None => expanded.extend(expand()),
            }
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

/// Removes `@extend` marker keys (prefix [`EXTEND_KEY`]) that no stage
/// resolved, at any depth, so they never reach an output.
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

/// Runs the pipeline on in-memory sources (for tests and tools), with the
/// built-in type table, signature inference and no package root. Returns
/// the merged document, in the dialect of its root, and every finding.
///
/// # Errors
/// [`Error::Parse`] for a Rust source that is not valid Rust,
/// [`Error::Yaml`] for an invalid YAML or JSON snippet and
/// [`Error::MultipleRootsFound`] when several snippets declare `openapi` and
/// `info`.
///
/// # Examples
///
/// ```
/// use oas_forge::generator::generate_from_sources;
/// use oas_forge::source::{SourceFile, SourceKind};
/// use oas_forge::template::Variables;
/// use serde_json::json;
///
/// let source = SourceFile::new(
///     "src/lib.rs",
///     SourceKind::Rust,
///     "/// @openapi\npub struct Tag { pub name: String }\n",
/// );
/// let (document, diagnostics) = generate_from_sources(vec![source], &Variables::new())?;
/// assert_eq!(
///     document,
///     json!({
///         "components": {
///             "schemas": {
///                 "Tag": {
///                     "type": "object",
///                     "properties": { "name": { "type": "string" } },
///                     "required": ["name"]
///                 }
///             }
///         }
///     })
/// );
/// assert!(diagnostics.is_empty());
/// # Ok::<(), oas_forge::Error>(())
/// ```
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
    let prepared = prepare(&SourceSet::from_files(files), settings, &mut diagnostics)?;
    let finish = Finish {
        scope: None,
        package: None,
        strict_lints: false,
    };
    let documents = assemble(&prepared, finish, &mut diagnostics)?;
    Ok((documents.document, diagnostics))
}
