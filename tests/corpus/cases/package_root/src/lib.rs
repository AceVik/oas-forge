//! @openapi
//! info:
//!   contact:
//!     name: Shop team
//!     email: team@example.com

/// A product.
/// @openapi
pub struct Product {
    pub id: u32,
    pub name: String,
}

/// Lists products.
/// @route GET /products
/// @return 200: Vec<$Product>
pub fn list() {}
