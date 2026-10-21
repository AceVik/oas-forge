/// List decks
/// @route GET /decks
/// @return 200: Vec<String>
pub async fn list_decks() {}

/// Card routes.
/// @route-defaults
/// @prefix /decks/{deck: u64 "Deck id"}
pub mod cards {
    /// List the cards of a deck
    /// @route GET /cards
    /// @return 200: Vec<String>
    pub async fn list_cards() {}
}
