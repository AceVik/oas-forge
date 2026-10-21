//! @openapi
//! openapi: 3.1.0
//! info:
//!   title: Corpus
//!   version: 1.0.0

pub mod models {
    use serde::{Deserialize, Serialize};

    /// A user.
    /// @openapi
    #[derive(Serialize, Deserialize)]
    pub struct User {
        pub id: u64,
    }

    /// An error.
    /// @openapi
    #[derive(Serialize)]
    pub struct ApiError {
        pub message: String,
    }

    /// How to page.
    /// @openapi
    #[derive(Deserialize)]
    pub struct Paging {
        /// Page number.
        pub page: Option<u32>,
        /// Page size.
        pub size: u8,
    }

    /// Sort order.
    /// @openapi
    #[derive(Deserialize)]
    pub struct Sorting {
        pub sort: String,
    }

    /// Filters for listing users.
    /// @openapi
    #[derive(Deserialize)]
    #[serde(rename_all = "camelCase")]
    pub struct UserFilter {
        /// Text in the name.
        pub name_contains: Option<String>,
        #[serde(flatten)]
        pub paging: Paging,
        #[serde(flatten)]
        pub sorting: Option<Sorting>,
    }

    /// The path of a user.
    /// @openapi
    #[derive(Deserialize)]
    pub struct UserPath {
        pub org: String,
        pub id: u64,
    }
}

/// List users
/// @route GET /users
/// @query-param nameContains: String min=3 "At least three characters"
/// @return default: $ApiError "Unexpected error"
pub async fn list_users(
    Query(filter): axum::extract::Query<models::UserFilter>,
) -> Result<axum::Json<Vec<models::User>>, models::ApiError> {
    todo!()
}

/// Get a user
/// @route GET /orgs/{org}/users/{id}
pub async fn get_user(Path(path): Path<crate::models::UserPath>) -> Json<models::User> {
    todo!()
}

/// Update a user
/// @route PATCH /users/{id: u64}
/// @return 200: $User
pub async fn update_user(body: Option<Json<models::User>>) {}
