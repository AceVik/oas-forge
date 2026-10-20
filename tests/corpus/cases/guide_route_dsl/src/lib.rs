use serde::{Deserialize, Serialize};

/// @openapi
/// openapi: 3.2.0
/// info: { title: Shop, version: 2.0.0 }
/// components:
///   securitySchemes:
///     oauth:
///       type: oauth2
///       flows:
///         clientCredentials:
///           tokenUrl: https://auth.example.com/token
///           scopes: { read: Read items, write: Change items }
pub fn api() {}

/// An item for sale.
/// @openapi
#[derive(Serialize, Deserialize)]
pub struct Item {
    pub id: u64,
    pub name: String,
}

/// How to page through results.
/// @openapi
#[derive(Deserialize)]
pub struct Paging {
    /// Page number, starting at 1.
    pub page: Option<u32>,
    /// Items per page.
    pub size: Option<u8>,
}

/// List items
/// @route GET /items
/// @tag Items
/// @query-params $Paging
/// @query-param sort: String enum=[name, price] default=name "Sort order"
/// @return 200: Vec<$Item> application/json application/x-ndjson "The items"
/// @return-header 200 X-Total-Count: u64 "Number of items"
/// @security oauth(read)
pub async fn list_items() {}

/// Add an item
/// @route POST /items
/// @tag Items
/// @body $Item
/// @return 201: $Item "Created"
/// @problem 409 "An item with this name exists"
/// @security oauth(write)
pub async fn add_item() {}

/// Watch the stock
/// @route GET /items/{id: u64}/stock
/// @tag Items
/// @return 200: stream<u32> "Stock levels as server-sent events"
/// @security none
pub async fn stock() {}

/// Search items
/// @route QUERY /items
/// @tag Items
/// @body { type: object, properties: { text: { type: string } } }
/// @return 200: Vec<$Item>
pub async fn search() {}

/// An item was sold out.
/// @webhook soldOut
/// @body $Item
/// @return 204:
pub async fn sold_out() {}
