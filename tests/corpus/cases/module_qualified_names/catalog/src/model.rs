/// A printing in the catalog.
/// @openapi rename "CatalogPrinting"
#[derive(serde::Serialize)]
pub struct Printing {
    pub set: String,
    pub status: Status,
}

/// Whether a printing is sold.
/// @openapi
#[derive(serde::Serialize)]
pub enum Status {
    Sold,
    Retired,
}
