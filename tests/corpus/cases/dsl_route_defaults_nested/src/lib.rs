//! @openapi
//! openapi: 3.1.0
//! info:
//!   title: Corpus
//!   version: 1.0.0
//! components:
//!   securitySchemes:
//!     bearer: { type: http, scheme: bearer }
//!     admin_key: { type: apiKey, in: header, name: X-Admin-Key }
//!
//! @openapi-fragment RateLimited(limit)
//! x-rate-limit: {{limit}}

/// The versioned API.
/// @route-defaults
/// @prefix v{{API_VERSION}}/
/// @server {{BASE}}
/// @security bearer
/// @tag Api
/// @extend RateLimited(100)
/// x-audience: public
pub mod api {
    /// List tenants
    /// @route GET /tenants
    /// @tag Tenants
    /// @tag Directory
    /// x-audience: internal
    /// @return 200: Vec<String>
    pub fn list_tenants() {}

    /// Tenant administration.
    /// @route-defaults
    /// @prefix tenants/{tenant: String "Tenant slug"}
    /// @security admin_key
    /// @tag Admin
    /// @no-infer
    pub mod admin {
        /// Remove a member
        /// @route DELETE /members/{member: u64}
        /// @return 204:
        pub async fn remove_member(Json(body): Json<String>) {}

        /// A tenant was deleted.
        /// @webhook tenantDeleted
        /// @return 204:
        pub fn tenant_deleted() {}
    }
}
