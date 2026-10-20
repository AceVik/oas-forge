# 0004 – Schema naming, aliases and collisions

## Decision
- A schema is named after its **serialized name**: `#[serde(rename)]` or
  `@openapi rename` win over the Rust identifier.
- The registry keeps an **alias table** from Rust identifiers to schema names,
  so a field of type `Profile` references `UserProfile` when the struct is
  renamed.
- Generic instantiations are named `Base_Arg1_Arg2` (`Page<User>` →
  `Page_User`); nested arguments are flattened (`Page_Wrapper_User`).
- Tagged enum variants are named `{Enum}{Variant}`.
- Two different definitions with the same name are an **error-level
  diagnostic**; the second one is qualified with its module path
  (`admin.User`) so neither is lost silently.
- Virtual routes get stable operation ids derived from method and path
  (`get_legacy_proxy`) instead of line numbers.
