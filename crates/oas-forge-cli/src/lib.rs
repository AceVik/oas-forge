//! Command-line interface of `oas-forge`.
//!
//! The binary is a thin wrapper around [`run`]; this library target exists so
//! argument handling and configuration layering can be tested without
//! spawning a process.

use clap::Parser;
use oas_forge::{Config, Diagnostic, Error, Generator, Severity};
use std::path::{Path, PathBuf};

/// Command-line arguments of `oas-forge`.
#[derive(Debug, Clone, Default, Parser)]
#[command(name = "oas-forge", version, about, long_about = None)]
pub struct Cli {
    /// Input directories to scan for Rust files and OpenAPI fragments
    #[arg(short = 'i', long = "input")]
    pub input: Option<Vec<PathBuf>>,

    /// Specific files to include (e.g., .json, .yaml)
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

    /// Path to a configuration file (toml)
    #[arg(long = "config")]
    pub config: Option<PathBuf>,
}

impl Cli {
    /// Resolves the effective configuration for working directory `dir`.
    ///
    /// Precedence from low to high: `Cargo.toml` metadata, `openapi.toml`,
    /// the file given with `--config`, command-line flags. When no output of
    /// any kind is configured, the full specification goes to `openapi.yaml`.
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

    fn flags(&self) -> Config {
        let mut flags = Config::default();
        flags.input = self.input.clone();
        flags.include = self.include.clone();
        flags.output = self.output.clone();
        flags.output_schemas = self.output_schemas.clone();
        flags.output_paths = self.output_paths.clone();
        flags.output_fragments = self.output_fragments.clone();
        flags
    }
}

/// What a successful run produced.
#[derive(Debug, Clone)]
pub struct Outcome {
    /// Every configured output file.
    pub outputs: Vec<PathBuf>,
    /// Warnings and notes of the run.
    pub diagnostics: Vec<Diagnostic>,
}

/// Generates every configured output.
///
/// # Errors
/// Any generator error; [`Error::Diagnostics`] when a diagnostic has error severity.
pub fn run(cli: &Cli, dir: &Path) -> oas_forge::Result<Outcome> {
    let config = cli.resolve_config(dir)?;
    let spec = Generator::new().with_config(config).build()?;
    if spec.diagnostics().has_errors() {
        return Err(Error::Diagnostics(spec.diagnostics().clone().into_vec()));
    }
    let outputs = spec.write()?;
    Ok(Outcome {
        outputs,
        diagnostics: spec
            .diagnostics()
            .iter()
            .filter(|d| d.severity() != Severity::Note)
            .cloned()
            .collect(),
    })
}
