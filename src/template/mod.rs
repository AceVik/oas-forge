//! Templates and references: fragments (`@insert`, `@extend`), blueprints
//! (`@openapi<T>` and `$Page<User>`), smart references (`$User`) and
//! `{{…}}` variables.
//!
//! The stages run in this order: [`lexical::expand`] on text, then on parsed
//! values [`extend::apply`], [`generics::Instantiator`], [`refs::resolve`] and
//! finally [`refs::unescape`].

pub(crate) mod aliases;
pub mod extend;
pub mod generics;
pub mod lexical;
pub mod refs;
pub mod registry;

pub use lexical::Variables;
pub use registry::{Blueprint, Fragment, Registry};
