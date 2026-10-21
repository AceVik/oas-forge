//! @openapi
//! openapi: 3.1.0
//! info:
//!   title: Corpus
//!   version: 1.0.0

/// tags: [Outer]
pub mod outer {
    /// tags: [Inner]
    pub mod inner {
        /// @openapi
        /// paths:
        ///   /deep:
        ///     get:
        ///       responses:
        ///         '200': { description: OK }
        pub fn deep() {}
    }

    /// @openapi
    /// paths:
    ///   /shallow:
    ///     get:
    ///       responses:
    ///         '200': { description: OK }
    pub fn shallow() {}
}
