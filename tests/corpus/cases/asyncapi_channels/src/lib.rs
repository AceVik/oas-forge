//! @openapi
//! openapi: 3.1.0
//! info:
//!   title: Corpus
//!   version: 1.0.0
//!
//! ---
//!
//! @asyncapi
//! defaultContentType: application/json
//! info:
//!   description: Events of the user service.
//!
//! ---
//!
//! @channel heartbeat
//! @send Heartbeat: u64 "Seconds since start"

/// @openapi
pub struct User {
    pub id: u64,
    pub name: Option<String>,
    pub position: (f64, f64),
}

/// @openapi
#[serde(tag = "kind")]
pub enum Change {
    Renamed { name: String },
    Deleted,
}

/// A user signed up.
/// More details about sign-ups.
/// @channel user/{id}/signedup
/// @send UserSignedUp: $User "The new user"
/// @tag Users
pub fn publish_signup() {}

/// @channel changes user/changes
/// @receive UserChanged: $Change
/// @send ChangeAck: { type: object, properties: { ok: { type: boolean } } }
pub fn on_change() {}
