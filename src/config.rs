//! Generator configuration and its layered loading from `Cargo.toml` and
//! `openapi.toml`.
//!
//! The same settings can be given programmatically through [`crate::Generator`],
//! as a [`Config`] value, in `[package.metadata.oas-forge]` of `Cargo.toml`, in a
//! dedicated `openapi.toml` file or on the command line of `oas-forge-cli`.

use crate::error::{Error, Result};
use serde::Deserialize;
use serde_json::Value;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

/// Settings of a generator run.
///
/// Every field is optional; `None` means "not configured here", so layers can
/// be combined with [`Config::merge`]. TOML keys use the field names, e.g.
/// `output_schemas = ["schemas.yaml"]`.
#[derive(Debug, Deserialize, Default, Clone, PartialEq, Eq)]
#[serde(default, deny_unknown_fields)]
#[non_exhaustive]
pub struct Config {
    /// Directories scanned recursively for Rust sources and YAML/JSON files.
    pub input: Option<Vec<PathBuf>>,
    /// Single YAML/JSON files merged into the result.
    pub include: Option<Vec<PathBuf>>,
    /// Files receiving the full specification.
    pub output: Option<Vec<PathBuf>>,
    /// Files receiving only `components.schemas`.
    pub output_schemas: Option<Vec<PathBuf>>,
    /// Files receiving only `paths`.
    pub output_paths: Option<Vec<PathBuf>>,
    /// Files receiving the headless specification (no root) for other crates.
    pub output_fragments: Option<Vec<PathBuf>>,
    /// Schemas for Rust types, by type name or path, as with
    /// [`Generator::map_type`](crate::Generator::map_type):
    ///
    /// ```toml
    /// [types]
    /// UserId = { type = "string", format = "uuid" }
    /// ```
    pub types: Option<BTreeMap<String, Value>>,
}

/// The `Cargo.toml` subset that carries `[package.metadata.oas-forge]`.
#[derive(Deserialize)]
struct CargoManifest {
    package: Option<CargoPackage>,
}

#[derive(Deserialize)]
struct CargoPackage {
    metadata: Option<CargoMetadata>,
}

#[derive(Deserialize)]
struct CargoMetadata {
    #[serde(rename = "oas-forge")]
    oas_forge: Option<Config>,
}

impl Config {
    /// Loads the configuration of the current directory, see [`Config::load_from`].
    ///
    /// In a build script the current directory is the package root, so this
    /// reads the package's own `Cargo.toml` and `openapi.toml`.
    pub fn load() -> Result<Self> {
        Self::load_from(".")
    }

    /// Loads `[package.metadata.oas-forge]` from `dir/Cargo.toml` and then
    /// `dir/openapi.toml`; settings in `openapi.toml` win.
    ///
    /// Missing files are skipped. Files that exist but cannot be read or
    /// parsed are an [`Error::Config`].
    pub fn load_from(dir: impl AsRef<Path>) -> Result<Self> {
        let dir = dir.as_ref();
        let mut config = Self::default();

        let manifest = dir.join("Cargo.toml");
        if manifest.is_file() {
            let parsed: CargoManifest = toml::from_str(&read(&manifest)?)
                .map_err(|e| Error::config(&manifest, e.to_string()))?;
            if let Some(found) = parsed
                .package
                .and_then(|p| p.metadata)
                .and_then(|m| m.oas_forge)
            {
                config.merge(found);
            }
        }

        let dedicated = dir.join("openapi.toml");
        if dedicated.is_file() {
            config.merge(Self::from_file(&dedicated)?);
        }
        Ok(config)
    }

    /// Reads a configuration file in the `openapi.toml` format.
    pub fn from_file(path: impl AsRef<Path>) -> Result<Self> {
        let path = path.as_ref();
        Self::from_toml_str(&read(path)?).map_err(|e| match e {
            Error::Config { message, .. } => Error::config(path, message),
            other => other,
        })
    }

    /// Parses a configuration in the `openapi.toml` format.
    pub fn from_toml_str(text: &str) -> Result<Self> {
        toml::from_str(text).map_err(|e| Error::config(Path::new("<inline>"), e.to_string()))
    }

    /// Overlays `other` on `self`: every field that is set in `other`
    /// replaces the value in `self`; `types` are merged by name.
    pub fn merge(&mut self, other: Config) {
        let Config {
            input,
            include,
            output,
            output_schemas,
            output_paths,
            output_fragments,
            types,
        } = other;
        if let Some(types) = types {
            self.types.get_or_insert_with(BTreeMap::new).extend(types);
        }
        overlay(&mut self.input, input);
        overlay(&mut self.include, include);
        overlay(&mut self.output, output);
        overlay(&mut self.output_schemas, output_schemas);
        overlay(&mut self.output_paths, output_paths);
        overlay(&mut self.output_fragments, output_fragments);
    }

    /// Every configured output file, across all output strategies.
    pub fn outputs(&self) -> impl Iterator<Item = &PathBuf> {
        [
            &self.output,
            &self.output_schemas,
            &self.output_paths,
            &self.output_fragments,
        ]
        .into_iter()
        .flatten()
        .flatten()
    }
}

fn overlay<T>(target: &mut Option<T>, value: Option<T>) {
    if value.is_some() {
        *target = value;
    }
}

fn read(path: &Path) -> Result<String> {
    std::fs::read_to_string(path).map_err(|e| Error::config(path, e.to_string()))
}
