//! @openapi
//! openapi: 3.1.0
//! info:
//!   title: Corpus
//!   version: 1.0.0

/// A page of results.
/// @openapi
pub struct Page<T> {
    pub items: Vec<T>,
    pub total: u64,
}

/// Either of two values.
/// @openapi
pub enum Either<L, R> {
    Left(L),
    Right(R),
}

/// Never used, never emitted.
/// @openapi
pub struct Unused<T> {
    pub value: T,
}

pub mod models {
    /// A user.
    /// @openapi
    pub struct User {
        pub name: String,
    }
}

/// Every kind of argument.
/// @openapi
pub struct Holder {
    pub numbers: Page<u64>,
    pub lists: Page<Vec<models::User>>,
    pub optional: Page<Option<models::User>>,
    pub nested: Page<Page<models::User>>,
    pub either: Either<String, models::User>,
}

/// Lists users.
/// @route GET /users
/// @return 200: $Page<User>
pub fn users() {}

/// Lists users (inferred).
/// @route GET /inferred
pub async fn inferred() -> axum::Json<Page<models::User>> {
    todo!()
}
