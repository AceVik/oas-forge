use axum::Json;
use axum::http::StatusCode;

type Refusal = (StatusCode, Json<ErrorBody>);
type Answer<T> = Result<T, Refusal>;

/// Only in this file: `String`.
type Code = String;

/// A seat at a table.
/// @openapi
#[derive(serde::Serialize)]
pub struct Seat {
    pub player: Owner,
    pub code: Code,
}

/// A page of results.
/// @openapi
#[derive(serde::Serialize)]
pub struct Page<T> {
    pub items: Vec<T>,
}

/// List the seats
/// @route GET /seats
/// @return 200: Paged<Seat>
pub async fn seats() -> Answer<Json<Paged<Seat>>> {
    todo!()
}
