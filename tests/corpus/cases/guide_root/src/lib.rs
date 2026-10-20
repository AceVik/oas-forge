//! @openapi
//! openapi: 3.1.1
//! info:
//!   title: Accounts
//!   version: '{{CARGO_PKG_VERSION}}'
//!   description: |
//!     User accounts and sessions.
//!
//!     Built from revision `{{GIT_REVISION}}`.
//! servers:
//!   - url: https://{{STAGE}}.example.com
//! security:
//!   - session: []
//! components:
//!   securitySchemes:
//!     session:
//!       type: openIdConnect
//!       openIdConnectUrl: $$ISSUER_URL/.well-known/openid-configuration

/// Who is signed in
/// @route GET /me
/// @return 200: String
pub async fn me() {}
