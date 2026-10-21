//! The catalog: its `Printing` is re-exported from a module.

mod model;
pub use model::{Printing, Status};

/// A note on a printing.
/// @openapi
#[derive(serde::Serialize)]
pub struct Note {
    pub text: String,
}
