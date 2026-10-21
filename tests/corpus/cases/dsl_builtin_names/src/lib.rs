//! @openapi
//! openapi: 3.1.0
//! info:
//!   title: Corpus
//!   version: 1.0.0
//!
//! @asyncapi
//! asyncapi: 3.0.0
//! info:
//!   title: Corpus events
//!   version: 1.0.0

use serde::{Deserialize, Serialize};

/// How long something takes, in minutes.
/// @openapi
#[derive(Serialize, Deserialize)]
pub struct Duration {
    pub minutes: u32,
}

/// Any value the client sends.
/// @openapi
#[derive(Serialize, Deserialize)]
#[serde(rename = "AnyValue")]
pub struct Value {
    pub raw: String,
}

/// Start a job
/// @route POST /jobs
/// @query-param timeout: Duration default=5 "Timeout"
/// @query-param poll: std::time::Duration "Poll interval"
/// @header-param X-Request-Id: Uuid
/// @body Value
/// @return 202: Vec<Duration> "Estimated steps"
/// @return-header 202 X-Took: Duration
pub async fn start_job() {}

/// Estimate a job
/// @route GET /jobs/estimate
pub async fn estimate() -> Json<Option<Duration>> {
    todo!()
}

/// Progress of the jobs.
/// @channel jobs/progress
/// @send Progress: Duration "Time left"
pub fn progress() {}
