//! @openapi
//! openapi: 3.1.0
//! info:
//!   title: Secure API
//!   version: 1.0.0
//! components:
//!   securitySchemes:
//!     BearerAuth:
//!       type: http
//!       scheme: bearer
//!       bearerFormat: JWT
//! security:
//!   - BearerAuth: []

/// @route GET /me
/// @return 200: "ok"
pub fn me() {}
