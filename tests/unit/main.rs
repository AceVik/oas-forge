//! Unit tests for `oas-forge`.
//!
//! The module layout mirrors `src/`: `tests/unit/<module>.rs` tests
//! `src/<module>.rs` through its public API. No test code lives in `src/`.

#[path = "../common/mod.rs"]
mod common;

mod attrs;
mod config;
mod diagnostics;
mod docs;
mod dsl;
mod error;
mod extract;
mod generator;
mod lint;
mod merge;
mod output;
mod package;
mod proto;
mod schema;
mod source;
mod template;
mod yaml;
