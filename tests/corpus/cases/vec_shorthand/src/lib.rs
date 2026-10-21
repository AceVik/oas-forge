//! @openapi
//! openapi: 3.1.0
//! info:
//!   title: Corpus
//!   version: 1.0.0

/// @openapi
pub struct Tag {
    pub name: String,
}

/// @openapi
/// paths:
///   /tags:
///     get:
///       responses:
///         '200':
///           description: OK
///           content:
///             application/json:
///               schema: $Vec<Tag>
pub fn tags() {}
