//! @openapi
//! openapi: 3.1.0
//! info:
//!   title: Corpus
//!   version: 1.0.0

/// @route POST /users
/// @body $CreateUser
/// @return 201: $User "Created"
pub fn create() {}

/// @route PUT /users/{id: u64}
/// @body CreateUser application/xml
/// @return 200: User
pub fn replace() {}

/// @route POST /bulk
/// @body Vec<CreateUser>
/// @return 204: "ok"
pub fn bulk() {}

/// @route POST /raw
/// @body String text/plain
/// @return 204: "ok"
pub fn raw() {}

/// @route POST /paged
/// @body $Page<CreateUser>
/// @return 204: "ok"
pub fn paged() {}

/// @openapi
pub struct CreateUser {
    pub name: String,
}

/// @openapi
pub struct User {
    pub id: i64,
}

/// @openapi<T>
/// properties:
///   items:
///     type: array
///     items:
///       $ref: $T
pub struct Page<T> {
    pub total: u64,
}
