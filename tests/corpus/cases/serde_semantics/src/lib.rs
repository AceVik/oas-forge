//! @openapi
//! openapi: 3.1.0
//! info:
//!   title: Corpus
//!   version: 1.0.0

use std::collections::HashMap;

/// An event about any item.
/// @openapi
#[serde(tag = "type")]
pub enum Event<T> {
    Created { item: T },
    Deleted { id: u64 },
}

/// One page of results.
/// @openapi
pub struct Page<T> {
    pub items: Vec<Wrapper<T>>,
}

/// @openapi
pub struct Wrapper<T> {
    pub value: T,
}

pub type Paged<T> = Page<T>;

/// A user.
/// @openapi
#[serde(tag = "kind", deny_unknown_fields)]
pub struct User {
    pub id: u64,
    #[serde(rename = "user_name")]
    pub user_name: String,
}

/// @openapi
#[derive(serde_repr::Serialize_repr)]
#[repr(u8)]
pub enum Level {
    Low = 1,
    High = 3,
}

/// @openapi
pub enum Color {
    /// The old name of red.
    #[deprecated]
    Crimson,
    Red,
}

/// @openapi
pub struct Settings {
    pub name: String,
    #[serde(flatten)]
    pub level: Wrapper<Level>,
    #[serde(flatten)]
    pub extra: HashMap<String, String>,
    #[serde(with = "chrono::serde::ts_seconds_option")]
    pub changed: Option<chrono::DateTime<chrono::Utc>>,
}

/// @openapi
pub struct Pair(pub u8, #[serde(skip)] pub u8);

/// List events
/// @route GET /events
/// @return 200: $Paged<Event<User>>
pub async fn events() {}

/// The settings
/// @route GET /settings
/// @return 200: $Settings
/// @return 201: $Color
/// @return 202: $Pair
pub async fn settings() {}
