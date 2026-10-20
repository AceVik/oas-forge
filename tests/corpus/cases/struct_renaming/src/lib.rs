//! @openapi
//! openapi: 3.1.0
//! info:
//!   title: Corpus
//!   version: 1.0.0

/// @openapi
#[serde(rename_all = "camelCase")]
pub struct Account {
    pub first_name: String,
    #[serde(rename = "mail")]
    pub email_address: String,
    /// @openapi rename "nick"
    pub nick_name: String,
}

/// @openapi rename "PublicProfile"
/// @openapi rename-all kebab-case
pub struct Profile {
    pub display_name: String,
}

/// @openapi
#[serde(rename = "Renamed", rename_all = "SCREAMING_SNAKE_CASE")]
pub struct Original {
    pub some_value: i32,
}
