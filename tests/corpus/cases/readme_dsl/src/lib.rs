/// @openapi
/// openapi: 3.1.0
/// info: { title: Orders, version: 1.0.0 }
/// components:
///   securitySchemes:
///     bearer: { type: http, scheme: bearer }
pub fn api() {}

/// An order.
/// @openapi
pub struct Order {
    pub id: u64,
    pub items: Vec<String>,
}

/// Create an order
/// @route POST /orders
/// @tag Orders
/// @security bearer
/// @header-param Idempotency-Key: Uuid required "Makes retries safe"
/// @body $Order "The order to place"
/// @return 201: $Order "Created"
/// @return-header 201 Location: String "URL of the new order"
/// @problem 422 "The order is invalid"
/// callbacks:
///   shipped:
///     '{$request.body#/callbackUrl}':
///       post:
///         responses: { '204': { description: Received } }
pub async fn create_order() {}
