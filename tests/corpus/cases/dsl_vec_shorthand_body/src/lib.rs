//! @openapi
//! openapi: 3.1.0
//! info:
//!   title: Corpus
//!   version: 1.0.0

/// @route POST /users
/// @body $Vec<User>
/// @return 200: $Vec<User> "Users"
pub fn create_many() {}

/// @openapi
pub struct User {
    pub id: i64,
}
