//! @openapi
//! openapi: 3.1.0
//! info:
//!   title: Corpus
//!   version: 1.0.0
//!   description: |
//!     Release notes
//!
//!     Version: {{VERSION}}
//! x-releases:
//!   - version: {{VERSION}}
//!
//! @openapi-fragment Paging
//! - name: page
//!   in: query
//!   schema: { type: integer }
//!
//! @openapi-fragment Versioned
//! x-api-version: {{VERSION}}
//! x-price-hint: \$5 per call
//!
//! @openapi-fragment Listing
//! parameters:
//!   @insert Paging
//! @extend Versioned
//!
//! @openapi-fragment Loop
//! x-loop: true
//! @extend Loop
//!
//! @openapi-type Price
//! type: string
//! pattern: '^\$\d+\.\d{2}$'

/// List prices
/// @extend Listing
/// Prices are strings such as `$5.00`.
/// @route GET /prices
/// @query-param below: String pattern=^\$\d+$ "Upper bound"
/// @return 200: Vec<$Price>
pub fn list_prices() {}

/// Loops
/// @route GET /loop
/// @extend Loop
/// @return 204:
pub fn looping() {}
