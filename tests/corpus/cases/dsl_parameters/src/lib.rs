//! @openapi
//! openapi: 3.1.0
//! info:
//!   title: Corpus
//!   version: 1.0.0

/// @route GET /orgs/{org: String "Organization slug"}/users/{id}
/// @path-param id: u64 "User id"
/// @query-param limit: Option<u32> "Page size"
/// @query-param sort: required deprecated example="name" "Sort key"
/// @query-param tags: [String]
/// @query-param filter: "Free text filter"
/// @header-param X-Request-Id: Uuid required "Tracing id"
/// @cookie-param session: String
/// @return 204: "No content"
pub fn get_user() {}
