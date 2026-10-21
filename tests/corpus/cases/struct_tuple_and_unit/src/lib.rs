//! @openapi
//! openapi: 3.1.0
//! info:
//!   title: Corpus
//!   version: 1.0.0

/// @openapi
pub struct UserId(pub u64);

/// @openapi
pub struct Pair(pub String, pub i32);

/// @openapi
/// type: string
/// enum: [a, b]
pub struct Virtual;

/// @openapi
pub struct Marker;
