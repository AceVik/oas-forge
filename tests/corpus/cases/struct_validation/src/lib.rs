//! @openapi
//! openapi: 3.1.0
//! info:
//!   title: Corpus
//!   version: 1.0.0

/// @openapi
pub struct Signup {
    #[validate(email)]
    pub email: String,
    #[validate(url)]
    pub homepage: String,
    #[validate(length(min = 3, max = 32))]
    pub username: String,
    #[validate(range(min = 18, max = 130))]
    pub age: u8,
    #[validate(range(min = -5, max = 5))]
    pub offset: i32,
    #[validate(range(min = 0.5, max = 1.5))]
    pub ratio: f64,
    #[validate(length(min = 1, max = 10))]
    pub tags: Vec<String>,
    #[validate(pattern = "^[a-z]+$")]
    pub slug: String,
    #[validate(regex(path = *SLUG_RE))]
    pub code: String,
    #[validate(contains(pattern = "@"))]
    pub handle: String,
    #[validate(length(equal = 4))]
    pub pin: String,
    #[validate(range(exclusive_min = 0, exclusive_max = 100))]
    pub percent: f32,
    #[validate(required)]
    pub nickname: Option<String>,
}

static SLUG_RE: std::sync::LazyLock<regex::Regex> =
    std::sync::LazyLock::new(|| regex::Regex::new("^[A-Z]{3}$").unwrap());
