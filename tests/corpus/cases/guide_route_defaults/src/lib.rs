//! @openapi
//! openapi: 3.1.0
//! info: { title: Tenants, version: 1.0.0 }
//! components:
//!   securitySchemes:
//!     bearer: { type: http, scheme: bearer }

/// Tenant administration.
/// @route-defaults
/// @prefix /tenants/{tenant}
/// @path-param tenant: String "Tenant slug"
/// @tag Tenants
/// @security bearer
/// @problem 401 "Not signed in"
pub mod tenants {
    /// List users
    /// @route GET /users
    /// @return 200: Vec<String>
    pub fn list_users() {}

    /// Billing endpoints.
    /// @route-defaults
    /// @prefix /billing
    /// @tag Billing
    pub mod billing {
        /// Get the invoice
        /// @route GET /invoices/{id: u64}
        /// @security none
        /// @return 200: String
        pub fn invoice() {}
    }
}
