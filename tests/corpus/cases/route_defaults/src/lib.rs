//! @openapi
//! openapi: 3.1.0
//! info:
//!   title: Corpus
//!   version: 1.0.0

/// Tenant-scoped API.
/// @route-defaults
/// @prefix /tenants/{tenant}
/// @param path tenant: String "Tenant slug"
/// @tag Tenants
/// @security bearer
/// @return 401: "Unauthorized"
/// @openapi
/// tags: [{ name: Tenants, description: Everything about tenants }]
pub mod tenants {
    /// @route GET /users
    /// @return 200: Vec<String>
    pub fn list_users() {}

    /// @route-defaults
    /// @prefix /admin
    /// @tag Admin
    pub mod admin {
        /// @route DELETE /users/{id: u64}
        /// @security none
        /// @return 204:
        /// @return 401: "Custom unauthorized"
        pub fn delete_user() {}
    }
}
