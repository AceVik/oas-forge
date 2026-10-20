//! @openapi-fragment Secured(role)
//! security:
//!   - oidc: [{{role}}]
//! summary: From fragment
//! responses:
//!   '401':
//!     description: Unauthorized
