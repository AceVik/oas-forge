//! Package metadata as the fallback root document.
//!
//! A crate without an `openapi`/`info` root still gets a complete
//! specification: the root is generated from the package's name, version,
//! description, license and homepage or repository. A root written in the
//! sources always wins.

use crate::error::{Error, Result};
use serde_json::{Map, Value, json};
use std::path::{Path, PathBuf};

/// The OpenAPI version of a generated root document.
pub const ROOT_VERSION: &str = "3.1.1";

/// Package metadata (the `[package]` section of `Cargo.toml`).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
#[non_exhaustive]
pub struct Package {
    /// `name`
    pub name: String,
    /// `version`
    pub version: String,
    /// `description`
    pub description: Option<String>,
    /// `license` (an SPDX expression)
    pub license: Option<String>,
    /// `repository`
    pub repository: Option<String>,
    /// `homepage`
    pub homepage: Option<String>,
}

impl Package {
    /// A package with a name and a version.
    pub fn new(name: impl Into<String>, version: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            version: version.into(),
            ..Self::default()
        }
    }

    /// The package of the build script that is running: Cargo sets
    /// `OUT_DIR` and `CARGO_PKG_*` for build scripts. `None` elsewhere.
    pub fn from_build_env() -> Option<Self> {
        std::env::var_os("OUT_DIR")?;
        Self::from_env()
    }

    /// The package described by the `CARGO_PKG_*` environment variables.
    pub fn from_env() -> Option<Self> {
        let var = |name: &str| std::env::var(name).ok().filter(|v| !v.is_empty());
        Some(Self {
            name: var("CARGO_PKG_NAME")?,
            version: var("CARGO_PKG_VERSION").unwrap_or_else(|| "0.0.0".into()),
            description: var("CARGO_PKG_DESCRIPTION"),
            license: var("CARGO_PKG_LICENSE"),
            repository: var("CARGO_PKG_REPOSITORY"),
            homepage: var("CARGO_PKG_HOMEPAGE"),
        })
    }

    /// The `[package]` of a `Cargo.toml`; fields inherited with
    /// `field.workspace = true` are read from the workspace manifest above
    /// it. `None` for a manifest without `[package]` (a virtual workspace).
    ///
    /// # Errors
    /// [`Error::Config`] when the manifest cannot be read or parsed.
    pub fn from_manifest(path: impl AsRef<Path>) -> Result<Option<Self>> {
        let path = path.as_ref();
        let manifest = read_manifest(path)?;
        let Some(package) = manifest.get("package") else {
            return Ok(None);
        };
        let workspace = workspace_package(path);
        let field = |name: &str| -> Option<String> {
            match package.get(name)? {
                toml::Value::String(value) => Some(value.clone()),
                toml::Value::Table(table)
                    if table.get("workspace").and_then(toml::Value::as_bool) == Some(true) =>
                {
                    workspace.as_ref()?.get(name)?.as_str().map(str::to_string)
                }
                _ => None,
            }
        };
        let Some(name) = field("name") else {
            return Err(Error::config(path, "`[package]` has no `name`"));
        };
        Ok(Some(Self {
            name,
            version: field("version").unwrap_or_else(|| "0.0.0".into()),
            description: field("description"),
            license: field("license"),
            repository: field("repository"),
            homepage: field("homepage"),
        }))
    }

    /// The root document: `openapi`, `info` and `externalDocs`.
    pub fn root_document(&self) -> Value {
        let mut info = Map::new();
        info.insert("title".into(), json!(self.name));
        info.insert("version".into(), json!(self.version));
        if let Some(description) = &self.description {
            info.insert("description".into(), json!(description));
        }
        if let Some(license) = &self.license {
            info.insert(
                "license".into(),
                json!({ "name": license, "identifier": license }),
            );
        }
        let mut root = json!({ "openapi": ROOT_VERSION, "info": info });
        if let Some(url) = self.homepage.as_ref().or(self.repository.as_ref()) {
            root["externalDocs"] = json!({ "url": url });
        }
        root
    }
}

fn read_manifest(path: &Path) -> Result<toml::Table> {
    let text = std::fs::read_to_string(path).map_err(|e| Error::config(path, e.to_string()))?;
    toml::from_str(&text).map_err(|e| Error::config(path, e.to_string()))
}

/// `[workspace.package]` of the nearest manifest with a `[workspace]` at or
/// above `manifest`.
fn workspace_package(manifest: &Path) -> Option<toml::Table> {
    let mut dir: Option<PathBuf> = manifest.parent().map(Path::to_path_buf);
    while let Some(current) = dir {
        let candidate = current.join("Cargo.toml");
        if let Ok(table) = read_manifest(&candidate)
            && let Some(workspace) = table.get("workspace")
        {
            return workspace.get("package")?.as_table().cloned();
        }
        dir = current.parent().map(Path::to_path_buf);
    }
    None
}
