//! @openapi
//! openapi: 3.1.0
//! info:
//!   title: Corpus
//!   version: 1.0.0

/// Paginated response wrapper.
/// @openapi<T>
pub struct Page<T> {
    /// @openapi example: 100
    pub total: u32,
    pub data: Vec<T>,
}

/// @openapi<A, B>
/// properties:
///   left: { $ref: $A }
///   right: { $ref: $B }
pub struct Pair<A, B> {
    pub tag: String,
}

/// @openapi
pub struct User {
    pub id: i64,
}

/// @openapi
pub struct Item {
    pub sku: String,
}

/// @route GET /users
/// @return 200: $Page<User> "Users"
pub fn users() {}

/// @openapi
/// paths:
///   /pairs:
///     get:
///       responses:
///         '200':
///           description: OK
///           content:
///             application/json:
///               schema:
///                 $ref: $Pair<User, Item>
///         '201':
///           description: Nested
///           content:
///             application/json:
///               schema:
///                 $ref: $Page<$Page<Item>>
///         '202':
///           description: Missing blueprint
///           content:
///             application/json:
///               schema:
///                 $ref: $Missing<User>
pub fn pairs() {}
