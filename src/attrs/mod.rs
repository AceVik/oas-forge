//! Typed models of the attributes and doc directives that shape schemas.

pub(crate) mod directives;
pub mod rename;
pub mod serde;
pub mod validate;

pub use rename::RenameRule;
pub use serde::SerdeAttrs;
pub use validate::Constraints;
