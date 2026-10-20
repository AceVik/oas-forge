//! @openapi
//! openapi: 3.1.0
//! info:
//!   title: Corpus
//!   version: 1.0.0

#[derive(Validate)]
pub struct UserDto {
    #[validate(email)]
    pub email: String,

    #[validate(url)]
    pub website: String,

    #[validate(length(min = 3, max = 20))]
    pub username: String,

    #[validate(range(min = 18, max = 100))]
    pub age: u8,

    #[validate(regex = "path")]
    pub code: String,
}
