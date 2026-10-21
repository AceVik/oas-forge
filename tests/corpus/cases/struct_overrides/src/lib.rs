//! @openapi
//! openapi: 3.1.0
//! info:
//!   title: Corpus
//!   version: 1.0.0

/// Docs become the description.
/// @openapi
/// description: Overridden description
/// x-internal: true
pub struct Overridden {
    /// @openapi example: "abc"
    pub code: String,
    /// Amount of items.
    /// @openapi
    /// minimum: 1
    /// example: 42
    pub count: u32,
    /// @openapi format: [unclosed
    pub broken: String,
}

/// @openapi description: inline header override
pub struct Inline {
    pub flag: bool,
}
