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
| `@send` | `Message: [Type] [media…] ["description"]` | the message in `components.messages`, the channel's `messages` and a `send` operation |
| `@receive` | `Message: [Type] [media…] ["description"]` | the same with a `receive` operation |
| `@ws-query` | `name: [Type] [flags] ["description"]` | a query parameter of the WebSocket handshake, see [below](#websocket-channels) |
| `@ws-header` | `name: [Type] [flags] ["description"]` | a header of the WebSocket handshake |
| `@tag` | `Name[, Name…]` | the operations' `tags` |
| `@summary`, `@description` | `text` | the operations' `summary` and `description` |
| `@scope` | `name[, name…]` | the documents the channel belongs to, see [scopes](route-dsl.md#scopes) |
| `@openapi` | `[channel \| message Name \| operation \| document]` | starts [embedded YAML](#embedded-yaml) |
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
- Media types after the type set the message's `contentType` (the first
  one): `@send Snapshot: Vec<$Job> application/x-ndjson`. Without one, the
  document's `defaultContentType` applies. A message may have a media
  type and no type, when YAML describes the payload.
- A channel without its own `@scope` takes the `@scope` of the
  innermost enclosing module's `@route-defaults`; the other defaults
  apply to routes only.

## WebSocket channels

`@ws-query` and `@ws-header` describe the HTTP handshake that opens a
WebSocket channel, with the syntax and flags of
[`@query-param`](route-dsl.md#parameters). They become the channel's
[WebSocket binding](https://github.com/asyncapi/bindings/tree/master/websockets):
`bindings.ws.query` and `bindings.ws.headers` are object schemas with one
property per parameter (with its description, `deprecated` and `example`
as `examples`) and `required` for the required ones, plus
`bindingVersion: 0.1.0`. Flags that only parameters have (`style`,
`explode`, `allow-reserved`, `allow-empty`) are reported and ignored.
The method and anything else go into the YAML:

```rust,ignore
/// Live updates
/// @channel live
/// @ws-query token: String "Session token"
/// @ws-header X-Client-Version: String example="2.1.0"
/// @send Update: $Update
/// bindings:
///   ws:
///     method: GET
pub async fn live() {}
```

## Embedded YAML

A line of a channel block that starts with a field of the Channel Object
(`title:`, `summary:`, `description:`, `servers:`, `parameters:`,
`messages:`, `tags:`, `externalDocs:`, `bindings:`) or an extension
(`x-…:`) starts YAML that is merged into the channel; the next directive
ends it. `@openapi` chooses the target:

| Block | Target |
| :--- | :--- |
| `@openapi` or `@openapi channel` | the Channel Object |
| `@openapi message Name` | `components.messages.Name` (also a message of another block) |
| `@openapi operation` | every operation of the block |
| `@openapi document` | the AsyncAPI document |

The YAML wins over what the directives produced; a value it changes is
reported as a note (`OF4001`) at its line. YAML that is invalid or not a
mapping is reported (`OF2005`) and ignored. Smart references resolve as
everywhere else (`payload: { $ref: $Pending }`), and the schemas they name
are shared into the AsyncAPI document.

In a `//!` file comment, an `@openapi` line starts a new block, so
`@openapi message …` sections need a `///` comment; channel fields
(`bindings:`) work in both.

## Shared schemas

Every component schema a message refers to, directly or through other
schemas, is copied from the OpenAPI document into the AsyncAPI
`components.schemas`, unless the AsyncAPI document defines a schema of that
name itself. Schemas are written in the AsyncAPI schema dialect: tuples
become `items` lists and the discriminator a property name. What has no
equivalent is reported (`OF6001`).

The AsyncAPI document is checked by the same lints as the OpenAPI document,
for example for references that lead nowhere (`OF5001`).
