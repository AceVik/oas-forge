//! @openapi
//! openapi: 3.1.0
//! info:
//!   title: Corpus
//!   version: 1.0.0

/// @openapi
#[serde(deny_unknown_fields, default)]
pub struct Settings {
    #[serde(skip)]
    pub secret: String,
    #[serde(skip_serializing)]
    pub password: String,
    #[serde(skip_deserializing)]
    pub created_at: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub note: Option<String>,
    #[serde(default)]
    pub retries: u32,
    #[serde(flatten)]
    pub meta: Meta,
    #[serde(alias = "colour")]
    pub color: String,
    pub r#type: String,
}

/// @openapi
pub struct Meta {
    pub created_by: String,
}

/// @openapi
#[serde(transparent)]
pub struct Wrapper {
    pub inner: String,
}
