//! @openapi
//! openapi: 3.1.0
//! info:
//!   title: Gateway
//!   version: 1.0.0

mod routes;

use baylee_catalog::Printing;
use baylee_cards::pool::*;

/// Not documented: a reference to it must not find the catalog's `Note`.
pub struct Note;

/// A pool of cards.
/// @openapi
#[derive(serde::Serialize)]
pub struct Pool {
    /// Imported with `use`: the catalog's (renamed) printing.
    pub printings: Vec<Printing>,
    /// Written with its path.
    pub other: baylee_cardtext::Printing,
    /// Through a glob import.
    pub cards: Vec<PoolCard>,
    /// Two crates document a `Status`; neither is imported here.
    pub status: Status,
    /// The catalog's, by path.
    pub sold: baylee_catalog::Status,
    /// The undocumented `Note` of this module.
    pub note: Note,
}
