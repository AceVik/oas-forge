//! @openapi
//! openapi: 3.1.0
//! info:
//!   title: Corpus
//!   version: 1.0.0

/// @openapi
pub enum Shape {
    Circle { radius: f64 },
    Square(f64),
    Point,
}
