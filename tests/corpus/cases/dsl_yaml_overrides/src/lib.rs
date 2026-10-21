//! @openapi
//! openapi: 3.1.0
//! info:
//!   title: Corpus
//!   version: 1.0.0

/// Upload a file
/// @route POST /upload
/// @return 200: "ok"
/// requestBody:
///   content:
///     multipart/form-data:
///       schema:
///         type: object
///         properties:
///           file: { type: string, format: binary }
/// externalDocs:
///   url: https://example.com/docs
/// servers:
///   - url: https://upload.example.com
pub fn upload() {}

/// @route GET /mixed
/// @query-param dsl: String
/// @return 200: "ok"
/// parameters:
///   - name: yaml
///     in: query
///     schema: { type: string }
pub fn mixed() {}
