//! @openapi
//! openapi: 3.1.0
//! info:
//!   title: Corpus
//!   version: 1.0.0

/// @openapi
pub struct Account {
    #[garde(length(chars, min = 3, max = 20), ascii)]
    pub username: String,
    #[garde(length(min = 8, max = 64))]
    pub password: String,
    #[garde(email)]
    pub email: String,
    #[garde(range(min = 18, max = 130))]
    pub age: u8,
    #[garde(prefix("+"), length(chars, max = 16))]
    pub phone: String,
    #[garde(pattern(HANDLE))]
    pub handle: String,
    #[garde(inner(length(chars, min = 1)), length(max = 5))]
    pub tags: Vec<String>,
    #[garde(required, inner(url))]
    pub homepage: Option<String>,
    #[garde(pattern(UNKNOWN))]
    pub unresolved: String,
}
