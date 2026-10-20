//! @openapi
//! openapi: 3.1.0
//! info:
//!   title: Corpus
//!   version: 1.0.0

/// A registered user.
/// Spans two lines.
/// @openapi
pub struct User {
    /// Unique identifier
    pub id: u64,
    pub name: String,
    pub email: Option<String>,
    pub roles: Vec<Role>,
    pub settings: std::collections::HashMap<String, String>,
    pub address: Address,
    pub manager: Option<Box<User>>,
}

/// @openapi
pub struct Address {
    pub street: String,
    pub city: String,
}

/// @openapi
pub enum Role {
    Admin,
    Member,
}

/// Not exported: no export directive.
pub struct Internal {
    pub secret: String,
}
