//! @openapi
//! openapi: 3.1.0
//! info:
//!   title: Corpus
//!   version: 1.0.0

/// List users
/// Returns all users, paginated.
///
/// Second paragraph with **Markdown**.
/// @route GET /users
/// @tag Users, Admin
/// @return 200: Vec<User> "All users"
pub async fn list_users() {}

/// @openapi
pub struct User {
    pub id: i64,
}
