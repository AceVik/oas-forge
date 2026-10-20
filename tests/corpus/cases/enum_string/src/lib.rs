//! @openapi
//! openapi: 3.1.0
//! info:
//!   title: Corpus
//!   version: 1.0.0

/// Order state
/// @openapi
#[serde(rename_all = "snake_case")]
pub enum OrderState {
    Pending,
    InTransit,
    #[serde(rename = "done")]
    Delivered,
    /// @openapi rename "cancelled_by_user"
    Cancelled,
}

/// @openapi
/// x-extensible: true
pub enum Plain {
    A,
    B,
}

/// Not exported.
pub enum Hidden {
    X,
}
