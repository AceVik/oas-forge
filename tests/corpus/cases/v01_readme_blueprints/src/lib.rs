//! @openapi
//! openapi: 3.1.0
//! info:
//!   title: Corpus
//!   version: 1.0.0

/// Paginated response wrapper.
/// @openapi<T>
pub struct PaginatedResponse<T> {
  /// @openapi example: 100
  pub total_count: u32,

  /// @openapi
  /// example: 1
  /// minimum: 1
  pub page: u32,

  /// @openapi
  /// example: 10
  /// minimum: 1
  /// maximum: 1000
  pub limit: u32,

  pub data: Vec<T>,
}

/// @openapi
pub struct User {
    pub id: u32,
}

/// @route GET /users
/// @return 200: $PaginatedResponse<User> "Success"
pub fn users() {}
