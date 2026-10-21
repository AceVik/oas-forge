//! @openapi
//! openapi: 3.2.0
//! info:
//!   title: Modern API
//!   version: 1.0.0

/// @openapi
pub struct Filter {
    pub name: Option<String>,
}

/// @openapi
pub struct Event {
    pub kind: String,
}

/// @route QUERY /search
/// @querystring-param filter: $Filter "Search filter"
/// @return 200: Vec<String>
pub fn search() {}

/// @route PURGE /cache
/// @return 204: "Purged"
pub fn purge() {}

/// @route GET /events
/// @return 200: stream<$Event> "Server-sent events"
/// @return 206: stream<$Event> application/jsonl "JSON Lines"
pub fn events() {}

/// @webhook orderCreated
/// @body $Event
/// @return 200: "Received"
pub fn order_created() {}
