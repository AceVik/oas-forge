//! @openapi
//! @scope feedback
//! openapi: 3.1.0
//! info:
//!   title: Feedback
//!   version: 2.0.0

use serde::{Deserialize, Serialize};

/// Feedback about the service.
/// @openapi
#[derive(Serialize, Deserialize)]
pub struct Feedback {
    pub text: String,
    pub rating: u8,
}

/// The state of the feedback service.
/// @openapi
#[derive(Serialize)]
pub struct FeedbackHealth {
    pub ok: bool,
    pub queued: u32,
}

/// Feedback routes.
/// @route-defaults
/// @scope feedback
pub mod routes {
    /// Feedback health
    /// @route GET /health
    /// @return 200: $FeedbackHealth
    pub async fn feedback_health() {}

    /// Send feedback
    /// @route POST /feedback
    /// @body $Feedback
    /// @return 204:
    pub async fn send_feedback() {}

    /// Feedback received as reports
    /// @route GET /reports
    /// @return 200: Vec<$Feedback>
    pub async fn feedback_reports() {}
}
