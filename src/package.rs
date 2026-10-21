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
///
/// # Examples
///
/// A package passed to the generator provides the root document when the
/// sources have none:
///
/// ```
/// use oas_forge::{Generator, OutputKind, Package};
///
/// let dir = tempfile::tempdir()?;
/// std::fs::write(
///     dir.path().join("lib.rs"),
///     "/// @openapi\npub struct Product { pub id: u64 }\n",
/// )?;
/// let mut package = Package::new("shop", "1.0.0");
/// package.description = Some("The shop API".into());
///
/// let spec = Generator::new().input(dir.path()).package(package).build()?;
/// let document = spec.render(OutputKind::Full)?;
/// assert_eq!(document["info"]["title"], "shop");
/// assert_eq!(document["info"]["description"], "The shop API");
/// # Ok::<(), Box<dyn std::error::Error>>(())
/// ```
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
    /// The `Cargo.toml` the metadata was read from ([`Package::manifest`]).
    manifest: Option<PathBuf>,
}

impl Package {
    /// A package with a name and a version.
    ///
    /// # Examples
    ///
    /// ```
    /// let package = oas_forge::Package::new("shop", "1.0.0");
    /// assert_eq!(package.name, "shop");
    /// assert_eq!(package.description, None);
    /// ```
    pub fn new(name: impl Into<String>, version: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            version: version.into(),
            ..Self::default()
        }
    }

    /// The package of the build script that is running: Cargo sets
    /// `OUT_DIR` and `CARGO_PKG_*` for build scripts. `None` elsewhere.
    ///
    /// [`Generator::new`](crate::Generator::new) already uses it.
    ///
    /// # Examples
    ///
    /// ```
    /// if let Some(package) = oas_forge::Package::from_build_env() {
    ///     // Only in a build script.
    ///     assert!(!package.name.is_empty());
    /// }
    /// ```
    pub fn from_build_env() -> Option<Self> {
        std::env::var_os("OUT_DIR").and_then(|_| Self::from_env())
    }

    /// The package described by the `CARGO_PKG_*` environment variables,
    /// with the [manifest](Package::manifest) in `CARGO_MANIFEST_DIR`.
    /// `None` without `CARGO_PKG_NAME`; a missing version is `0.0.0`, and
    /// empty variables count as unset.
    ///
    /// # Examples
    ///
    /// ```
    /// use oas_forge::Package;
    ///
    /// // Cargo sets the variables for build scripts, `cargo run` and `cargo test`.
    /// let package = Package::from_env().unwrap_or_else(|| Package::new("shop", "1.0.0"));
    /// assert!(!package.version.is_empty());
    /// ```
    pub fn from_env() -> Option<Self> {
        let var = |name: &str| std::env::var(name).ok().filter(|v| !v.is_empty());
        Some(Self {
            name: var("CARGO_PKG_NAME")?,
            version: var("CARGO_PKG_VERSION").unwrap_or_else(|| "0.0.0".into()),
            description: var("CARGO_PKG_DESCRIPTION"),
            license: var("CARGO_PKG_LICENSE"),
            repository: var("CARGO_PKG_REPOSITORY"),
            homepage: var("CARGO_PKG_HOMEPAGE"),
            manifest: var("CARGO_MANIFEST_DIR").map(|dir| Path::new(&dir).join("Cargo.toml")),
        })
    }

    /// The `[package]` of a `Cargo.toml`; fields inherited with
    /// `field.workspace = true` are read from the workspace manifest above
    /// it. `None` for a manifest without `[package]` (a virtual workspace).
    ///
    /// # Errors
    /// [`Error::Config`] when the manifest cannot be read or parsed, or its
    /// `[package]` has no `name`.
    ///
    /// # Examples
    ///
    /// ```
    /// use oas_forge::Package;
    ///
    /// let workspace = tempfile::tempdir()?;
    /// std::fs::write(
    ///     workspace.path().join("Cargo.toml"),
    ///     r#"
    ///     [workspace]
    ///     members = ["api"]
    ///
    ///     [workspace.package]
    ///     version = "2.1.0"
    ///     license = "MIT"
    ///     "#,
    /// )?;
    /// std::fs::create_dir(workspace.path().join("api"))?;
    /// let manifest = workspace.path().join("api/Cargo.toml");
    /// std::fs::write(
    ///     &manifest,
    ///     r#"
    ///     [package]
    ///     name = "api"
    ///     version.workspace = true
    ///     license.workspace = true
    ///     "#,
    /// )?;
    ///
    /// let package = Package::from_manifest(&manifest)?.expect("a [package] section");
    /// assert_eq!(package.name, "api");
    /// assert_eq!(package.version, "2.1.0");
    /// assert_eq!(package.license.as_deref(), Some("MIT"));
    ///
    /// // The workspace manifest itself has no package.
    /// assert_eq!(Package::from_manifest(workspace.path().join("Cargo.toml"))?, None);
    /// # Ok::<(), Box<dyn std::error::Error>>(())
    /// ```
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
            manifest: Some(path.to_path_buf()),
        }))
    }

    /// The `Cargo.toml` the metadata was read from: the file given to
    /// [`Package::from_manifest`], or `CARGO_MANIFEST_DIR/Cargo.toml` for
    /// [`Package::from_env`]; `None` for a package made with
    /// [`Package::new`]. A [`Generator`](crate::Generator) with the package
    /// watches it ([`Spec::watched`](crate::Spec::watched)), so that a build
    /// script reruns when the metadata of the generated root changes.
    ///
    /// # Examples
    ///
    /// ```
    /// use oas_forge::Package;
    ///
    /// let dir = tempfile::tempdir()?;
    /// let manifest = dir.path().join("Cargo.toml");
    /// std::fs::write(&manifest, "[package]\nname = \"shop\"\nversion = \"1.0.0\"\n")?;
    ///
    /// let package = Package::from_manifest(&manifest)?.expect("a [package] section");
    /// assert_eq!(package.manifest(), Some(manifest.as_path()));
    /// assert_eq!(Package::new("shop", "1.0.0").manifest(), None);
    /// # Ok::<(), Box<dyn std::error::Error>>(())
    /// ```
    pub fn manifest(&self) -> Option<&Path> {
        self.manifest.as_deref()
    }

    /// The `CARGO_PKG_*` variables Cargo sets for this package; like Cargo,
    /// unset fields are empty strings.
    ///
    /// # Examples
    ///
    /// ```
    /// let package = oas_forge::Package::new("shop", "1.4.2-beta.1");
    /// let variables = package.variables();
    /// assert!(variables.contains(&("CARGO_PKG_NAME", "shop".to_string())));
    /// assert!(variables.contains(&("CARGO_PKG_VERSION_MINOR", "4".to_string())));
    /// assert!(variables.contains(&("CARGO_PKG_VERSION_PATCH", "2".to_string())));
    /// assert!(variables.contains(&("CARGO_PKG_LICENSE", String::new())));
    /// ```
    pub fn variables(&self) -> Vec<(&'static str, String)> {
        let mut parts = self.version.split(['.', '-', '+']);
        let mut part = || parts.next().unwrap_or_default().to_string();
        let (major, minor, patch) = (part(), part(), part());
        let field = |value: &Option<String>| value.clone().unwrap_or_default();
        vec![
            ("CARGO_PKG_NAME", self.name.clone()),
            ("CARGO_PKG_VERSION", self.version.clone()),
            ("CARGO_PKG_VERSION_MAJOR", major),
            ("CARGO_PKG_VERSION_MINOR", minor),
            ("CARGO_PKG_VERSION_PATCH", patch),
            ("CARGO_PKG_DESCRIPTION", field(&self.description)),
            ("CARGO_PKG_LICENSE", field(&self.license)),
            ("CARGO_PKG_REPOSITORY", field(&self.repository)),
            ("CARGO_PKG_HOMEPAGE", field(&self.homepage)),
        ]
    }

    /// The root document: `openapi` ([`ROOT_VERSION`]), `info` and, from
    /// `homepage` or else `repository`, `externalDocs`.
    ///
    /// # Examples
    ///
    /// ```
    /// use serde_json::json;
    ///
    /// let mut package = oas_forge::Package::new("shop", "1.0.0");
    /// package.license = Some("MIT".into());
    /// package.repository = Some("https://example.com/shop".into());
    ///
    /// assert_eq!(
    ///     package.root_document(),
    ///     json!({
    ///         "openapi": "3.1.1",
    ///         "info": {
    ///             "title": "shop",
    ///             "version": "1.0.0",
    ///             "license": { "name": "MIT", "identifier": "MIT" }
    ///         },
    ///         "externalDocs": { "url": "https://example.com/shop" }
    ///     })
    /// );
    /// ```
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
        let mut root = Map::new();
        root.insert("openapi".into(), json!(ROOT_VERSION));
        root.insert("info".into(), Value::Object(info));
        if let Some(url) = self.homepage.as_ref().or(self.repository.as_ref()) {
            root.insert("externalDocs".into(), json!({ "url": url }));
        }
        Value::Object(root)
    }
}

/// Reads and parses a `Cargo.toml`; failures are an [`Error::Config`].
fn read_manifest(path: &Path) -> Result<toml::Table> {
    let text = std::fs::read_to_string(path).map_err(|e| Error::config(path, e.to_string()))?;
    toml::from_str(&text).map_err(|e| Error::config(path, e.to_string()))
}

/// `[workspace.package]` of the nearest manifest with a `[workspace]` at or
/// above `manifest`.
fn workspace_package(manifest: &Path) -> Option<toml::Table> {
    // A relative path such as `./Cargo.toml` has no parents to walk up to.
    let manifest = manifest.canonicalize().unwrap_or_else(|_| manifest.into());
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
