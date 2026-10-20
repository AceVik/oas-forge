//! @openapi
//! openapi: 3.1.0
//! info:
//!   title: Corpus
//!   version: 1.0.0

/// @route GET /secure
/// @security bearer
/// @security oauth2("read:users", "write:users")
/// @security apiKey()
/// @return 200: "ok"
pub fn secure() {}
