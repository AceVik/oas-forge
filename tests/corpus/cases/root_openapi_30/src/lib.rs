//! @openapi
//! openapi: 3.0.3
//! info:
//!   title: Legacy API
//!   version: 1.0.0

/// @openapi
pub struct Order {
    pub id: u64,
    pub note: Option<String>,
    pub customer: Option<Customer>,
    pub position: (f64, f64),
    #[validate(range(exclusive_min = 0, exclusive_max = 100))]
    pub discount: f32,
    pub tags: Vec<Option<String>>,
    pub nothing: (),
}

/// @openapi
pub struct Customer {
    pub name: String,
}

/// @openapi
#[serde(untagged)]
pub enum Reference {
    Id(u64),
    Missing,
}

/// @route GET /orders/{id: u64}
/// @return 200: Option<$Order> "The order, if any"
pub fn get_order() {}
