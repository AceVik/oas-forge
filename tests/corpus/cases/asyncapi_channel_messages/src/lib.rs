//! @asyncapi
//! asyncapi: 3.0.0
//! info:
//!   title: Games
//!   version: 1.0.0

use serde::Serialize;

/// The first frame of a connection.
/// @openapi
#[derive(Serialize)]
pub struct Hello {
    pub session: String,
}

/// A seat at a table.
/// @openapi
#[derive(Serialize)]
pub struct Seat {
    pub table: u32,
    pub seat: u8,
}

/// A move of a player.
/// @openapi
#[derive(Serialize)]
pub struct Move {
    pub card: String,
}

/// The lobby
///
/// Who is waiting for a game.
/// @channel lobby /lobby/ws
/// @send Waiting: Vec<$Seat> "Players waiting"
/// bindings:
///   ws:
///     method: GET
///
/// - one update per change
/// - no history
/// @openapi message Nope
/// x-ignored: true
pub async fn lobby() {}

/// Chat
/// @channel chat /chat
/// @summary Chat lines
/// @send Line: String "A line"
/// @description Every line of the chat.
/// x-not-yaml: [prose after description
/// summary: also prose
pub async fn chat() {}

/// A seat
/// @channel seat /games/{id}/seat
/// @send HelloAck: $Hello "Seated"
/// @send Moved: $Move "A move at the table"
/// @receive Moved: $Move "A move at the table"
/// @receive Move: $Move "The player's move"
/// @receive Move: $Seat "A second definition"
/// parameters:
///   id:
///     description: The game
pub async fn seat() {}

/// Watching a game
/// @channel watch /games/{id}/watch
/// @send HelloAck: $Seat "Watching"
/// summary: Spectators
/// address: /games/{game}/watch
pub async fn watch() {}

/// The tables
/// @channel tables /tables
/// @send HelloAck: $Hello "Seated"
/// @send Moved: $Move "A move at the table"
pub async fn tables() {}
