//! Command-line interface of `oas-forge`.
//!
//! The binary is a thin wrapper around [`run`]; this library target exists so
//! argument handling and configuration layering can be tested without
//! spawning a process.

#![forbid(unsafe_code)]
#![deny(
    missing_docs,
    clippy::missing_docs_in_private_items,
    clippy::missing_errors_doc,
    clippy::doc_markdown
)]
#![deny(
    clippy::indexing_slicing,
    clippy::string_slice,
    clippy::unwrap_used,
    clippy::dbg_macro,
    clippy::todo,
    clippy::unimplemented
)]

use clap::{Parser, ValueEnum};
use oas_forge::{Config, Diagnostic, Error, Generator, Package, Severity};
use std::path::{Path, PathBuf};

/// Command-line arguments of `oas-forge`.
#[derive(Debug, Clone, Default, Parser)]
#[command(name = "oas-forge", version, about, long_about = None)]
pub struct Cli {
    /// Input directories to scan for Rust files and OpenAPI fragments
    #[arg(short = 'i', long = "input")]
    pub input: Option<Vec<PathBuf>>,

    /// Specific files to include (.yaml, .json or .proto)
    #[arg(short = 'I', long = "include")]
    pub include: Option<Vec<PathBuf>>,

    /// Output file(s) for the generated OpenAPI definition (defaults to openapi.yaml if no output of any kind is set)
    #[arg(short = 'o', long = "output", alias = "out")]
    pub output: Option<Vec<PathBuf>>,

    /// Output file(s) for just the components/schemas section
    #[arg(long = "output-schemas")]
    pub output_schemas: Option<Vec<PathBuf>>,

    /// Output file(s) for just the paths section
    #[arg(long = "output-paths")]
    pub output_paths: Option<Vec<PathBuf>>,

    /// Output file(s) for the full spec minus root details (fragments)
    #[arg(long = "output-fragments")]
    pub output_fragments: Option<Vec<PathBuf>>,

    /// Output file(s) for the component schemas as a JSON Schema 2020-12 bundle
    #[arg(long = "output-json-schema")]
    pub output_json_schema: Option<Vec<PathBuf>>,

    /// Output file(s) for the specification lowered to OpenAPI 3.0.3
    #[arg(long = "output-openapi-30")]
    pub output_openapi_30: Option<Vec<PathBuf>>,

    /// Output file(s) for the specification converted to Swagger 2.0
    #[arg(long = "output-swagger2")]
    pub output_swagger2: Option<Vec<PathBuf>>,

    /// Output file(s) for the AsyncAPI document (@asyncapi and @channel blocks)
    #[arg(long = "output-asyncapi")]
    pub output_asyncapi: Option<Vec<PathBuf>>,

    /// Write the document of this scope (`@scope`) to every output
    #[arg(long = "scope", value_name = "NAME")]
    pub scope: Option<String>,

    /// Path to a configuration file (toml)
    #[arg(long = "config")]
    pub config: Option<PathBuf>,

    /// Value of a `{{NAME}}` placeholder (repeatable)
    #[arg(long = "var", value_name = "NAME=VALUE", value_parser = parse_variable)]
    pub var: Vec<(String, String)>,

    /// Environment variable `{{env:NAME}}` placeholders may read (repeatable)
    #[arg(long = "allow-env", value_name = "NAME")]
    pub allow_env: Vec<String>,

    /// Write nothing; fail when an output is not up to date (for CI)
    #[arg(long = "check")]
    pub check: bool,

    /// Treat warnings as errors
    #[arg(long = "deny-warnings")]
    pub deny_warnings: bool,

    /// Also report unused components and missing descriptions
    #[arg(long = "strict")]
    pub strict: bool,

    /// How findings are printed
    #[arg(long = "message-format", value_enum, default_value_t = MessageFormat::Human)]
    pub message_format: MessageFormat,
}

/// How findings are printed.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, ValueEnum)]
pub enum MessageFormat {
    /// Colored text for terminals.
    #[default]
    Human,
    /// One JSON object per line (for editors and CI tools).
    Json,
}

/// `NAME=VALUE`
fn parse_variable(text: &str) -> Result<(String, String), String> {
    match text.split_once('=') {
        Some((name, value)) if !name.trim().is_empty() => {
            Ok((name.trim().to_string(), value.to_string()))
        }
        _ => Err(format!("`{text}` is not NAME=VALUE")),
    }
}

impl Cli {
    /// Resolves the effective configuration for working directory `dir`.
    ///
    /// Precedence from low to high: `Cargo.toml` metadata, `openapi.toml`,
    /// the file given with `--config`, command-line flags. The lists and
    /// tables (`allow_env`, `variables`, `types`, `files`) add up instead.
    /// When no output of any kind is configured, the full specification
    /// goes to `openapi.yaml`.
    ///
    /// # Errors
    /// [`Error::Config`] when a configuration file exists but cannot be read
    /// or parsed, or the `--config` file is missing.
    ///
    /// # Examples
    ///
    /// ```
    /// use clap::Parser;
    /// use oas_forge_cli::Cli;
    /// use std::path::PathBuf;
    ///
    /// let dir = tempfile::tempdir()?;
    /// std::fs::write(dir.path().join("openapi.toml"), r#"input = ["src"]"#)?;
    ///
    /// let cli = Cli::parse_from(["oas-forge", "--output-schemas", "schemas.yaml"]);
    /// let config = cli.resolve_config(dir.path())?;
    /// assert_eq!(config.input, Some(vec![PathBuf::from("src")]));
    /// // An explicit output: no default `openapi.yaml`.
    /// assert_eq!(config.output, None);
    /// # Ok::<(), Box<dyn std::error::Error>>(())
    /// ```
    pub fn resolve_config(&self, dir: &Path) -> oas_forge::Result<Config> {
        let mut config = Config::load_from(dir)?;
        if let Some(file) = &self.config {
            config.merge(Config::from_file(dir.join(file))?);
        }
        config.merge(self.flags());
        if config.outputs().next().is_none() {
            config.output = Some(vec![PathBuf::from("openapi.yaml")]);
        }
        Ok(config)
    }

    /// The settings given as command-line flags, as the top configuration
    /// layer; flags that are not given stay `None`.
    fn flags(&self) -> Config {
        let mut flags = Config::default();
        flags.input = self.input.clone();
        flags.include = self.include.clone();
        flags.output = self.output.clone();
        flags.output_schemas = self.output_schemas.clone();
        flags.output_paths = self.output_paths.clone();
        flags.output_fragments = self.output_fragments.clone();
        flags.output_json_schema = self.output_json_schema.clone();
        flags.output_openapi_30 = self.output_openapi_30.clone();
        flags.output_swagger2 = self.output_swagger2.clone();
        flags.output_asyncapi = self.output_asyncapi.clone();
        flags.scope = self.scope.clone();
        if !self.var.is_empty() {
            flags.variables = Some(self.var.iter().cloned().collect());
        }
        if !self.allow_env.is_empty() {
            flags.allow_env = Some(self.allow_env.clone());
        }
        flags
    }
}

/// What a successful run produced.
#[derive(Debug, Clone)]
pub struct Outcome {
    /// Every configured output file.
    pub outputs: Vec<PathBuf>,
    /// Warnings of the run.
    pub diagnostics: Vec<Diagnostic>,
    /// With `--check`: the outputs that are not up to date (nothing is
    /// written).
    pub outdated: Vec<PathBuf>,
}

/// Generates every configured output.
///
/// Paths in `cli` and in the configuration files are used as given, so
/// relative paths are relative to the current directory.
///
/// # Errors
/// Any generator error; [`Error::Diagnostics`] when a diagnostic has error severity.
///
/// # Examples
///
/// ```
/// use oas_forge_cli::{Cli, run};
///
/// let dir = tempfile::tempdir()?;
/// std::fs::write(dir.path().join("lib.rs"), "/// @openapi\npub struct Tag { pub name: String }\n")?;
/// let schemas = dir.path().join("schemas.yaml");
///
/// let cli = Cli {
///     input: Some(vec![dir.path().to_path_buf()]),
///     output_schemas: Some(vec![schemas.clone()]),
///     ..Cli::default()
/// };
/// let outcome = run(&cli, dir.path())?;
/// assert_eq!(outcome.outputs, [schemas.clone()]);
/// assert!(std::fs::read_to_string(&schemas)?.starts_with("Tag:\n"));
/// # Ok::<(), Box<dyn std::error::Error>>(())
/// ```
pub fn run(cli: &Cli, dir: &Path) -> oas_forge::Result<Outcome> {
    let config = cli.resolve_config(dir)?;
    // The package comes first, so configured variables override its
    // `CARGO_PKG_*` values.
    let mut generator = Generator::new();
    let manifest = dir.join("Cargo.toml");
    if manifest.is_file()
        && let Some(package) = Package::from_manifest(&manifest)?
    {
        generator = generator.package(package);
    }
    let generator = generator
        .with_config(config)
        .deny_warnings(cli.deny_warnings)
        .strict_lints(cli.strict);
    let spec = generator.build()?;
    if spec.diagnostics().has_errors() {
        return Err(Error::Diagnostics(spec.diagnostics().clone().into_vec()));
    }
    let diagnostics = spec
        .diagnostics()
        .iter()
        .filter(|d| d.severity() != Severity::Note)
        .cloned()
        .collect();
    if cli.check {
        return Ok(Outcome {
            outputs: Vec::new(),
            diagnostics,
            outdated: spec.check()?,
        });
    }
    let outputs = spec.write()?;
    Ok(Outcome {
        outputs,
        diagnostics,
        outdated: Vec::new(),
    })
}
