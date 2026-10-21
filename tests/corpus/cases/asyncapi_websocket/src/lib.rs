//! @openapi
//! openapi: 3.1.0
//! info:
//!   title: Gateway
//!   version: 1.0.0
//!
//! @asyncapi
//! asyncapi: 3.0.0
//! info:
//!   title: Gateway events
//!   version: 1.0.0
//! defaultContentType: application/json
//! servers:
//!   public:
//!     host: api.example.com
//!     pathname: /ws
//!     protocol: wss

use serde::{Deserialize, Serialize};

/// Work that waits for a worker.
/// @openapi
#[derive(Serialize)]
pub struct Pending {
    pub jobs: Vec<Job>,
}

/// A job.
/// @openapi
#[derive(Serialize)]
pub struct Job {
    pub id: u64,
    pub name: String,
}

/// Which events a client wants.
/// @openapi
#[derive(Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Topic {
    Jobs,
    Alerts,
}

/// A subscription request of the client.
/// @openapi
#[derive(Deserialize)]
pub struct Subscribe {
    pub topics: Vec<Topic>,
}

/// Live updates
///
/// One WebSocket connection per client; the client subscribes to topics and
/// receives updates as they happen.
/// @channel live live/{tenant}
/// @ws-query token: String "Session token"
/// @ws-query topic: Option<$Topic> example=jobs "The first topic"
/// @ws-header X-Client-Version: String example="2.1.0" "Version of the client"
/// @send Pending: application/json "Work waiting for a worker"
/// @send Snapshot: Vec<$Job> application/x-ndjson
/// @receive Subscribe: $Subscribe
/// @tag Live
/// servers:
///   - $ref: '#/servers/public'
/// parameters:
///   tenant:
///     description: Tenant slug
/// bindings:
///   ws:
///     method: GET
/// x-rate-limit: 10
/// @openapi message Pending
/// title: Pending work
/// payload: { $ref: $Pending }
/// @openapi operation
/// x-requires-session: true
/// @openapi document
/// components:
///   messages:
///     Ping:
///       payload: { type: string, const: ping }
pub async fn live_updates() {}
