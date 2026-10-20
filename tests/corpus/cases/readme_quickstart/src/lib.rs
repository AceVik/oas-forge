use serde::{Deserialize, Serialize};

/// A product in the catalog.
/// @openapi
#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Product {
    pub id: u64,
    /// Shown in the shop.
    pub display_name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub price_cents: Option<u32>,
}

/// List products
///
/// Returns one page of the catalog.
/// @route GET /products
/// @query-param page: u32 default=1 "Page number"
/// @return 200: Vec<$Product>
pub async fn list_products() {}

/// Get a product
/// @route GET /products/{id: u64 "Product id"}
/// @return 200: $Product
/// @problem 404 "No such product"
pub async fn get_product() {}
