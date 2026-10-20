/// @openapi
/// openapi: 3.1.0
/// info:
///   title: Secure API
///   version: 1.0.0
/// components:
///   securitySchemes:
///     # Define a JWT Bearer scheme
///     BearerAuth:
///       type: http
///       scheme: bearer
///       bearerFormat: JWT
///     # Define an OAuth2/OIDC scheme
///     OidcAuth:
///       type: openIdConnect
///       openIdConnectUrl: [https://auth.example.com/.well-known/openid-configuration](https://auth.example.com/.well-known/openid-configuration)
/// # Apply BearerAuth globally to all routes (optional)
/// security:
///   - BearerAuth: []
pub fn create_app() {}
