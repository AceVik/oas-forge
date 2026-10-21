//! @openapi
//! openapi: 3.2.0
//! info:
//!   title: Files
//!   version: 1.0.0

/// An attachment.
/// @openapi-type Attachment
/// type: object
/// properties:
///   thumbnail:
///     type: string
///     contentEncoding: base64
///     contentMediaType: image/png
///   checksum:
///     type: [string, "null"]
///     contentEncoding: base64
///   notes:
///     type: string
///     contentMediaType: text/markdown
fn attachment() {}

/// Upload a file
/// @openapi
/// paths:
///   /files:
///     post:
///       operationId: upload
///       requestBody:
///         content:
///           multipart/form-data:
///             schema:
///               type: object
///               properties:
///                 file:
///                   type: string
///                   contentMediaType: application/octet-stream
///                 name:
///                   type: string
///               required: [file]
///       responses:
///         '201':
///           description: Stored
///           content:
///             application/json:
///               schema: { $ref: $Attachment }
///   /files/{id}/raw:
///     get:
///       operationId: download
///       parameters:
///         - { name: id, in: path, required: true, schema: { type: string } }
///       responses:
///         '200':
///           description: The file
///           content:
///             application/octet-stream:
///               schema:
///                 type: string
///                 contentMediaType: application/octet-stream
pub fn files() {}
