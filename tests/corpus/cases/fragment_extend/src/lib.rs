//! @openapi
//! openapi: 3.1.0
//! info:
//!   title: Corpus
//!   version: 1.0.0

/// @openapi
/// paths:
///   /admin:
///     get:
///       summary: Local summary
///       @extend Secured("admin")
///       responses:
///         '200':
///           description: OK
pub fn admin() {}
