//! @openapi
//! openapi: 3.1.0
//! info:
//!   title: Corpus
//!   version: 1.0.0

/// @openapi
#[serde(untagged)]
pub enum IdOrName {
    Id(u64),
    Name(String),
}
