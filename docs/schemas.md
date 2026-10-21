# Schemas

Structs, enums and type aliases with `@openapi` become entries of
`components.schemas`; type aliases without it are
[transparent](#type-aliases). The
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

- The prose of the doc comment is the description. Lines are joined with
  spaces; blank lines separate paragraphs. Fenced code blocks, list items
  and the line after a heading keep their line breaks. `/** … */` comments
  work too, also with a leading `*` on every line. For prose meant for
  developers, see [descriptions](#descriptions).
- Fields and variants need no `@openapi` line; their doc comments become
  their descriptions.
- A sentence that merely mentions `@openapi` does not export anything.
- Generic types such as `struct Page<T>` are templates. They are exported
  once per use, see [generics](#generics).

## Overrides

YAML after `@openapi`, on the same line or on the following doc lines up
to the next line that starts with a directive (`@description`, `@internal`,
`@example`, …; `@insert` and `@extend` lines belong to the YAML) or to a
paragraph of prose (a line after a blank line that cannot start YAML), is
merged into the reflected schema. Several `@openapi` lines merge in order, later
keys win. Values written there win over everything else:

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
`Option<Id>` the result stays nullable. On a schema that accepts `null`, an
`enum` of the override gets a `null` entry and a `const` becomes
`enum: [value, null]` (3.0: `nullable: true` with that `enum`), unless the
override sets `type` or `nullable` itself.

Invalid YAML, and text that is not a YAML mapping (`/// @openapi The id`),
is reported (`OF1001`) and ignored.

## Descriptions

Doc comments often explain the code to developers. Three directives choose
what reaches the document, on types, fields, variants and type aliases:

```rust,ignore
/// @openapi
#[derive(Serialize)]
struct ErrorBody {
    /// Borrowed for the constant messages, owned for the few that have to
    /// name a value back to the caller.
    /// @summary Error
    /// @description One sentence for a person saying what went wrong.
    /// @internal
    /// `Cow` rather than `String` everywhere: most messages are constants.
    error: std::borrow::Cow<'static, str>,
}
```

```yaml
error:
  type: string
  title: Error
  description: One sentence for a person saying what went wrong.
```

- `@description text` replaces the prose as the `description`. It continues
  on the following plain lines up to the next directive line; several
  `@description` lines are joined.
- `@summary text` (one line) sets the JSON Schema `title`.
- `@internal` drops the plain lines after it, up to the next directive line,
  wherever it stands (also before `@openapi`).
- Precedence: `description:` or `title:` in the `@openapi` YAML, then
  `@description` and `@summary`, then the prose. Without these directives
  nothing changes.
- A unit variant with `@summary` gets the `oneOf` of `const`s described in
  [enums](#enums), like one with an `@example`.

## Directives

| Directive | On | Effect |
| :--- | :--- | :--- |
| `@openapi` | type | Exports the type; YAML after it is merged |
| `@openapi rename "Name"` | type, field, variant | Uses another name; wins over `#[serde(rename)]` |
| `@openapi rename-all rule` | type | Renames all fields (structs) or variants (enums); rules as in serde: `camelCase`, `snake_case`, `kebab-case`, `PascalCase`, `SCREAMING_SNAKE_CASE`, `SCREAMING-KEBAB-CASE`, `lowercase`, `UPPERCASE` |
| `@openapi skip` | type, field, variant | Leaves it out of the document (a type alias becomes [transparent](#type-aliases)) |
| `@example value` | type, field, variant | Adds a YAML value (or plain text) to `examples`; repeatable |
| `@description text` | type, field, variant, type alias | Replaces the prose as `description`; continues on plain lines ([descriptions](#descriptions)) |
| `@summary text` | type, field, variant, type alias | Sets `title` |
| `@internal` | type, field, variant, type alias | Drops the plain lines after it from the document |
| `@openapi<T, U>` | generic type, type alias | Declares a [blueprint](#explicit-blueprints) with YAML that uses `$T` |
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

Which fields are required depends on the direction the type is used in,
read from its `derive` (also behind `cfg_attr`).

A type that derives `Serialize` but not `Deserialize` is an output type:
the schema describes what serde writes. A field is written unless it is
skipped, so it is required, also an `Option<T>` (serde writes `null`; the
schema allows it). With `#[serde(skip_serializing_if = "Option::is_none")]`
serde leaves `None` out instead: the field is optional and the schema is
that of `T`, without `null`. Any other `skip_serializing_if` makes the field
optional too, `#[serde(skip_serializing)]` as well. `#[serde(default)]` does
not matter for output.

Any other type (deriving `Deserialize`, both, or no visible derive) follows
both directions. A field is required unless one of these applies:

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

`required: true` or `required: false` in the `@openapi` override of a field
decides: the field joins or leaves the `required` list of its object (a
`required` list in the override stays a keyword of the property).

## Enums

Enums follow serde's representation:

| serde | Schema |
| :--- | :--- |
| default (externally tagged) | unit variants as a string `enum`; data variants as single-key objects; together a `oneOf` |
| `#[derive(Serialize_repr)]` ([serde_repr](https://docs.rs/serde_repr)) | an integer `enum` of the discriminants |
| `#[serde(tag = "type")]` | one component per variant (`{Enum}{Variant}`) with the tag property, combined with `oneOf` and a `discriminator` |
| `#[serde(tag = "t", content = "c")]` | like internally tagged, the data under the content key |
| `#[serde(untagged)]` | `anyOf` of the variants' content |

`#[default]` on a unit variant (with `#[derive(Default)]`) is the enum's
`default`. A field of such an enum with `#[serde(default)]` (or in a
container with `#[serde(default)]` and `#[derive(Default)]`) gets that
`default` too, also when the enum is defined in another file; with
`default = "path"` the value is unknown and left out.

A string `enum` cannot describe single values. When a unit variant has an
`@openapi` override, an `@example`, a `@summary` or `#[deprecated]`, the
enum also gets a `oneOf` with one `const` per unit variant that carries it,
together with the variants' doc comments. A `serde_repr` discriminant that is not an integer
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

A generic type alias with `@openapi` (`type Paged<T> = Page<T>;`) is a
template too; without `@openapi`, it is [transparent](#type-aliases). The
variants of a generic tagged enum that use a type parameter are instantiated
with it: `Event<User>` has the variants `EventCreated_User`, … and keeps the
variants without parameters (`EventDeleted`) shared.

### Explicit blueprints

`@openapi<T>` declares the template with YAML; `$T` stands for the argument.
This also works for types without fields and for type aliases
(`/// @openapi<T>` above `pub type Envelope<T> = …;`):

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

## Type aliases

A type alias with `@openapi` is exported like a struct: its schema is the
schema of its target with the description, overrides and `@example`s of its
doc comment; a generic one is a [template](#generics).

A type alias without `@openapi` (or with `@openapi skip`) is transparent. It
is not exported; where a documented type, a route, a channel message or a YAML
block refers to it, the reference is replaced by the alias's target, as if
the target had been written:

```rust,ignore
type Stamp = chrono::DateTime<chrono::Utc>;
pub type Owner = User;
type Paged<T> = Page<T>;

/// @openapi
pub struct Repo {
    pub created: Stamp,       // {type: string, format: date-time}
    pub owner: Owner,         // $ref: '#/components/schemas/User'
    pub members: Paged<User>, // $ref: '#/components/schemas/Page_User'
}
```

- An alias defined in the file of the reference wins, also over a
  documented schema of the same name. Otherwise, the aliases of that name in
  the other scanned files apply when no documented schema or template has the
  name and they all have the same target.
- Aliases of the same name with different targets make the name ambiguous in
  the other files: it is reported (`OF3012`) and the reference is left
  unresolved. Unused aliases produce nothing, so private aliases of the same
  name in several modules never collide.
- Chains of aliases are followed; the target of each alias is read in the
  file that defines it. An alias that refers to itself through other aliases
  is reported (`OF3013`); `type User = models::User;` is no cycle.
- A built-in name written bare (`Duration`) means an alias of that name only
  in the file that defines it.
- A target that cannot be documented (a function pointer, `dyn Trait`,
  `impl Trait`) is reported (`OF1010`) where the alias is used, once, at the
  alias; an unused alias is not.
- The schema of the target replaces the reference, so an alias of an
  `Option` makes the value nullable but not the field optional.
- An alias used as a type argument stands for its target, so the instance
  is named as if the target were written: `Page<Owner>` is `Page_User`, and
  `Page<List<User>>` with `type List<T> = Vec<T>;` is `Page_Vec_User`. A
  target that is no named type (a tuple, an array) keeps the alias's name.

## Types from other crates

A type that is not exported in the scanned sources is a reference to a
schema of the same name (`$ref: '#/components/schemas/Money'`), which is
reported when nothing defines it (`OF3007`). A type of another scanned crate
is found by its module path (see [names and collisions](#names-and-collisions)).
Three ways to define a type that is not scanned:

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

## Names and collisions

A Rust type in a field, a type argument, a type alias, a handler signature
or a route directive (`@return 200: Printing`) is looked up by its module
path, so two documented types with the same Rust name in different crates
or modules do not mix. Each scanned file has a module path: its crate (the
`[package] name` of the `Cargo.toml` next to its `src` directory, `-` read
as `_`; without one, the directory's name, and a manifest that cannot be
read or has no name is reported, `OF0007`) and the modules below `src`
(`mod.rs`, `lib.rs` and `main.rs` are their directory's module; a file
named by a `mod` declaration belongs to the declaring module, `#[path]`
included and also inside inline `mod` blocks, as rustc finds it; inline
`mod` blocks count). Single files given as input belong to the crate of their `src`
directory too. A name resolves, in this order, to:

1. the type of that name defined in the same module,
2. the type an explicit `use` names (groups, `as`, `self`, `super`, `crate`
   and re-exports with `pub use` are followed),
3. the type a glob import (`use a::*;`) brings in, when only one does,
4. for a written path (`baylee_cardtext::Printing`), the type of that path,
   else the one documented type whose path ends with it,
5. the one documented type of that name in all sources.

The reference uses the schema name of that type, renames applied: with
`use baylee_catalog::Printing;`, a field `Vec<Printing>` points to
`CatalogPrinting` when that type is renamed so. A name written with type
arguments (`Page<User>`) means a template (a generic type), one without
means a schema, so a template `Page<T>` and a struct `Page` do not mix; the
template and every argument are looked up so. Several candidates, or a
type of the same module that is not documented while other documented
types have its name, are reported (`OF3014`); the schema then has no
reference, and a generic type with such a template or argument is not
instantiated. A `$Name` written in a directive or in YAML is a schema name
and is not looked up by module.

Two documented types whose schema names are equal (no rename) are no longer
merged: each is named by its module path with dots, `crate.module.Name`
(`baylee_catalog.model.Status`), each is reported (`OF1004`), and the
references above point to these names. Give them distinct names with
`@openapi rename` to choose the names yourself. The same type read twice
(the same file and module) is one type. Two templates of one name are
qualified the same way (`baylee_catalog.model.Page`, instances
`baylee_catalog.model.Page_User`); a template and a schema of one name do
not collide. Schemas written in YAML keep their names.

## References in YAML

In any YAML block, a value `$Name` is a reference to the schema `Name`:
`$ref: $User` becomes `$ref: '#/components/schemas/User'`, and
`$Page<User>` instantiates the template. Only whole values are replaced;
`$User` inside a sentence stays text. Write `\$` for a literal dollar sign at
the start of a value.

Where a schema is expected, `$Name` alone is the reference object: the
items of `allOf`, `anyOf`, `oneOf` and `prefixItems`; the value of `items`,
`not`, `additionalProperties`, `unevaluatedProperties`, `unevaluatedItems`,
`contains`, `propertyNames`, `if`, `then`, `else` and `contentSchema`; the
values of `properties`, `patternProperties`, `$defs` and
`dependentSchemas`; and the `schema` or `itemSchema` of media types and
parameters. So `/// @return 409: { oneOf: [$ErrorBody, $Conflict] }` is a
`oneOf` of two references. Elsewhere (a `discriminator` mapping, examples,
`enum` values, extensions) `$Name` is the pointer string
`'#/components/schemas/Name'`.

## Validation constraints

`#[validate(...)]` and `#[garde(...)]` add JSON Schema keywords; for example
`length(min = 1, max = 64)` becomes `minLength`/`maxLength` on strings,
`minItems`/`maxItems` on lists and `minProperties`/`maxProperties` on maps.
A `regex(path = RE)` refers to a constant; when a
`static RE: LazyLock<Regex> = LazyLock::new(|| Regex::new("…").unwrap())`
(or `Lazy`, `lazy_static!`, `OnceLock`) with a literal pattern is in the
scanned sources, its pattern is used, otherwise the finding `OF1007` reports
it. When several modules define a constant of that name, a path selects one
by its modules (`regex(path = *crate::a::RE)` or `a::RE` finds `RE` in
`src/a.rs` or `src/a/mod.rs`; constants in inline `mod` blocks count as part
of their file), and a bare name the one in the same file; constants with
different patterns that still match are reported (`OF1011`) and left out.
Values that are not literals (`length(max = MAX)`) are reported (`OF1006`)
and left out. See the
[validator and garde tables](compatibility.md#validator).
