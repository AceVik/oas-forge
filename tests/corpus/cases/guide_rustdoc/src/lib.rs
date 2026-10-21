//! @openapi
//! ```yaml
//! openapi: 3.1.0
//! info:
//!   title: Gallery
//!   version: 1.0.0
//! ```
//!
//! @openapi-fragment `Paging(max)`
//! ```yaml
//! - name: page
//!   in: query
//!   schema: { type: integer, minimum: 1, maximum: {{max}} }
//! ```

use serde::Serialize;

/// A piece of art.
/// @openapi
/// ```yaml
/// x-table: art
/// ```
#[derive(Serialize)]
pub struct Art {
    pub id: u64,
    pub title: String,
}

/// List the art
///
/// @route GET `/art`
/// @query-param crop: `String` enum=[`art_crop`, `art_full`] default=`art_full` "Crop"
/// @return 200: `Vec<$Art>` "The art"
/// @external-docs <https://example.com/art>
/// @openapi
/// ```yaml
/// parameters:
///   @insert `Paging(100)`
/// ```
pub async fn list_art() {}
