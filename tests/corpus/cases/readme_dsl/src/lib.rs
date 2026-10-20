//! @openapi
//! openapi: 3.1.0
//! info:
//!   title: Corpus
//!   version: 1.0.0

/// List Users
///
/// Returns a paginated list of users.
///
/// @route GET /users
/// @tag Users
/// @query-param page: Option<u32> "Page number"
/// @return 200: $Vec<User> "List of users"
async fn list_users() {}

/// @route GET /users/{id: u32 "The unique ID"}
fn get_user(id: u32) {}

/// @route GET /search
/// @query-param filter: Option<String> deprecated example="Alice" "Filter by name"
/// @header-param X-Trace-ID: Uuid required "Tracing ID"
fn search() {}

/// @route POST /users
/// @return 201: $User "Created"
/// @return 200: $Page<User> "List"
/// @return 204: "Deleted"
/// @return 400: "Invalid Input"
fn create_user() {}

/// @route POST /users/create
/// @body $CreateRequest application/json
fn create() {}

/// @route GET /protected
/// @security oidc("read", "write")
/// @security basic()
fn protected() {}

/// @route GET /complex
/// @tag Items
///
/// # You can inject raw parameters alongside DSL
/// parameters:
///   - name: raw_param
///     in: query
///     schema: { type: string }
///
/// # Or override the description
/// externalDocs:
///   url: https://example.com/docs
///   description: More info
///   description: More info
fn complex_handler() {}

/// @openapi
pub struct User {
    pub id: u32,
}

/// @openapi
pub struct CreateRequest {
    pub name: String,
}

/// @openapi<T>
pub struct Page<T> {
    pub data: Vec<T>,
}
