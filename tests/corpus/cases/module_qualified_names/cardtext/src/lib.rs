/// A printing picked for card text.
/// @openapi
#[derive(serde::Serialize)]
pub struct Printing {
    pub face: u8,
    pub status: Status,
}

/// Whether the text is final.
/// @openapi
#[derive(serde::Serialize)]
pub enum Status {
    Draft,
    Final,
}
