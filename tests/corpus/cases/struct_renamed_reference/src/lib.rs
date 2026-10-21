//! @openapi
//! openapi: 3.1.0
//! info:
//!   title: Corpus
//!   version: 1.0.0

/// @openapi
#[serde(rename = "UserProfile")]
pub struct Profile {
    pub bio: String,
}

/// @openapi
pub struct User {
    pub profile: Profile,
}
