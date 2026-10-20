//! Unit tests for `oas-forge`.
//!
//! The module layout mirrors `src/`: `tests/unit/<module>.rs` tests
//! `src/<module>.rs` through its public API. No test code lives in `src/`.

#[path = "../common/mod.rs"]
mod common;

mod config;
mod doc_parser;
mod dsl;
mod error;
mod generics;
mod index;
mod merger;
mod preprocessor;
mod scanner;
mod type_mapper;
mod visitor;
