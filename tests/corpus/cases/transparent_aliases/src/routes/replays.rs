use axum::Json;
use axum::http::StatusCode;

type Refusal = (StatusCode, Json<ErrorBody>);

/// A recorded game.
/// @openapi
#[derive(serde::Serialize)]
pub struct Replay {
    pub id: u64,
    /// When the game ended.
    pub recorded: Stamp,
    pub seats: Seats,
    pub owner: Owner,
}

/// Get a replay
/// @route GET /replays/{id: u64}
/// @return 200: $Replay
/// @return 404: $ErrorBody
pub async fn replay() -> Result<Json<Replay>, Refusal> {
    todo!()
}
