//! @openapi
//! openapi: 3.1.0
//! info:
//!   title: Corpus
//!   version: 1.0.0

/// @route GET /items/{id}
pub fn missing_param() {}

/// @route GET /items
/// @path-param id: u32
pub fn unused_param() {}

/// @route GET
pub fn no_path() {}
