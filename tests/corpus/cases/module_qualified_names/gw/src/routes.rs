use baylee_cardtext::Printing as TextPrinting;
use crate::Pool;

/// Get the pool
/// @route GET /pool
/// @return 200: Pool
/// @return 201: TextPrinting
/// @return 202: $Printing "By component name"
pub async fn pool() {}

/// The catalog's status changed
/// @channel pool/status
/// @send StatusChanged: baylee_catalog::Status
pub fn status_changes() {}
