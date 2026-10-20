//! @openapi
//! openapi: 3.1.0
//! info:
//!   title: Corpus
//!   version: 1.0.0

/// @openapi
pub struct User {
    pub id: i64,
}

/// @openapi
/// paths:
///   /refs:
///     get:
///       description: Returns $User objects and costs \$5
///       responses:
///         '200':
///           description: OK
///           content:
///             application/json:
///               schema:
///                 oneOf:
///                   - $ref: $User
///                   - $ref: "$User"
///                   - $ref: $Unknown
pub fn refs() {}
