//! @openapi
//! openapi: 3.1.0
//! info:
//!   title: Corpus
//!   version: 1.0.0

/// @openapi
/// paths:
///   /users:
///     get:
///       parameters:
///         # Inserts the list items directly here
///         @insert PaginationParams
///       responses:
///         '200':
///           description: OK
pub fn users() {}

/// @openapi
/// paths:
///   /admin/dashboard:
///     get:
///       summary: Admin Area
///
///       # Merges security and 401/403 responses into this operation
///       @extend Secured("admin")
///
///       responses:
///         '200':
///           description: OK
pub fn dashboard() {}
