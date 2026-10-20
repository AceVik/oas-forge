//! @openapi
//! openapi: 3.1.0
//! info:
//!   title: Corpus
//!   version: 1.0.0

/// @openapi
#[serde(rename_all = "camelCase")]
pub struct Session {
    #[serde(with = "chrono::serde::ts_seconds")]
    pub started_at: DateTime<Utc>,
    #[serde(with = "time::serde::rfc3339::option")]
    pub ended_at: Option<OffsetDateTime>,
    #[serde(with = "humantime_serde")]
    pub timeout: std::time::Duration,
    pub r#ref: String,
    #[serde(flatten)]
    pub extra: std::collections::HashMap<String, serde_json::Value>,
}

/// @openapi
#[serde(transparent)]
pub struct Token {
    #[validate(length(min = 32, max = 32))]
    pub value: String,
}

/// @openapi
#[serde(into = "String")]
pub struct Version {
    pub major: u16,
    pub minor: u16,
}

/// @openapi
pub struct Point(pub f64, #[serde(skip)] pub u8, pub f64);
