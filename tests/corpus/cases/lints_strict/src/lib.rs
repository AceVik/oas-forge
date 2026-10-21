//! @openapi
//! openapi: 3.1.0
//! info:
//!   title: Corpus
//!   version: 1.0.0
//! components:
//!   securitySchemes:
//!     bearer: { type: http, scheme: bearer }
//!   responses:
//!     Orphan: { description: Never used }

/// Described and used.
/// @openapi
pub struct User {
    pub id: u32,
}

/// @openapi
pub struct Undescribed {
    pub id: u32,
}

/// @openapi
pub struct Lonely {
    pub next: Option<Box<Lonely>>,
}

/// Gets a user.
/// @route GET /users/{id: u32}
/// @security bearer
/// @return 200: $User
pub fn get_user() {}

/// @route GET /teams/{team: String}
/// @operation-id get_user
/// @security api_key
/// @return 200: $Undescribed
pub fn get_team() {}

/// @openapi
/// paths:
///   /orgs/{org}:
///     get:
///       summary: Path parameter mismatch.
///       parameters:
///         - { name: organization, in: path, required: true, schema: { type: string } }
///       responses: { '204': { description: Nothing } }
pub fn orgs() {}
