//! @openapi
//! openapi: 3.1.0
//! info:
//!   title: Corpus
//!   version: 1.0.0

/// @openapi
#[serde(tag = "kind", rename_all = "camelCase", rename_all_fields = "camelCase")]
pub enum Event {
    UserCreated { user_id: i64 },
    #[serde(rename_all = "SCREAMING_SNAKE_CASE")]
    UserDeleted { deleted_at: String },
}
