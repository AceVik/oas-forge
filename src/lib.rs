#![doc = include_str!("../README.md")]
#![allow(clippy::collapsible_if)]
pub mod config;
pub mod dsl;
pub mod error;
pub mod generics;
pub mod index;
pub mod merger;
pub mod preprocessor;
pub mod scanner;
pub mod visitor;

use config::Config;
use error::Result;
use std::path::PathBuf;

/// Main entry point for generating OpenAPI definitions.
/// Main entry point for generating OpenAPI definitions.
#[derive(Default)]
pub struct Generator {
    inputs: Vec<PathBuf>,
    includes: Vec<PathBuf>,
    outputs: Vec<PathBuf>,
    schema_outputs: Vec<PathBuf>,
    path_outputs: Vec<PathBuf>,
    fragment_outputs: Vec<PathBuf>,
}

impl Generator {
    /// Creates a new Generator instance.
    pub fn new() -> Self {
        Self::default()
    }

    /// Configures the generator from a Config object.
    pub fn with_config(mut self, config: Config) -> Self {
        if let Some(inputs) = config.input {
            self.inputs.extend(inputs);
        }
        if let Some(includes) = config.include {
            self.includes.extend(includes);
        }
        if let Some(output) = config.output {
            self.outputs.extend(output);
        }
        if let Some(output_schemas) = config.output_schemas {
            self.schema_outputs.extend(output_schemas);
        }
        if let Some(output_paths) = config.output_paths {
            self.path_outputs.extend(output_paths);
        }
        if let Some(output_fragments) = config.output_fragments {
            self.fragment_outputs.extend(output_fragments);
        }
        self
    }

    /// Adds an input directory to scan.
    pub fn input<P: Into<PathBuf>>(mut self, path: P) -> Self {
        self.inputs.push(path.into());
        self
    }

    /// Adds a specific file to include.
    pub fn include<P: Into<PathBuf>>(mut self, path: P) -> Self {
        self.includes.push(path.into());
        self
    }

    /// Appends an output file path.
    pub fn output<P: Into<PathBuf>>(mut self, path: P) -> Self {
        self.outputs.push(path.into());
        self
    }

    /// Appends an output file path for just the schemas.
    pub fn output_schemas<P: Into<PathBuf>>(mut self, path: P) -> Self {
        self.schema_outputs.push(path.into());
        self
    }

    /// Appends an output file path for just the paths.
    pub fn output_paths<P: Into<PathBuf>>(mut self, path: P) -> Self {
        self.path_outputs.push(path.into());
        self
    }

    /// Appends an output file path for full spec minus root details (fragments).
    pub fn output_fragments<P: Into<PathBuf>>(mut self, path: P) -> Self {
        self.fragment_outputs.push(path.into());
        self
    }

    /// Executes the generation process.
    pub fn generate(self) -> Result<()> {
        if self.outputs.is_empty()
            && self.schema_outputs.is_empty()
            && self.path_outputs.is_empty()
            && self.fragment_outputs.is_empty()
        {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                "At least one output path (output, output_schemas, output_paths, or output_fragments) is required",
            )
            .into());
        }

        // 1. Scan and Extract
        log::info!(
            "Scanning directories: {:?} and includes: {:?}",
            self.inputs,
            self.includes
        );
        let snippets = scanner::scan_directories(&self.inputs, &self.includes)?;

        // 2. Merge (Relaxed - may return empty map if no root)
        log::info!("Merging {} snippets", snippets.len());
        let merged_value = merger::merge_openapi(snippets)?;

        // Strategy 1: Full Spec (Strict Validation)
        if !self.outputs.is_empty() {
            if let serde_yaml::Value::Mapping(map) = &merged_value {
                let openapi_key = serde_yaml::Value::String("openapi".to_string());
                let info_key = serde_yaml::Value::String("info".to_string());

                if !map.contains_key(&openapi_key) || !map.contains_key(&info_key) {
                    return Err(error::Error::NoRootFound);
                }
            } else {
                return Err(error::Error::NoRootFound);
            }

            for output in &self.outputs {
                self.write_file(output, &merged_value)?;
                log::info!("Written full spec to {:?}", output);
            }
        }

        // Strategy 2: Schemas Only (Relaxed)
        if !self.schema_outputs.is_empty() {
            let schemas = merged_value
                .get("components")
                .and_then(|c| c.get("schemas"))
                .cloned()
                .unwrap_or_else(|| serde_yaml::Value::Mapping(serde_yaml::Mapping::new()));

            if let serde_yaml::Value::Mapping(m) = &schemas {
                if m.is_empty() {
                    log::warn!("Generating empty schemas file.");
                }
            }

            for output in &self.schema_outputs {
                self.write_file(output, &schemas)?;
                log::info!("Written schemas to {:?}", output);
            }
        }

        // Strategy 3: Paths Only (Relaxed)
        if !self.path_outputs.is_empty() {
            let paths = merged_value
                .get("paths")
                .cloned()
                .unwrap_or_else(|| serde_yaml::Value::Mapping(serde_yaml::Mapping::new()));

            if let serde_yaml::Value::Mapping(m) = &paths {
                if m.is_empty() {
                    log::warn!("Generating empty paths file.");
                }
            }

            for output in &self.path_outputs {
                self.write_file(output, &paths)?;
                log::info!("Written paths to {:?}", output);
            }
        }

        // Strategy 4: Fragments (Headless Spec)
        // Removes top-level keys: openapi, info, servers, externalDocs
        // Keeps: paths, components, tags, security, etc.
        if !self.fragment_outputs.is_empty() {
            let mut fragment = merged_value.clone();
            if let serde_yaml::Value::Mapping(ref mut map) = fragment {
                map.remove(&serde_yaml::Value::String("openapi".to_string()));
                map.remove(&serde_yaml::Value::String("info".to_string()));
                map.remove(&serde_yaml::Value::String("servers".to_string()));
            }

            for output in &self.fragment_outputs {
                self.write_file(output, &fragment)?;
                log::info!("Written fragment to {:?}", output);
            }
        }

        Ok(())
    }

    fn write_file<T: serde::Serialize>(&self, path: &PathBuf, content: &T) -> Result<()> {
        // Ensure parent directory exists
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }

        let file = std::fs::File::create(path)?;
        let extension = path.extension().and_then(|s| s.to_str()).unwrap_or("yaml");

        match extension {
            "json" => {
                serde_json::to_writer_pretty(file, content)?;
            }
            "yaml" | "yml" => {
                serde_yaml::to_writer(file, content)?;
            }
            _ => {
                serde_yaml::to_writer(file, content)?;
            }
        }
        Ok(())
    }
}
