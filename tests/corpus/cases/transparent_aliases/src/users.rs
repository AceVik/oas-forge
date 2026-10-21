pub type Owner = User;

/// A player.
/// @openapi
#[derive(serde::Serialize)]
pub struct User {
    pub name: String,
}

/// Uses `Code`, which two other files define differently.
/// @openapi
#[derive(serde::Serialize)]
pub struct Invite {
    pub code: Code,
}
