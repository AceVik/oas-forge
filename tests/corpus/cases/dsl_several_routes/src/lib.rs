//! @openapi
//! openapi: 3.1.0
//! info:
//!   title: Corpus
//!   version: 1.0.0

/// CORS preflight
/// @route OPTIONS /client/reports
/// @route OPTIONS /client/alive
/// @return 204:
pub async fn preflight() {}

/// Get a deck by id or by slug
/// @route GET /decks/{id: u64 "Deck id"}
/// @route GET /decks/by-slug/{slug: String "Deck slug"}
/// @tag Decks
/// @return 200: String
pub async fn get_deck() {}

/// A deck changed.
/// @webhook deckCreated
/// @webhook PUT deckChanged
/// @body String
pub async fn on_deck() {}

/// Ping
/// @route GET /ping
/// @return 204:
pub async fn ping() {}
