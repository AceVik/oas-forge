# Schemas

Structs, enums and type aliases become entries of `components.schemas`. The
schema describes the JSON that serde writes for the type, so field names,
optional fields, flattening and enum tagging follow your serde attributes.
The full list of supported attributes and types is in
[Compatibility](compatibility.md).

## Exporting a type

A type is exported when a line of its doc comment starts with `@openapi`:

```rust,ignore
/// A user account.
/// @openapi
pub struct User {
    pub id: u64,
    /// The name shown to others.
    pub name: String,
    pub email: Option<String>,
}
```

```yaml
User:
  type: object
  properties:
    id: { type: integer, format: int64, minimum: 0 }
    name: { type: string, description: The name shown to others. }
    email: { type: [string, "null"] }
  required: [id, name]
  description: A user account.
```

- Doc lines before the first `@openapi` line are the description. Lines are
  joined with spaces; blank lines separate paragraphs. Fenced code blocks,
  list items and the line after a heading keep their line breaks.
  `/** … */` comments work too, also with a leading `*` on every line.
- Fields and variants need no `@openapi` line; their doc comments become
  their descriptions.
- A sentence that merely mentions `@openapi` does not export anything.
- Generic types such as `struct Page<T>` are templates. They are exported
  once per use, see [generics](#generics).

## Overrides

YAML after `@openapi`, on the same line or on the following doc lines, is
merged into the reflected schema. Values written there win:

```rust,ignore
/// @openapi
/// example: { id: 1, name: Ada }
/// x-internal: false
pub struct User {
    /// @openapi format: int64
    pub id: u64,
    /// @openapi
    /// minLength: 1
    /// examples: [Ada]
    pub name: String,
}
```

The same works on enum variants; it describes the variant as serialized. An
override that defines the structure itself (`type`, `properties`, `oneOf`,
…) replaces the reflected reference of a newtype, so `struct Id(Uuid)` with
`/// @openapi type: string` is exactly `{type: string}`; on a field of type
`Option<Id>` the result stays nullable.

Invalid YAML, and text that is not a YAML mapping (`/// @openapi The id`),
is reported (`OF1001`) and ignored.

## Directives

| Directive | On | Effect |
| :--- | :--- | :--- |
| `@openapi` | type | Exports the type; YAML after it is merged |
| `@openapi rename "Name"` | type, field, variant | Uses another name; wins over `#[serde(rename)]` |
| `@openapi rename-all rule` | type | Renames all fields (structs) or variants (enums); rules as in serde: `camelCase`, `snake_case`, `kebab-case`, `PascalCase`, `SCREAMING_SNAKE_CASE`, `SCREAMING-KEBAB-CASE`, `lowercase`, `UPPERCASE` |
| `@openapi skip` | type, field, variant | Leaves it out of the document |
| `@example value` | type, field, variant | Adds a YAML value (or plain text) to `examples`; repeatable |
| `@openapi<T, U>` | generic type | Declares a [blueprint](#explicit-blueprints) with YAML that uses `$T` |
| `#[deprecated]` | type, field, variant | Sets `deprecated: true` |

Precedence for names: `@openapi rename`, then `#[serde(rename)]`, then
`@openapi rename-all` or `#[serde(rename_all)]`, then the Rust name. An
explicit name is used as written, even when it equals the Rust name. A
directive with an invalid argument (an unknown `rename-all` rule, text after
`skip` that is not a `# comment`) is reported (`OF1008`).
References follow renames: a field of type `Profile` points to
`#/components/schemas/UserProfile` when `Profile` is renamed.

<!-- corpus: guide_schemas -->
```rust
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
```

## Required and optional fields

A field is required unless one of these applies:

- its type is `Option<T>`; the schema then also allows `null`
  (`type: [T, "null"]` in 3.1, `nullable: true` in 3.0),
- it has `#[serde(default)]`, or the container has it,
- it has `#[serde(skip_serializing_if = "…")]`.

A field with a custom deserializer (`#[serde(with = "…")]` or
`deserialize_with`) is required even if it is an `Option`: serde reports a
missing field instead of using `None`, unless the field has `default`.

`#[validate(required)]` or `#[garde(required)]` on an `Option` makes it
required and not nullable. `#[serde(skip_serializing)]` adds `writeOnly`,
`#[serde(skip_deserializing)]` adds `readOnly`.

## Enums

Enums follow serde's representation:

| serde | Schema |
| :--- | :--- |
| default (externally tagged) | unit variants as a string `enum`; data variants as single-key objects; together a `oneOf` |
| `#[derive(Serialize_repr)]` ([serde_repr](https://docs.rs/serde_repr)) | an integer `enum` of the discriminants |
| `#[serde(tag = "type")]` | one component per variant (`{Enum}{Variant}`) with the tag property, combined with `oneOf` and a `discriminator` |
| `#[serde(tag = "t", content = "c")]` | like internally tagged, the data under the content key |
| `#[serde(untagged)]` | `anyOf` of the variants' content |

A string `enum` cannot describe single values. When a unit variant has an
`@openapi` override, an `@example` or `#[deprecated]`, the enum also gets a
`oneOf` with one `const` per unit variant that carries it, together with the
variants' doc comments. A `serde_repr` discriminant that is not an integer
literal is reported (`OF1009`).

Variants with `#[serde(untagged)]` in a tagged enum become additional
`anyOf` alternatives. A variant shape serde cannot put next to an internal
tag (a tuple or a primitive newtype) is reported (`OF1005`).

## Generics

A generic struct or enum is a template. Every use with concrete arguments
produces a schema named after the arguments:

| Rust | Schema name |
| :--- | :--- |
| `Page<User>` | `Page_User` |
| `Page<u64>` | `Page_u64` |
| `Page<Vec<User>>` | `Page_Vec_User` |
| `Page<Option<User>>` | `Page_Option_User` |
| `Page<Page<User>>` | `Page_Page_User` |
| `Either<String, User>` | `Either_String_User` |

Uses count everywhere: field types, `@return 200: $Page<User>`, `@body`,
inferred handler signatures and YAML values `$ref: $Page<User>`. Arguments
may be paths (`Page<models::User>`). A generic type that is never used is not
exported. Instantiation stops at a nesting depth of 16 (`OF3006`).

A generic type alias (`type Paged<T> = Page<T>;`) is a template too. The
variants of a generic tagged enum that use a type parameter are instantiated
with it: `Event<User>` has the variants `EventCreated_User`, … and keeps the
variants without parameters (`EventDeleted`) shared.

### Explicit blueprints

`@openapi<T>` declares the template with YAML; `$T` stands for the argument.
This also works for types without fields:

```rust,ignore
/// A response envelope.
/// @openapi<T>
/// type: object
/// properties:
///   data: { $ref: $T }
///   meta: { type: object }
pub struct Envelope<T>(T);
```

A blueprint used with the wrong number of arguments is reported (`OF3005`),
an unknown one (`$Missing<User>`) too (`OF3004`).

## Types from other crates

A type that is not exported in the scanned sources is a reference to a
schema of the same name (`$ref: '#/components/schemas/Money'`), which is
reported when nothing defines it (`OF3007`). Three ways to define it:

1. A named YAML schema anywhere in the sources:

   ```rust,ignore
   //! @openapi-type Money
   //! type: string
   //! pattern: '^\d+\.\d{2}$'
   ```

2. A fixed mapping for every use of the type, in the build script or the
   configuration (`[types]`), by name or path:

   ```rust,ignore
   Generator::new().map_type("Money", serde_json::json!({ "type": "string" }))
   ```

3. A YAML file with `components.schemas` passed as `include`.

Many common crates (`chrono`, `time`, `jiff`, `uuid`, `url`,
`rust_decimal`, `bytes`, `indexmap`, …) are built in; see the
[type table](compatibility.md#types).

Your own types may have the names of built-in ones. A type written by its
bare name (`Duration`, `Value`, `Uuid`) refers to the documented schema of
that name when the sources define one (an `@openapi` type or an
`@openapi-type` schema, also in another file); otherwise it is the built-in
type. A path (`std::time::Duration`, `serde_json::Value`) and a custom
mapping always mean the built-in or mapped type.

## References in YAML

In any YAML block, a value `$Name` is a reference to the schema `Name`:
`$ref: $User` becomes `$ref: '#/components/schemas/User'`, and
`$Page<User>` instantiates the template. Only whole values are replaced;
`$User` inside a sentence stays text. Write `\$` for a literal dollar sign at
the start of a value.

## Validation constraints

`#[validate(...)]` and `#[garde(...)]` add JSON Schema keywords; for example
`length(min = 1, max = 64)` becomes `minLength`/`maxLength` on strings,
`minItems`/`maxItems` on lists and `minProperties`/`maxProperties` on maps.
A `regex(path = RE)` refers to a constant; when a
`static RE: LazyLock<Regex> = LazyLock::new(|| Regex::new("…").unwrap())`
(or `Lazy`, `lazy_static!`, `OnceLock`) with a literal pattern is in the
scanned sources, its pattern is used, otherwise the finding `OF1007` reports
it. Values that are not literals (`length(max = MAX)`) are reported
(`OF1006`) and left out. See the
[validator and garde tables](compatibility.md#validator).
