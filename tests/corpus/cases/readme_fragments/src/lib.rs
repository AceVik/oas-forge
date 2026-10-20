//! ---------------------------------------------------------------------------
//! A. Fragment for a List (e.g., Parameters)
//! ---------------------------------------------------------------------------
//! @openapi-fragment PaginationParams
//! - name: page
//!   in: query
//!   description: Page number
//!   schema: { type: integer, default: 1 }
//! - name: limit
//!   in: query
//!   schema: { type: integer, default: 10 }

//! ---------------------------------------------------------------------------
//! B. Fragment for an Object (e.g., Responses or Security)
//! ---------------------------------------------------------------------------
//! @openapi-fragment Secured(role)
//! security:
//!   - oidcAuth: [ {{role}} ]
//! responses:
//!   '401':
//!     description: Unauthorized
//!   '403':
//!     description: Forbidden - Requires {{role}} role
