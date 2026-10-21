use serde::{Deserialize, Serialize};

/// A note.
/// @openapi
#[derive(Serialize)]
pub struct Note {
    pub id: u64,
    pub text: String,
    pub done: bool,
}

/// What a client sends to add a note.
/// @openapi
#[derive(Deserialize)]
pub struct NewNote {
    /// @example "Buy milk"
    pub text: String,
}

/// List notes
/// @route GET /notes
/// @query-param done: Option<bool> "Only done or only open notes"
/// @return 200: Vec<$Note>
pub async fn list_notes() {}

/// Add a note
/// @route POST /notes
/// @body $NewNote
/// @return 201: $Note "Created"
/// @problem 422 "The text is empty"
pub async fn add_note() {}
