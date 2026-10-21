# AsyncAPI

Events your service publishes or consumes are documented in a separate
[AsyncAPI 3](https://www.asyncapi.com/docs/reference/specification/v3.0.0)
document. It is written with `output_asyncapi(path)` (CLI
`--output-asyncapi`) and shares the component schemas of the OpenAPI
document.

<!-- corpus: guide_asyncapi -->
```rust
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
```

## The root

A `@asyncapi` block (or a YAML file with an `asyncapi:` root) with `asyncapi`
and `info` is the root of the AsyncAPI document. Other `@asyncapi` blocks
add to it, like `@openapi` blocks do for OpenAPI. These blocks never end up in
the OpenAPI document. Without a root, the AsyncAPI output fails with an error;
the OpenAPI outputs are not affected.

## Channels

| Directive | Syntax | Result |
| :--- | :--- | :--- |
| `@channel` | `[id] address` | `channels.id` with `address`; placeholders such as `{id}` become channel `parameters` |
| `@send` | `Message: Type ["description"]` | the message in `components.messages`, the channel's `messages` and a `send` operation |
| `@receive` | `Message: Type ["description"]` | the same with a `receive` operation |
| `@tag` | `Name[, Name…]` | the operations' `tags` |
| `@summary`, `@description` | `text` | the operations' `summary` and `description` |
| `@internal` | | the plain lines after it are left out |

- Without an id, the id is derived from the address: `user/{id}/signedup`
  becomes `user_id_signedup`.
- The first plain line of the block is the summary, the following ones the
  description. `@summary`, `@description` and `@internal` work as in
  [routes](route-dsl.md#the-block): with `@summary`, all plain lines are
  the description; `@description` (continued on the following plain lines)
  replaces them; the plain lines after `@internal` are left out.
- The operation id is the function name; with both `@send` and `@receive` in
  one block it gets the suffix `_send` or `_receive`. A channel in a `//!`
  comment has no function; its operations are named `send_{channel}` and
  `receive_{channel}`.
- Message types use the [DSL type syntax](route-dsl.md#types): `$Order`,
  `Vec<$Item>`, `u64` or inline YAML.

## Shared schemas

Every component schema a message refers to, directly or through other
schemas, is copied from the OpenAPI document into the AsyncAPI
`components.schemas`, unless the AsyncAPI document defines a schema of that
name itself. Schemas are written in the AsyncAPI schema dialect: tuples
become `items` lists and the discriminator a property name. What has no
equivalent is reported (`OF6001`).

The AsyncAPI document is checked by the same lints as the OpenAPI document,
for example for references that lead nowhere (`OF5001`).
