//! @openapi
//! openapi: 3.1.0
//! info: { title: Admin, version: 1.0.0 }
//! components:
//!   securitySchemes:
//!     oidc: { type: openIdConnect, openIdConnectUrl: 'https://id.example.com/.well-known/openid-configuration' }
//!
//! @openapi-fragment Paging
//! - name: page
//!   in: query
//!   schema: { type: integer, minimum: 1, default: 1 }
//! - name: size
//!   in: query
//!   schema: { type: integer, minimum: 1, maximum: 100, default: 20 }
//!
//! @openapi-fragment Secured(role)
//! security:
//!   - oidc: [{{role}}]
//! responses:
//!   '401': { description: Not signed in }
//!   '403': { description: 'Requires the {{role}} role' }

/// List audit events
/// @route GET /audit
/// @return 200: Vec<String>
/// @extend Secured(admin)
/// parameters:
///   @insert Paging
pub async fn audit() {}

/// @openapi
/// paths:
///   /reports:
///     get:
///       summary: List reports
///       @extend Secured("auditor")
///       responses:
///         '200': { description: The reports }
pub async fn reports() {}
