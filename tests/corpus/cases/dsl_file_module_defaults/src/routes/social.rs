//! @route-defaults
//! @prefix /social
//! @tag Social

/// List devices
/// @route GET /devices
/// @return 200: Vec<String>
pub async fn devices() {}

/// Public status
/// @route GET /status
/// @security none
/// @return 204:
pub async fn status() {}
