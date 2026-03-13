use serde::Serialize;

/// @openapi
/// type: object
/// properties:
///   id:
///     type: integer
///   name:
///     type: string
/// required:
///   - id
///   - name
#[derive(Serialize)]
pub struct User {
    pub id: u64,
    pub name: String,
}

/// @route GET /users
/// List all users with pagination
/// @tag Users
/// @return 200: $Paginated<User> "Paginated list of users"
pub fn list_users() {}

fn main() {}
