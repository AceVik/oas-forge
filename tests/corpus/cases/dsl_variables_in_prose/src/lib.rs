//! @openapi
//! openapi: 3.1.0
//! info:
//!   title: Corpus
//!   version: {{VERSION}}
//!
//! @asyncapi
//! asyncapi: 3.0.0
//! info:
//!   title: Status events
//!   version: {{VERSION}}

/// Versioned endpoints.
/// @route-defaults
/// @query-param limit: {{COUNT}}
/// x-api-version: {{VERSION}}
pub mod api {
    /// Since: {{VERSION}}
    ///
    /// Version: {{VERSION}}
    /// @route GET /status
    /// @return 200: {{COUNT}}
    /// x-since: {{VERSION}}
    /// @openapi path
    /// x-path-version: {{VERSION}}
    pub fn status() {}

    /// @route GET /history
    /// @description
    /// Released: {{VERSION}}
    /// tags: {{VERSION}}
    /// @return 204:
    pub fn history() {}
}

/// Since: {{VERSION}}
/// @webhook statusChanged
/// @body {{COUNT}}
/// x-since: {{VERSION}}
pub fn status_changed() {}

/// Since: {{VERSION}}
/// Version: {{VERSION}}
/// @channel status
/// @send Status: {{COUNT}}
pub fn publish_status() {}
