//! @openapi
//! openapi: 3.1.0
//! info:
//!   title: Corpus
//!   version: 1.0.0

/// @openapi
pub struct User { pub id: u64, pub name: String }

/// @openapi
pub struct NewUser { pub name: String }

/// @openapi
pub struct ListParams {
    /// Maximum number of users.
    pub limit: Option<u32>,
    pub cursor: Option<String>,
}

/// @openapi
pub struct OrgUser { pub org: String, pub id: u64 }

/// Get a user.
/// @route GET /users/{id}
pub async fn get_user(State(db): State<Db>, Path(id): Path<u64>) -> Result<Json<User>, ApiError> { todo!() }

/// @route GET /users
pub async fn list_users(Query(params): Query<ListParams>) -> Json<Vec<User>> { todo!() }

/// @route POST /users
/// @return 201: $User "Created"
pub async fn create_user(Json(body): Json<NewUser>) -> (StatusCode, Json<User>) { todo!() }

/// @route GET /orgs/{org}/users/{id}
pub async fn org_user(Path((org, id)): Path<(String, u64)>) -> impl IntoResponse { todo!() }

/// @route GET /v2/orgs/{org}/users/{id}
pub async fn org_user_struct(Path(params): Path<OrgUser>) -> HttpResponse { todo!() }

/// @route POST /login
pub async fn login(web::Form(form): web::Form<NewUser>) -> actix_web::Result<web::Json<User>> { todo!() }

/// @route POST /upload
pub async fn upload(mut multipart: Multipart) -> StatusCode { todo!() }

/// @route GET /explicit/{id: String "Explicit wins"}
/// @no-infer
pub async fn explicit(Path(id): Path<u64>) -> Json<User> { todo!() }
