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
    /// Files receiving the component schemas as a JSON Schema 2020-12 bundle.
    pub output_json_schema: Option<Vec<PathBuf>>,
    /// Files receiving the specification lowered to OpenAPI 3.0.3.
    pub output_openapi_30: Option<Vec<PathBuf>>,
    /// Files receiving the specification converted to Swagger 2.0.
    pub output_swagger2: Option<Vec<PathBuf>>,
    /// Files receiving the AsyncAPI document.
    pub output_asyncapi: Option<Vec<PathBuf>>,
    /// Schemas for Rust types, by type name or path, as with
    /// [`Generator::map_type`](crate::Generator::map_type):
    ///
    /// ```toml
    /// [types]
    /// UserId = { type = "string", format = "uuid" }
    /// ```
    pub types: Option<BTreeMap<String, Value>>,
    /// Values of `{{NAME}}` placeholders, as with
    /// [`Generator::var`](crate::Generator::var):
    ///
    /// ```toml
    /// [variables]
    /// API_URL = "https://api.example.com"
    /// ```
    pub variables: Option<BTreeMap<String, String>>,
    /// The configuration files the settings were read from.
    #[serde(skip)]
    files: Vec<PathBuf>,
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
            if let Some(mut found) = parsed
                .package
                .and_then(|p| p.metadata)
                .and_then(|m| m.oas_forge)
            {
                found.files.push(manifest);
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
        let mut config: Self =
            toml::from_str(&read(path)?).map_err(|e| Error::config(path, e.to_string()))?;
        config.files.push(path.to_path_buf());
        Ok(config)
    }

    /// The files the settings were read from (`Cargo.toml` when it has
    /// `[package.metadata.oas-forge]`, `openapi.toml`, a `--config` file), so
    /// a build script can rerun when they change
    /// ([`Generator::with_config`](crate::Generator::with_config) watches them).
    pub fn files(&self) -> &[PathBuf] {
        &self.files
    }

    /// Parses a configuration in the `openapi.toml` format.
    pub fn from_toml_str(text: &str) -> Result<Self> {
        toml::from_str(text).map_err(|e| Error::config(Path::new("<inline>"), e.to_string()))
    }

    /// Overlays `other` on `self`: every field that is set in `other`
    /// replaces the value in `self`; `types` and `variables` are merged by
    /// name.
    pub fn merge(&mut self, other: Config) {
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
            files,
        } = other;
        for file in files {
            if !self.files.contains(&file) {
                self.files.push(file);
            }
        }
        if let Some(types) = types {
            self.types.get_or_insert_with(BTreeMap::new).extend(types);
        }
        if let Some(variables) = variables {
            self.variables
                .get_or_insert_with(BTreeMap::new)
                .extend(variables);
        }
        overlay(&mut self.input, input);
        overlay(&mut self.include, include);
        overlay(&mut self.output, output);
        overlay(&mut self.output_schemas, output_schemas);
        overlay(&mut self.output_paths, output_paths);
        overlay(&mut self.output_fragments, output_fragments);
        overlay(&mut self.output_json_schema, output_json_schema);
        overlay(&mut self.output_openapi_30, output_openapi_30);
        overlay(&mut self.output_swagger2, output_swagger2);
        overlay(&mut self.output_asyncapi, output_asyncapi);
    }

    /// Every configured output file, across all output strategies.
    pub fn outputs(&self) -> impl Iterator<Item = &PathBuf> {
        [
            &self.output,
            &self.output_schemas,
            &self.output_paths,
            &self.output_fragments,
            &self.output_json_schema,
            &self.output_openapi_30,
            &self.output_swagger2,
            &self.output_asyncapi,
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
