//! @openapi
//! openapi: 3.1.0
//! info:
//!   title: Corpus
//!   version: 1.0.0

/// Health check
/// Returns 200 while the service is up.
/// @openapi
/// paths:
///   /health:
///     get:
///       summary: Health
///       responses:
///         '200':
///           description: OK
pub fn health() {}

/// @openapi
fn empty_block() {}
