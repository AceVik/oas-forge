# Documents and templates

Besides types and routes, the sources can contain plain OpenAPI YAML: the
root document, extra paths or components, reusable fragments and named
schemas. All of it is merged into one document.

## YAML blocks

An `@openapi` line followed by YAML on a function, a module, a unit struct
or in a file comment (`//!`) is a free document snippet:

```rust,ignore
/// @openapi
/// paths:
///   /health:
///     get:
///       summary: Liveness probe
///       responses:
///         '204': { description: Alive }
pub async fn health() {}
```

- Several blocks in one `//!` comment are separate snippets. A block ends at
  the next `@openapi…` header, at a separator line such as `//! ---` (note
  `OF1003`), at a gap in the comment, or at a paragraph of prose: a line
  after a blank line, at the indentation of the block, that cannot start
  YAML (`Some prose.`; a line such as `key: value`, `- item`, `# comment`
  or `{…}` can). Prose before the first header describes the Rust item and
  is ignored.
- `@openapi document` (also `@openapi path` and `@openapi operation`, which
  only mean something in a [route](route-dsl.md#embedded-openapi)) is such a
  block, also on a struct, enum or type alias: there it is not the type's
  schema override and does not export the type.
- `.yaml`, `.yml` and `.json` files in the input directories, and files
  passed with `include`, are snippets as well. A missing include is reported
  (`OF0001`).
- Snippets must be mappings at the top level; anything else is an error with
  its location.
- `$Name` values are [smart references](schemas.md#references-in-yaml).
- The YAML may be a fenced code block right after the header line
  (```` ```yaml ````, ```` ``` ````, `~~~`), so that rustdoc renders it as
  code: the fences are removed and the block ends at the closing fence (see
  [rustdoc-friendly annotations](getting-started.md#annotations-that-rustdoc-renders-as-code)).
  This works for every `@openapi…` header, `@asyncapi`, `@openapi-fragment`
  and `@openapi-type`, and for overrides of types and fields.

## Root documents

Exactly one snippet is the root: it has `openapi` and `info`. Two roots are
an error that names both locations.

<!-- corpus: guide_root -->
```rust
//! @openapi
//! openapi: 3.1.1
//! info:
//!   title: Accounts
//!   version: '{{CARGO_PKG_VERSION}}'
//!   description: |
//!     User accounts and sessions.
//!
//!     Built from revision `{{GIT_REVISION}}`.
//! servers:
//!   - url: https://{{STAGE}}.example.com
//! security:
//!   - session: []
//! components:
//!   securitySchemes:
//!     session:
//!       type: openIdConnect
//!       openIdConnectUrl: $$ISSUER_URL/.well-known/openid-configuration

/// Who is signed in
/// @route GET /me
/// @return 200: String
pub async fn me() {}
```

Here `{{CARGO_PKG_VERSION}}`, `{{GIT_REVISION}}` and `{{STAGE}}` are filled in
at build time (see [variables](#variables)), while `$$ISSUER_URL` stays in the
document for the service to replace when it serves the document.

Without any root, the root is generated from the package metadata: `openapi:
3.1.1`, `info.title` from the package name, `info.version`,
`info.description`, `info.license` (SPDX) and `externalDocs` from the
homepage or repository (note `OF0004`). An `info` block without `openapi`
is completed with the package values instead. Build scripts use the running
package automatically, the CLI the `Cargo.toml` of the working directory;
`Generator::without_package()` turns this off.

The `openapi` version of the root selects the schema dialect of the whole
document; see [Outputs](outputs.md#versions).

## Merging

Snippets are merged in source order (files sorted by path, items in file
order):

| Value | Merge |
| :--- | :--- |
| mappings | recursively, key by key |
| lists | appended, duplicates removed; `parameters` are matched by name and location; entries added to a `security` or `servers` list another snippet set, and such a list added to an operation or path another snippet defined, are reported (`OF4001`): a joined `security` list accepts either requirement, and an operation's own list replaces the document's |
| positional lists (`allOf`, `oneOf`, `anyOf`, `prefixItems`) | a different list replaces the old one, with a warning |
| scalars | the later value wins; a different value is reported (`OF4001`) with both locations |

Top-level `tags` become unique tag objects. Two items that produce the same
schema name are merged too (`OF1004`); rename one of them to keep both.
The full rules are in [ADR 0003](adr/0003-merge-policies.md).

## Fragments

A fragment is reusable YAML with parameters:

<!-- corpus: guide_fragments -->
```rust
//! @openapi
//! openapi: 3.1.0
//! info: { title: Admin, version: 1.0.0 }
//! components:
//!   securitySchemes:
//!     oidc: { type: openIdConnect, openIdConnectUrl: 'https://id.example.com/.well-known/openid-configuration' }
//!
//! @openapi-fragment Paging
//! - name: page
//!   in: query
//!   schema: { type: integer, minimum: 1, default: 1 }
//! - name: size
//!   in: query
//!   schema: { type: integer, minimum: 1, maximum: 100, default: 20 }
//!
//! @openapi-fragment Secured(role)
//! security:
//!   - oidc: [{{role}}]
//! responses:
//!   '401': { description: Not signed in }
//!   '403': { description: 'Requires the {{role}} role' }

/// List audit events
/// @route GET /audit
/// @return 200: Vec<String>
/// @extend Secured(admin)
/// parameters:
///   @insert Paging
pub async fn audit() {}

/// @openapi
/// paths:
///   /reports:
///     get:
///       summary: List reports
///       @extend Secured("auditor")
///       responses:
///         '200': { description: The reports }
pub async fn reports() {}
```

- `@openapi-fragment Name(param, …)` defines a fragment, in a file comment
  or in the doc comment of any item, field or variant (also of a type that
  is not exported). The parameters are optional; `{{param}}` in the body is
  replaced by the argument. The body ends at the next `@openapi…` line (on
  a type, field or variant also at an `@example` line) or where the doc
  comment is interrupted by a line that is not a doc comment, such as a
  blank line between two `///` groups; a fragment above a type is not part
  of the type's description or `@openapi` override.
- `@insert Name(args)` pastes the fragment's text at that line, indented
  like the line, before the YAML is parsed. Use it for list items. Inserts
  may insert other fragments, up to a depth of 8. `@insert Name` without
  arguments and without such a fragment means a reusable parameter:
  `- $ref: '#/components/parameters/Name'`.
- `@extend Name(args)` parses the fragment and merges it into the mapping
  that contains the line. Values written next to it win, like inheritance.
  It also works in routes. The fragment's text is expanded like any other
  YAML first: `{{variables}}`, `@insert` and `@extend` inside it work, up
  to a depth of 8.
- A fragment that inserts or extends itself, directly or through others,
  is reported (`OF3010`); the repeated `@insert` or `@extend` is skipped.
- Arguments are separated by commas; quote an argument that contains one
  (`Secured("a, b")`).
- The fragment may be written in Markdown backticks for rustdoc, where it is
  used (`` @insert `Paging` ``, `` @extend `Secured(admin)` ``) and where it
  is defined (`` @openapi-fragment `Secured(role)` ``).

Unknown fragments (`OF3001`), invalid fragment YAML (`OF3002`), missing
arguments (`OF3003`) and extra arguments (`OF3011`, ignored) are reported.
Expansion also has a total budget of 16 MiB of processed text and 65,536
visited lines or extension resolutions per expansion. Exceeding it reports
error `OF3016`, even when the fragments form no cycle.

## Named schemas

`@openapi-type Name` defines a schema in YAML, for types from other crates
or types without Rust definition:

```rust,ignore
//! @openapi-type Money
//! type: string
//! pattern: '^-?\d+\.\d{2}$'
//! examples: ['12.50']
```

Fields of type `Money` and `$Money` in YAML refer to it.

## Variables

`{{NAME}}` placeholders work in every YAML block, route, channel, type
override and `@example`:

| Placeholder | Value |
| :--- | :--- |
| `{{CARGO_PKG_NAME}}`, `{{CARGO_PKG_VERSION}}`, `{{CARGO_PKG_VERSION_MAJOR}}`, …, `{{CARGO_PKG_DESCRIPTION}}`, `{{CARGO_PKG_LICENSE}}`, `{{CARGO_PKG_REPOSITORY}}`, `{{CARGO_PKG_HOMEPAGE}}` | the package; in a build script from Cargo, in the CLI from `Cargo.toml` |
| `{{NAME}}` | `Generator::var("NAME", value)`, `[variables]` in the configuration or `--var NAME=VALUE` |
| `{{env:NAME}}` | the environment variable `NAME` at build time, if it is allowed with `Generator::allow_env("NAME")`, `allow_env = ["NAME"]` in the configuration or `--allow-env NAME`; other and unset variables are kept and reported (`OF3009`) |

The sources choose the names of `{{env:…}}` placeholders, so only the
variables you allow are read: a source from an untrusted change (a pull
request built in CI) cannot copy a secret of the build environment into
the document.

A placeholder that is the whole value of a mapping entry or list item
(also `- version: {{CARGO_PKG_VERSION}}`) is inserted as a quoted string, so
`version: {{CARGO_PKG_VERSION}}` stays the string `"1.10"` instead of the
number `1.1`. Inside block scalars (`description: |`) the value is inserted
as it is. The summary, description and directive lines of route, webhook and
channel blocks are not YAML, so there the value is always inserted as it is:
`Since: {{CARGO_PKG_VERSION}}` reads `Since: 1.10`, and `@return 200: {{TYPE}}`
names the type. Unknown placeholders are left as they are.

Whole YAML string values preserve newlines and control characters through
escaped quoting. Variable substitution is limited to 16 MiB of expanded
text and 65,536 source lines per call; exceeding either reports `OF3016`.

With `emit_cargo_directives(true)`, every allowed `{{env:NAME}}` in the
inputs is also printed as `cargo:rerun-if-env-changed=NAME`, so Cargo reruns the
build script when the variable changes.

## Escapes

`\$` at the start of a value writes a literal `$` (so `\$User` is the text
`$User`, not a reference). A value such as `$$ISSUER_URL/…` is not a
reference and needs no escape. Regular expressions keep `\$` as written:
in `pattern` values (and `pattern=` flags) and `patternProperties` keys it is
the regex escape of a literal dollar sign (`pattern: '^\$\d+$'`).

## Cross-crate templates

A library crate can publish its schemas, generic templates and fragments for
other crates:

```rust,no_run
// build.rs of the shared crate
fn main() {
    let out = std::path::Path::new(&std::env::var("OUT_DIR").unwrap()).join("fragments.yaml");
    oas_forge::Generator::new()
        .input("src")
        .output_fragments(&out)
        .emit_cargo_directives(true)
        .generate()
        .expect("OpenAPI fragments");
    // With `links = "shared_models"` in Cargo.toml, dependents read this
    // path from DEP_SHARED_MODELS_FRAGMENTS.
    println!("cargo::metadata=fragments={}", out.display());
}
```

The fragments output has no root. Besides `paths` and `components`, it
carries the templates in `components.x-oas-forge-templates` and
`components.x-oas-forge-fragments` with a format marker
(`x-oas-forge-format: 1`, [ADR 0005](adr/0005-template-wire-format.md)).
A crate that `include`s the file can use the shared types, instantiate
`$Paginated<User>` with its own `User`, and `@insert` or `@extend` the shared
fragments. The `x-oas-forge-*` keys never appear in a full document.

The [`examples/`](../examples) workspace shows the setup: `shared-models`
declares `links`, so Cargo runs its build script first, and `microservice`
depends on it and includes the file.
