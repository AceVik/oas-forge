//! Compile-time OpenAPI for Rust.
//!
//! `oas-forge` reads Rust source code and its doc comments, typically from a
//! build script, and writes an OpenAPI document: no macros, nothing at
//! runtime. Schemas describe exactly what serde writes, including `rename`,
//! `flatten`, `skip` and all four enum representations, plus the constraints
//! of the `validator` and `garde` crates. Operations come from a compact route
//! DSL that accepts embedded OpenAPI YAML, or from plain YAML.
//!
//! ```no_run
//! // In `fn main` of build.rs:
//! oas_forge::Generator::new()
//!     .input("src")
//!     .output("openapi.yaml")
//!     // `cargo:rerun-if-changed` for every input, findings as warnings.
//!     .emit_cargo_directives(true)
//!     .generate()
//!     .expect("OpenAPI generation failed");
//! ```
//!
//! ```
//! /// A user account.
//! /// @openapi
//! #[derive(serde::Serialize)]
//! #[serde(rename_all = "camelCase")]
//! pub struct User {
//!     pub id: u64,
//!     /// Shown to other users.
//!     pub display_name: Option<String>,
//! }
//!
//! /// Get a user
//! /// @route GET /users/{id: u64 "The user id"}
//! /// @return 200: $User
//! /// @problem 404 "No such user"
//! pub async fn get_user() {}
//! ```
//!
//! # Outputs
//!
//! One run can write the full OpenAPI document (3.2, 3.1 or 3.0, following the
//! `openapi` version of the root), parts of it, an OpenAPI 3.0 or Swagger 2.0
//! export, a JSON Schema bundle of the component schemas and an AsyncAPI 3
//! document; see [`OutputKind`]. Files are written atomically and only when
//! their content changes. [`Spec::check`] tells CI whether the committed files
//! are up to date.
//!
//! # Pipeline
//!
//! [`Generator`] runs these stages; each has its own module with a stable
//! API, so tools can use a single stage on its own:
//!
//! 1. [`source`]: find the input files, in a deterministic order.
//! 2. [`extract`]: parse Rust files with `syn` and collect annotations.
//! 3. [`template`]: `@insert`, `{{variables}}`, `@extend`, generic
//!    instantiation (`$Page<User>`) and smart references (`$User`).
//! 4. [`dsl`]: compile `@route`, `@webhook` and `@channel` blocks.
//! 5. [`merge`]: merge every snippet into one document under explicit
//!    policies.
//! 6. [`schema`] and [`output`]: lower the schemas for the target version
//!    and render the outputs.
//!
//! Problems are reported as [`Diagnostic`]s with a stable [`Code`] and a
//! source [`Location`]; only problems that make the result unusable are
//! [`Error`]s.
//!
//! # Guide
//!
//! The [guide](https://github.com/AceVik/oas-forge/tree/main/docs) covers
//! every annotation, the route DSL, the compatibility tables for serde,
//! validator and garde, the outputs and all diagnostic codes. The
//! [migration guide](https://github.com/AceVik/oas-forge/blob/main/docs/migration-0.2.md)
//! lists the changes since 0.1.

#![forbid(unsafe_code)]
#![deny(missing_docs, rustdoc::broken_intra_doc_links)]

pub mod attrs;
pub mod config;
pub mod diagnostics;
pub(crate) mod docs;
pub(crate) mod document;
pub mod dsl;
pub mod error;
pub mod extract;
pub mod generator;
pub(crate) mod lint;
pub mod merge;
pub mod output;
pub mod package;
pub mod schema;
pub mod source;
pub mod template;
pub(crate) mod yaml;

pub use config::Config;
pub use diagnostics::{Code, Diagnostic, Diagnostics, Location, Severity};
pub use error::{Error, Result};
pub use generator::{Generator, Spec};
pub use output::{Format, OutputKind};
pub use package::Package;
