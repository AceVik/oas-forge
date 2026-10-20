//! @openapi
//! openapi: 3.1.0
//! info:
//!   title: Corpus
//!   version: 1.0.0

/// @openapi
/// paths:
///   /manual/endpoint:
///     get:
///       tags: [Manual]
///       summary: Fully manual definition
///       description: This is standard OpenAPI YAML.
///       responses:
///         '200':
///           description: OK
///           content:
///             text/plain:
///               schema: { type: string }
async fn manual_handler() {}

/// @openapi
/// paths:
///   /docs/openapi.yaml:
///     get:
///       tags: [System]
///       summary: Get OpenAPI Specification
///       description: Returns the dynamic OpenAPI specification.
///       responses:
///         '200':
///           description: The OpenAPI YAML file.
///           content:
///             application/yaml:
///               schema: { type: string }
async fn serve_spec() {}
