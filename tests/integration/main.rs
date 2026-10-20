//! Integration tests: drive the public `Generator` API end-to-end against
//! real files in temporary directories and compare the written output.

#[path = "../common/mod.rs"]
mod common;

mod cross_crate;
mod examples;
mod generator;
mod pipeline;
