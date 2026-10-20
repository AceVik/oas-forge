//! @openapi
//! openapi: 3.1.0
//! info:
//!   title: Corpus
//!   version: 1.0.0

/// @openapi
#[serde(tag = "t", content = "c")]
pub enum Message {
    Text(String),
    Move { x: i32, y: i32 },
    Ping,
    #[serde(rename = "img")]
    Image(#[validate(length(min = 1))] String),
}
