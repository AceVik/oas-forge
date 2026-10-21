//! @openapi
//! openapi: 3.1.0
//! info:
//!   title: Corpus
//!   version: 1.0.0

/// Identifier of a user.
/// @openapi format: uuid
pub type UserId = String;

pub type Timestamp = chrono::DateTime<chrono::Utc>;

/// @openapi
/// minimum: 0
pub type Count = u32;

/// @openapi
pub struct Event {
    pub user: UserId,
    pub at: Timestamp,
    pub count: Count,
}
