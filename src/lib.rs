// The README is the front page on GitHub, crates.io and docs.rs; its Rust
// examples run as doctests. The library overview follows it.
#![doc = include_str!("../README.md")]
//!
//! # The library
//!
//! [`Generator`] is the entry point: configure inputs and outputs, then call
//! [`Generator::generate`] (build and write) or [`Generator::build`], which
//! returns a [`Spec`] with the documents in memory, every [`Diagnostic`] and
//! [`Spec::check`] for CI. [`Config`] loads the same settings from
//! `openapi.toml` or `[package.metadata.oas-forge]`, and [`Package`] the root
//! document from `Cargo.toml`.
//!
//! ```no_run
//! let spec = oas_forge::Generator::new()
//!     .input("src")
//!     .output("openapi.yaml")
//!     .output_as(oas_forge::OutputKind::Swagger20, "swagger.json")
//!     .build()?;
//! for diagnostic in spec.diagnostics() {
//!     eprintln!("{diagnostic}");
//! }
//! spec.write()?;
//! # Ok::<(), oas_forge::Error>(())
//! ```
//!
//! ## Pipeline
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
//!    and render the outputs ([`OutputKind`], [`Format`]).
//!
//! Problems are reported as [`Diagnostic`]s with a stable [`Code`], a
//! [`Severity`] and a source [`Location`]; only problems that make the result
//! unusable are [`Error`]s. Files are written atomically and only when their
//! content changes.

#![forbid(unsafe_code)]
#![deny(missing_docs, rustdoc::broken_intra_doc_links)]
// Every item is documented, internals included; fallible public functions
// say when they fail; identifiers in docs are code (`clippy.toml` lists the
// product names that are not).
#![deny(
    clippy::missing_docs_in_private_items,
    clippy::missing_errors_doc,
    clippy::doc_markdown
)]
// No panics on user input (AGENTS.md, rule 1): no unchecked indexing or
// slicing, no `unwrap`. `expect` is reserved for invariants, with the reason.
#![deny(
    clippy::indexing_slicing,
    clippy::string_slice,
    clippy::unwrap_used,
    clippy::dbg_macro,
    clippy::todo,
    clippy::unimplemented,
    clippy::print_stdout,
    clippy::print_stderr
)]
// The README shows complete build scripts, `fn main` included.
#![allow(clippy::needless_doctest_main)]

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
pub mod proto;
pub mod schema;
pub(crate) mod scope;
pub mod source;
pub mod template;
pub(crate) mod yaml;

pub use config::Config;
pub use diagnostics::{Code, Diagnostic, Diagnostics, Location, Severity};
pub use error::{Error, Result};
pub use generator::{Generator, Spec};
pub use output::{Format, OutputKind};
pub use package::Package;
