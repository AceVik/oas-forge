//! @openapi
//! openapi: 3.1.0
//! info:
//!   title: Shop ({{STAGE}})
//!   version: {{API_VERSION}}
//! servers:
//!   - url: https://{{STAGE}}.example.com
//! x-built-by: {{env:CARGO_PKG_NAME}}
//! x-missing: '{{env:OAS_FORGE_CORPUS_UNSET}}'

/// @openapi
/// x-stage: {{STAGE}}
pub struct Config {
    /// @example {{API_VERSION}}
    pub version: String,
    /// @openapi
    /// description: Unknown placeholders stay, like {{NOT_SET}}.
    pub other: String,
}

/// Reads the configuration.
/// @route GET /config
/// @description Served by {{STAGE}}.
/// @return 200: $Config
pub fn config() {}
