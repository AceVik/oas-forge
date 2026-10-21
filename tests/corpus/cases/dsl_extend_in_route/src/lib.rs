//! @openapi
//! openapi: 3.1.0
//! info:
//!   title: Corpus
//!   version: 1.0.0

//! @openapi-fragment Secured(role)
//! security:
//!   - oidc: [{{role}}]
//! responses:
//!   '401':
//!     description: Unauthorized
