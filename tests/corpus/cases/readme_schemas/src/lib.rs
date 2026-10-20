use serde::Serialize;
use std::collections::BTreeSet;
use validator::Validate;

/// How an order is paid.
/// @openapi
#[derive(Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Payment {
    Card { last4: String },
    Invoice { due_days: u16 },
}

/// A postal address.
/// @openapi
#[derive(Serialize)]
pub struct Address {
    pub city: String,
    pub zip: String,
}

/// A customer.
/// @openapi
#[derive(Serialize, Validate)]
pub struct Customer {
    #[validate(email)]
    pub email: String,
    #[validate(length(min = 1, max = 64))]
    pub name: String,
    /// The address fields are part of the customer object.
    #[serde(flatten)]
    pub address: Address,
    /// @example [vip, newsletter]
    #[serde(default)]
    pub tags: BTreeSet<String>,
    pub payment: Option<Payment>,
    #[deprecated]
    pub fax: Option<String>,
}
