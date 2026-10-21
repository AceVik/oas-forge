/// @openapi
/// openapi: 3.1.1
/// info:
///   title: User Management API
///   version: {{CARGO_PKG_VERSION}}
///   description: API documentation for the user service.
/// components:
///   securitySchemes:
///     oidcAuth:
///       type: openIdConnect
///       openIdConnectUrl: $$OIDC_URL/.well-known/openid-configuration
///       description: OIDC Authentication via Rauthy.
pub fn create_app() {}
