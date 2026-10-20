//! @openapi
//! openapi: 3.1.0
//! info:
//!   title: Corpus
//!   version: 1.0.0

/// A change event.
/// @openapi
#[serde(tag = "kind", content = "payload", rename_all = "kebab-case", rename_all_fields = "camelCase")]
pub enum Event {
    /// Created.
    UserCreated { user_id: u64, display_name: String },
    Moved(f64, f64),
    #[serde(skip)]
    Internal,
    Ping,
}

/// @openapi
pub enum Command {
    Start,
    Stop,
    Configure { max_items: u32 },
    #[serde(untagged)]
    Raw(String),
}

/// @openapi
#[serde(tag = "type")]
pub enum Shape {
    Circle(Circle),
    #[serde(rename = "sq")]
    Square { side: f64 },
}

/// @openapi
pub struct Circle {
    pub radius: f64,
}
