//! @openapi
//! openapi: 3.1.0
//! info:
//!   title: Corpus
//!   version: 1.0.0

/// @route GET /items/{id: u64}
/// @return 200: $Item "Found"
/// @return 301: () "Moved"
/// @return 304: unit "Not modified"
/// @return 404: "Missing"
/// @return 400: { type: object, properties: { reason: { type: string } } } "Bad request"
/// @return 409: { "type": "string" }
/// @return 422: Option<Item>
/// @return 500: Json<Problem> "Server error"
pub fn get_item() {}

/// @openapi
pub struct Item {
    pub id: u64,
}

/// @openapi
pub struct Problem {
    pub title: String,
}
