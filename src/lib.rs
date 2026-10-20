#![doc = include_str!("../README.md")]
#![forbid(unsafe_code)]
#![warn(missing_docs)]

pub mod attrs;
pub mod config;
pub mod diagnostics;
pub(crate) mod docs;
pub mod dsl;
pub mod error;
pub mod extract;
pub mod generator;
pub(crate) mod lint;
pub mod merge;
pub mod output;
pub mod schema;
pub mod source;
pub mod template;
pub(crate) mod yaml;

pub use config::Config;
pub use diagnostics::{Code, Diagnostic, Diagnostics, Location, Severity};
pub use error::{Error, Result};
pub use generator::{Generator, Spec};
pub use output::{Format, OutputKind};
