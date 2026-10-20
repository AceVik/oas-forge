//! @openapi
//! openapi: 3.1.0
//! info:
//!   title: Strategies
//!   version: 1.0.0
//! servers:
//!   - url: https://api.example.com
//! tags:
//!   - name: Users

//! @openapi-fragment Problem(status)
//! description: Problem {{status}}

/// @openapi<T>
/// type: object
/// properties:
///   items: { type: array, items: { $ref: $T } }
pub struct Page<T> {
    pub total: u64,
}

/// @openapi
pub struct User {
    pub id: i64,
}

/// @route GET /users
/// @tag Users
/// @return 200: $Page<User> "Users"
pub fn users() {}
