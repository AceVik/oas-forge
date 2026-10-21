//! @asyncapi
//! asyncapi: 3.0.0
//! info:
//!   title: Order events
//!   version: 1.0.0
//! defaultContentType: application/json
//! servers:
//!   production:
//!     host: kafka.example.com:9092
//!     protocol: kafka

use serde::Serialize;

/// An order.
/// @openapi
#[derive(Serialize)]
pub struct Order {
    pub id: u64,
    pub total_cents: u64,
}

/// What changed.
/// @openapi
#[derive(Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum OrderEvent {
    Placed { order: Order },
    Cancelled { id: u64, reason: Option<String> },
}

/// Order lifecycle
///
/// Every change of an order, keyed by order id.
/// @channel orders order/{id}/events
/// @send OrderChanged: $OrderEvent "A change of the order"
/// @tag Orders
pub fn publish_order_event() {}

/// Payment confirmations from the payment provider.
/// @channel payments/confirmed
/// @receive PaymentConfirmed: { type: object, properties: { orderId: { type: integer } }, required: [orderId] }
pub fn on_payment() {}
