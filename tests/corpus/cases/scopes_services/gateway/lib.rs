//! @openapi
//! @scope public
//! openapi: 3.1.0
//! info:
//!   title: Gateway
//!   version: 1.0.0
//! servers:
//!   - url: https://api.example.com
//! tags:
//!   - name: Users
//!     description: Accounts
//!   - name: Reports
//!     description: Reports about accounts
//!   - name: Internal
//!     description: Not used by any public route
//!
//! @openapi
//! @scope admin
//! openapi: 3.1.0
//! info:
//!   title: Gateway administration
//!   version: 1.0.0
//! servers:
//!   - url: http://127.0.0.1:9000
//!
//! @openapi
//! components:
//!   securitySchemes:
//!     bearer: { type: http, scheme: bearer }
//!     admin_token: { type: apiKey, in: header, name: X-Admin-Token }

use serde::{Deserialize, Serialize};

/// A user account.
/// @openapi
#[derive(Serialize)]
pub struct User {
    pub id: u64,
    pub name: String,
}

/// A report about a user.
/// @openapi
#[derive(Serialize, Deserialize)]
pub struct Report {
    pub id: u64,
    pub user: u64,
    pub reason: String,
}

/// The state of the gateway.
/// @openapi
#[derive(Serialize)]
pub struct GatewayHealth {
    pub ok: bool,
}

/// The public listener.
/// @route-defaults
/// @scope public
/// @security bearer
pub mod public {
    /// Gateway health
    /// @route GET /health
    /// @security none
    /// @return 200: $GatewayHealth
    pub async fn gateway_health() {}

    /// Get a user
    /// @route GET /users/{id: u64}
    /// @tag Users
    /// @return 200: $User
    /// @problem 404 "No such user"
    pub async fn get_user() {}

    /// Report a user
    /// @route POST /reports
    /// @tag Reports
    /// @body $Report
    /// @return 202:
    pub async fn report_user() {}
}

/// The loopback admin listener.
/// @route-defaults
/// @scope admin
/// @security admin_token
pub mod admin {
    /// List reports
    /// @route GET /reports
    /// @return 200: Vec<$Report>
    pub async fn list_reports() {}

    /// Delete a user
    /// @route DELETE /users/{id: u64}
    /// @return 204:
    pub async fn delete_user() {}
}
