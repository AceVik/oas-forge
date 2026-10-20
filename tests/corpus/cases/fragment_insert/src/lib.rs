//! @openapi
//! openapi: 3.1.0
//! info:
//!   title: Corpus
//!   version: 1.0.0

/// @route GET /list
/// @return 200: "ok"
/// parameters:
///   @insert Paging
pub fn list() {}

/// @openapi
/// paths:
///   /manual:
///     get:
///       parameters:
///         @insert Paging
///         @insert ExternalParam
///       responses:
///         '400':
///           @insert Error("Bad Request")
///         '401':
///           @insert Error
pub fn manual() {}
