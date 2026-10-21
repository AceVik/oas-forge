//! @openapi
//! openapi: 3.1.0
//! info:
//!   title: Corpus
//!   version: 1.0.0
//!
//! @openapi-fragment Limited(limit)
//! x-rate-limit: {{limit}}

use serde::{Deserialize, Serialize};

/// A note.
/// @openapi
#[derive(Serialize, Deserialize)]
pub struct Note {
    pub text: String,
}

/// How to page.
/// @openapi
#[derive(Deserialize)]
pub struct Paging {
    pub page: u32,
    pub size: u8,
}

/// Notes of a tenant.
/// @route-defaults
/// @prefix /tenants/{tenant: String "Tenant slug"}/notes
pub mod notes {
    use super::*;

    /// List notes
    /// @route GET /
    /// @extend Limited(10, 20)
    pub async fn list(paging: Option<Query<Paging>>) -> Json<Vec<Note>> {
        todo!()
    }

    /// Get a note
    /// @route GET /{id}
    pub async fn get(Path(path): Option<Path<(String, u64)>>) -> Json<Note> {
        todo!()
    }

    /// Replace a note
    /// @route PUT /{id: u64}
    /// @return 204:
    pub async fn put(body: Option<Json<Note>>) {}
}
