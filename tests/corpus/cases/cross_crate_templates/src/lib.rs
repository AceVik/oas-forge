//! @openapi
//! openapi: 3.1.0
//! info:
//!   title: Corpus
//!   version: 1.0.0

/// @openapi
pub struct Invoice {
    pub amount: Money,
}

/// @route GET /invoices
/// @return 200: $Paginated<Invoice> "Invoices"
pub fn list_invoices() {}

/// @openapi
/// paths:
///   /invoices/{id}:
///     get:
///       responses:
///         '500':
///           @insert Problem("internal")
pub fn invoice() {}
