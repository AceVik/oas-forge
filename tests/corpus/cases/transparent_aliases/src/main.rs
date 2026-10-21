//! @openapi
//! openapi: 3.1.0
//! info:
//!   title: Gateway
//!   version: 1.0.0

use std::sync::Arc;

/// Shared by every handler.
type Shared = Arc<AppState>;

/// Never documented: a function pointer in a type argument.
type Listening = axum::serve::TapIo<tokio::net::TcpListener, fn(&mut tokio::net::TcpStream)>;

pub(crate) type Admin = Arc<Console>;
pub(crate) type Refusal = (StatusCode, Json<ErrorBody>);

/// What went wrong.
/// @openapi
#[derive(serde::Serialize)]
pub struct ErrorBody {
    pub error: String,
}
