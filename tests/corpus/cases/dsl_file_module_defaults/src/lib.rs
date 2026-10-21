//! @openapi
//! openapi: 3.1.0
//! info:
//!   title: Corpus
//!   version: 1.0.0
//! components:
//!   securitySchemes:
//!     bearer: { type: http, scheme: bearer }

/// All routes need a session.
/// @route-defaults
/// @security bearer
/// @problem 401 "Not signed in"
pub mod routes;

/// @route-defaults
/// @tag Legacy
#[path = "legacy/handlers.rs"]
pub mod legacy;
