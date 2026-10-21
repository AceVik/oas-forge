//! @asyncapi
//! asyncapi: 3.0.0
//! info:
//!   title: Gateway events
//!   version: 1.0.0
//! servers:
//!   gateway:
//!     host: gateway.example.com
//!     protocol: wss

/// Gateway socket
///
/// One connection per client; frames are `baylee.v1.Envelope` messages in
/// both directions.
/// @channel socket /ws
/// @ws-query token: String "Session token from the login"
/// @ws-query resume: Option<u64> "Sequence number to resume after"
/// @ws-header Sec-WebSocket-Protocol: String example="baylee.v1"
/// @send ServerFrame: proto(baylee.v1.Envelope) "A frame from the gateway"
/// @receive ClientFrame: proto(baylee.v1.Envelope) "A frame from the client"
/// @tag Gateway
/// servers:
///   - $ref: '#/servers/gateway'
/// bindings:
///   ws:
///     method: GET
/// @openapi message ServerFrame
/// title: Server frame
pub async fn socket() {}
