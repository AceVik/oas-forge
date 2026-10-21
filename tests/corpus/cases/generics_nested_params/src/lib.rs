//! @openapi
//! openapi: 3.1.0
//! info:
//!   title: Corpus
//!   version: 1.0.0

/// A user.
/// @openapi
pub struct User {
    pub id: u64,
}

/// One value with metadata.
/// @openapi
pub struct Envelope<T> {
    pub data: T,
}

/// A page of wrapped values.
/// @openapi
pub struct Page<T> {
    pub first: Envelope<T>,
    pub rest: Vec<Envelope<T>>,
    pub next: Option<Box<Page<T>>>,
}

/// A tree.
/// @openapi
pub struct Tree<T> {
    pub value: T,
    pub children: Vec<Tree<T>>,
}

/// Lists users.
/// @route GET /users
/// @return 200: $Page<User>
pub fn users() {}

/// The user tree.
/// @route GET /tree
/// @return 200: $Tree<String>
pub fn tree() {}
