//! @openapi
//! openapi: 3.1.0
//! info:
//!   title: Corpus
//!   version: 1.0.0

/// @openapi
pub struct Envelope<T> {
    pub data: T,
    pub items: Vec<T>,
}

/// @openapi
pub struct User {
    pub id: i64,
}

/// @openapi
pub struct Response {
    pub user: Envelope<User>,
}
