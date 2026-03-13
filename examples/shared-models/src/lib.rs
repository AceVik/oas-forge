use serde::Serialize;

/// @openapi<T>
/// type: object
/// properties:
///   items:
///     type: array
///     items:
///       $ref: $T
///   total:
///     type: integer
///   page:
///     type: integer
/// required:
///   - items
///   - total
///   - page
#[derive(Serialize)]
pub struct Paginated<T> {
    pub items: Vec<T>,
    pub total: usize,
    pub page: usize,
}
