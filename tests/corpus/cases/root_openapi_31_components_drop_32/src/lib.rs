//! @openapi
//! openapi: 3.1.0
//! info:
//!   title: Modern components
//!   version: 1.0.0

/// @openapi
pub struct Event {
    pub kind: String,
}

/// @openapi
/// components:
///   pathItems:
///     Search:
///       query:
///         responses:
///           '200': { description: Found }
///       get:
///         parameters:
///           - name: filter
///             in: querystring
///             content:
///               application/json:
///                 schema: { type: object }
///           - name: page
///             in: query
///             schema: { type: integer }
///         responses:
///           '200': { $ref: '#/components/responses/Events' }
///   responses:
///     Events:
///       description: Server-sent events
///       content:
///         text/event-stream:
///           itemSchema: $Event
///   requestBodies:
///     Upload:
///       content:
///         application/jsonl:
///           itemSchema: $Event
///   callbacks:
///     Notify:
///       '{$request.body#/callback}':
///         additionalOperations:
///           NOTIFY:
///             responses:
///               '200': { description: Notified }
///         post:
///           requestBody: { $ref: '#/components/requestBodies/Upload' }
///           responses:
///             '200': { description: Received }
fn components() {}

/// Subscribe to events
/// @openapi
/// paths:
///   /subscriptions:
///     post:
///       operationId: subscribe
///       responses:
///         '201': { description: Subscribed }
///       callbacks:
///         onEvent:
///           '{$request.body#/callback}':
///             post:
///               requestBody:
///                 content:
///                   application/jsonl:
///                     itemSchema: $Event
///               responses:
///                 '200': { description: Received }
///         shared: { $ref: '#/components/callbacks/Notify' }
pub fn subscribe() {}
