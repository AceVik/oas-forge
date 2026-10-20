//! @openapi
//! openapi: 3.1.0
//! info:
//!   title: Corpus
//!   version: 1.0.0

//! @openapi-type PhoneNumber
//! type: string
//! format: tel
//! pattern: "^\\+?[1-9]\\d{1,14}$"
//! description: E.164 formatted phone number.
//! example: "+491701234567"

/// E-Mail address.
/// @openapi
/// example: "johnd@mail.com"
/// format: email
pub type Email = String;

/// Description of user roles within the system.
/// @openapi
pub enum Role {
  Admin,
  Moderator,
  User,
}

/// Description of a User entity.
/// @openapi
pub struct User {
  pub id: u32,

  /// The username of the user.
  /// @openapi example: "johndoe"
  pub username: Option<String>,

  /// The email address of the user.
  /// @openapi example: "johndoe@mail.com"
  pub email: Email,

  pub phone: PhoneNumber,

  /// The role assigned to the user.
  /// @openapi default: "User"
  pub role: Role,

  pub is_active: bool,

  /// The timestamp when the user was created.
  /// @openapi example: "2024-01-01T12:00:00"
  pub created_at: chrono::NaiveDateTime,
}
