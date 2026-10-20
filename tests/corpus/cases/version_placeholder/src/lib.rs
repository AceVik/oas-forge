//! @openapi
//! openapi: 3.1.0
//! info:
//!   title: Versioned
//!   version: '{{CARGO_PKG_VERSION}}'
//!   description: Built from version {{CARGO_PKG_VERSION}}

/// @openapi
/// x-unquoted-version: {{CARGO_PKG_VERSION}}
pub fn meta() {}
