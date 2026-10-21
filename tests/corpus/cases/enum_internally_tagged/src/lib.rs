//! @openapi
//! openapi: 3.1.0
//! info:
//!   title: Corpus
//!   version: 1.0.0

/// Storage configuration.
/// @openapi
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Storage {
    /// Local disk
    Local { root_path: String },
    S3 { bucket: String, region: Option<String> },
    Disabled,
    Custom(CustomStorage),
}

/// @openapi
pub struct CustomStorage {
    pub url: String,
}
