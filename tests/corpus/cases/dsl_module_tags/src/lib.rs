//! @openapi
//! openapi: 3.1.0
//! info:
//!   title: Corpus
//!   version: 1.0.0

/// @openapi
/// tags: [Admin]
pub mod admin {
    /// @route GET /admin/stats
    /// @return 200: "ok"
    pub fn stats() {}
}
