//! @openapi
//! openapi: 3.1.0
//! info:
//!   title: Corpus
//!   version: 1.0.0
//! components:
//!   securitySchemes:
//!     oauth: { type: oauth2, flows: { implicit: { authorizationUrl: "https://auth/x", scopes: { read: r, write: w } } } }
//!     api_key: { type: apiKey, in: header, name: X-Key }

/// @openapi
pub struct Pagination {
    /// Page number.
    pub page: Option<u32>,
    /// Page size.
    #[validate(range(min = 1, max = 100))]
    pub size: u32,
}

/// @openapi
pub struct Item {
    pub id: u64,
}

/// @route GET /items
/// @summary List items
/// @description
/// Lists the items of the shop.
///
/// responses: are described below (this is prose, not YAML).
/// @tag Items
/// @query-params $Pagination
/// @param query sort: String enum=[name, price] default=name "Sort order"
/// @param header X-Request-Id: Uuid required "Correlation id"
/// @param cookie session: String deprecated
/// @param query q: String min=2 max=50 pattern="^[a-z ]+$" example="red shoes" style=form explode=false
/// @return 200: Vec<$Item> application/json application/x-ndjson "The items"
/// @return-header 200 X-Total-Count: u64 "Total number of items"
/// @problem 400 "Invalid query"
/// @problem 500
/// @security oauth(read) + api_key
/// @security none
/// @external-docs https://docs.example.com/items "Item guide"
/// @server https://eu.example.com "EU"
/// @deprecated
/// @operation-id listShopItems
pub fn list_items() {}

/// @route POST /items
/// @body optional $Item application/json application/xml "The new item"
/// @body $Item multipart/form-data
/// @return 2XX: $Item
/// @return default: unit "Unexpected error"
pub fn create_item() {}
