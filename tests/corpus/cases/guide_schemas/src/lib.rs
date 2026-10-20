use serde::Serialize;
use uuid::Uuid;

/// @openapi
/// openapi: 3.1.0
/// info: { title: Directory, version: 1.0.0 }
pub fn api() {}

/// A team member.
/// @openapi rename "Member"
/// @example { id: 7, displayName: Ada, role: admin }
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TeamMember {
    pub id: u32,
    /// @openapi
    /// minLength: 1
    pub display_name: String,
    pub role: Role,
    /// Internal bookkeeping, never shown.
    /// @openapi skip
    pub audit_token: String,
    #[deprecated(note = "use display_name")]
    pub nick: Option<String>,
}

/// What a member may do.
/// @openapi
#[derive(Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Role {
    Admin,
    /// @openapi rename "member"
    Regular,
    #[serde(skip)]
    System,
}

/// An opaque identifier.
/// @openapi
/// type: string
/// format: uuid
#[derive(Serialize)]
#[serde(transparent)]
pub struct MemberId(Uuid);

/// One page of results.
/// @openapi
#[derive(Serialize)]
pub struct Page<T> {
    pub items: Vec<T>,
    pub next: Option<String>,
}

/// List members
/// @route GET /members
/// @return 200: $Page<Member>
pub async fn members() {}
