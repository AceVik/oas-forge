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
- Rust type names in type positions (fields, type arguments, aliases,
  signatures, route directives written as Rust types) are resolved by
  **module path**: every scanned file belongs to a crate (`[package] name`
  of the `Cargo.toml` next to its `src`, else the directory name) and a
  module (path below `src`, or the module whose `mod` declaration names
  the file, `#[path]` included, as rustc finds it; inline modules). A name
  resolves to the definition of its own module, then an explicit `use`
  (re-exports followed), then glob imports, then a written
  path (exact, then by suffix), then the one documented type of the name.
  Anything else is ambiguous: reported (`OF3014`), not guessed. A user's
  `$Name` (directives, YAML) stays a schema name.
- Two documented types with the same schema name (no rename) are both
  named by their module path with dots, `crate.module.Name` (valid per the
  component-name pattern), each reported as a **warning** (`OF1004`)
  recommending `@openapi rename`; references follow the qualified names.
  The same type read twice is one type. Templates collide only with
  templates and are qualified the same way (instances `crate.module.Page_User`);
  a name written with type arguments resolves among templates. YAML schemas
  are not qualified; other duplicates are merged and reported.
- The module paths are computed in a pass over the parsed files before
  extraction, so references carry final schema names and projects without
  collisions produce the same documents as before.
- Virtual routes get stable operation ids derived from method and path
  (`get_legacy_proxy`) instead of line numbers.
