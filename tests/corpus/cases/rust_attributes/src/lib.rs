//! @openapi
//! openapi: 3.1.0
//! info:
//!   title: Corpus
//!   version: 1.0.0

/// An account.
/// @example { id: 1, login: ada }
/// @openapi
#[deprecated(note = "use Profile")]
pub struct Account {
    pub id: u32,
    /// The login name.
    /// @example ada
    /// @example "grace"
    pub login: String,
    #[deprecated]
    pub legacy: Option<String>,
    /// @openapi skip
    pub internal_token: String,
}

/// @openapi skip
pub struct Hidden {
    pub secret: String,
}

/// An event.
/// @openapi
#[serde(tag = "type")]
pub enum Event {
    /// @example { type: Created, id: 7 }
    Created { id: u32 },
    #[deprecated]
    Renamed { id: u32, name: String },
    /// @openapi skip
    Internal,
}

/// Gets the account.
/// @route GET /account
/// @return 200: $Account
#[deprecated]
pub fn account() {}

/// Lists events.
/// @route GET /events
/// @return 200: Vec<$Event>
pub fn events() {}
