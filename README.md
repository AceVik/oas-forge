<h1 align="center">
  <a href="https://github.com/AceVik/oas-forge">
    <img src="https://raw.githubusercontent.com/AceVik/oas-forge/main/docs/assets/banner.svg" alt="oas-forge: compile-time OpenAPI for Rust" width="100%">
  </a>
</h1>

<p align="center">
  <a href="https://crates.io/crates/oas-forge"><img src="https://img.shields.io/crates/v/oas-forge.svg?logo=rust" alt="crates.io"></a>
  <a href="https://docs.rs/oas-forge"><img src="https://img.shields.io/docsrs/oas-forge?logo=docs.rs" alt="docs.rs"></a>
  <a href="https://github.com/AceVik/oas-forge/actions/workflows/ci.yml"><img src="https://github.com/AceVik/oas-forge/actions/workflows/ci.yml/badge.svg" alt="CI"></a>
  <a href="https://github.com/AceVik/oas-forge/blob/main/Cargo.toml"><img src="https://img.shields.io/badge/MSRV-1.89-blue?logo=rust" alt="Minimum supported Rust version: 1.89"></a>
  <a href="https://github.com/AceVik/oas-forge/blob/main/LICENSE"><img src="https://img.shields.io/crates/l/oas-forge.svg" alt="License: MIT"></a>
  <a href="https://crates.io/crates/oas-forge"><img src="https://img.shields.io/crates/d/oas-forge.svg" alt="Downloads"></a>
</p>

**OpenAPI for Rust, generated at build time from the code you already
write.** `oas-forge` reads your types, their serde, validator and garde
attributes, your doc comments and your axum or actix-web handlers, and writes
an **OpenAPI 3.2, 3.1 or 3.0** document, plus **Swagger 2.0**, a **JSON
Schema** bundle and **AsyncAPI 3** on request. It runs in `build.rs` or on the
command line: no proc macros, no runtime dependency, nothing in your binary.

<p align="center">
  <img src="https://raw.githubusercontent.com/AceVik/oas-forge/main/docs/assets/how-it-works.svg" alt="Rust types and handlers go into oas-forge at build time, which writes OpenAPI, Swagger, JSON Schema and AsyncAPI documents" width="100%">
</p>

## Highlights

- **Schemas that match the wire format.** `rename`, `rename_all`, `flatten`,
  `skip`, `default`, `transparent`, `untagged`, internally and adjacently
  tagged enums: the schema describes exactly the JSON serde writes. The
  rules of [`validator`](https://crates.io/crates/validator) and
  [`garde`](https://crates.io/crates/garde) become JSON Schema keywords;
  chrono, time, jiff, uuid, url, rust_decimal, bytes and the standard
  collections are mapped out of the box.
- **A route DSL that stays out of your way.** `@route`, `@return`,
  `@problem` (RFC 9457), `@security`, typed path, query and header
  parameters, request bodies, streaming responses, webhooks and module-wide
  defaults, with any standard OpenAPI YAML embedded where the DSL ends.
- **Handler inference.** `Path<T>`, `Query<T>`, `Json<T>`, `Form<T>` and
  `Multipart` extractors and `Json<T>` return types of axum and actix-web
  fill in what the doc comment leaves open.
- **Every format from one source.** OpenAPI 3.2, 3.1 and 3.0 (YAML or JSON),
  a Swagger 2.0 export for older tooling, a JSON Schema 2020-12 bundle and
  AsyncAPI 3 for event channels. What a target cannot express is reported,
  never dropped silently.
- **Reuse.** Generic types (`Page<User>` becomes `Page_User`), shared YAML
  fragments, `{{CARGO_PKG_VERSION}}` and `{{env:NAME}}` variables, and
  templates shared across crates of a workspace.
- **Several documents from one source.** `@scope` splits one code base into
  separate documents, such as a public API, an admin listener and a second
  service, each with its own root and only the schemas it uses.
- **Written like Rust docs.** Annotations live in doc comments and can use
  backticks and fenced YAML, so they render well in rustdoc and pass
  `clippy::doc_markdown`; `@internal` keeps developer notes out of the
  public document.
- **Built for CI.** Byte-identical output on every platform, files written
  only when they change, `--check` for stale documents, diagnostics with
  stable codes (`--message-format json`), and every generated document
  validated against the official schemas in the test suite.

## Quick start

Add the generator as a build dependency:

```toml
[build-dependencies]
oas-forge = "0.2"
```

Call it from `build.rs`:

```rust,no_run
// build.rs
fn main() {
    oas_forge::Generator::new()
        .input("src")
        .output("openapi.yaml")
        // Rebuild when a source changes; report findings as Cargo warnings.
        .emit_cargo_directives(true)
        .generate()
        .expect("OpenAPI generation failed");
}
```

Annotate your types and handlers:

<!-- corpus: readme_quickstart -->
```rust
use serde::{Deserialize, Serialize};

/// A product in the catalog.
/// @openapi
#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Product {
    pub id: u64,
    /// Shown in the shop.
    pub display_name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub price_cents: Option<u32>,
}

/// List products
///
/// Returns one page of the catalog.
/// @route GET /products
/// @query-param page: u32 default=1 "Page number"
/// @return 200: Vec<$Product>
pub async fn list_products() {}

/// Get a product
/// @route GET /products/{id: u64 "Product id"}
/// @return 200: $Product
/// @problem 404 "No such product"
pub async fn get_product() {}
```

`cargo build` writes `openapi.yaml`. Without an `openapi`/`info` block in the
sources, the document's root comes from `Cargo.toml` (name, version,
description, license, homepage):

```yaml
openapi: "3.1.1"
info:
  title: shop
  version: "1.0.0"
paths:
  /products:
    get:
      summary: List products
      description: Returns one page of the catalog.
      operationId: list_products
      parameters:
      - name: page
        in: query
        required: false
        description: Page number
        schema:
          type: integer
          format: int64
          minimum: 0
          maximum: 4294967295
          default: 1
      responses:
        "200":
          description: OK
          content:
            application/json:
              schema:
                type: array
                items:
                  $ref: "#/components/schemas/Product"
  "/products/{id}":
    get:
      summary: Get a product
      operationId: get_product
      # … the id parameter, the 200 response and an RFC 9457 problem response
components:
  schemas:
    # … ProblemDetails
    Product:
      type: object
      properties:
        id:
          type: integer
          format: int64
          minimum: 0
        displayName:
          type: string
          description: Shown in the shop.
        priceCents:
          type:
          - integer
          - "null"
          format: int64
          minimum: 0
          maximum: 4294967295
      required:
      - id
      - displayName
      description: A product in the catalog.
```

Serve it with Swagger UI, Redoc or Scalar, generate clients from it, or embed
it in your binary with `include_str!` ([how](https://github.com/AceVik/oas-forge/blob/main/docs/getting-started.md#embedding-the-document)).

## Command line

The same generator as a standalone tool, for projects without a build script
and for CI:

```bash
cargo install oas-forge-cli

oas-forge -i src -o openapi.yaml
oas-forge -i src -o openapi.yaml --check --deny-warnings   # fail when stale or noisy
oas-forge -i src --output-openapi-30 openapi-3.0.json --output-swagger2 swagger.json
```

Settings can also live in `[package.metadata.oas-forge]` of `Cargo.toml` or
in an `openapi.toml`; see [the CLI](https://github.com/AceVik/oas-forge/blob/main/crates/oas-forge-cli/README.md)
and [Getting started](https://github.com/AceVik/oas-forge/blob/main/docs/getting-started.md).

## A short tour

### Types

Structs, enums and type aliases with an `@openapi` line are exported as they
serialize:

<!-- corpus: readme_schemas -->
```rust
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
```

`Payment` becomes a `oneOf` with a discriminator, the flattened `address` an
`allOf`, the validator rules `format: email`, `minLength` and `maxLength`,
`tags` an array with `uniqueItems`, and `fax` is marked `deprecated`.
[Schemas](https://github.com/AceVik/oas-forge/blob/main/docs/schemas.md)
explains every directive; the
[compatibility tables](https://github.com/AceVik/oas-forge/blob/main/docs/compatibility.md)
list every supported attribute and type.

### Routes

The DSL covers the common cases. Any other field of the OpenAPI operation can
be written as YAML in the same block:

<!-- corpus: readme_dsl -->
```rust
/// @openapi
/// openapi: 3.1.0
/// info: { title: Orders, version: 1.0.0 }
/// components:
///   securitySchemes:
///     bearer: { type: http, scheme: bearer }
pub fn api() {}

/// An order.
/// @openapi
pub struct Order {
    pub id: u64,
    pub items: Vec<String>,
}

/// Create an order
/// @route POST /orders
/// @tag Orders
/// @security bearer
/// @header-param Idempotency-Key: Uuid required "Makes retries safe"
/// @body $Order "The order to place"
/// @return 201: $Order "Created"
/// @return-header 201 Location: String "URL of the new order"
/// @problem 422 "The order is invalid"
/// callbacks:
///   shipped:
///     '{$request.body#/callbackUrl}':
///       post:
///         responses: { '204': { description: Received } }
pub async fn create_order() {}
```

### Handlers

With axum or actix-web, the signature fills what the block leaves open:
path parameter types, query parameters from a struct, the request body and
the success response.

```rust,ignore
/// Update a user
/// @route PUT /users/{id}
/// @problem 404 "No such user"
pub async fn update_user(
    Path(id): Path<u64>,
    Json(body): Json<UpdateUser>,
) -> Result<Json<User>, ApiError> {
    // …
}
```

The [route DSL reference](https://github.com/AceVik/oas-forge/blob/main/docs/route-dsl.md)
covers parameters, bodies, streaming (`text/event-stream`), webhooks, module
defaults (`@route-defaults`) and inference.

### Generics, fragments and variables

Generic types are instantiated where they are used, YAML fragments are
reused with `@insert` and `@extend`, and placeholders are filled in at build
time:

```yaml
openapi: 3.1.0
info:
  title: Orders
  version: "{{CARGO_PKG_VERSION}}"   # from Cargo.toml
servers:
  - url: "{{env:API_URL}}"            # from the build environment
```

A `$Page<Order>` anywhere in the API becomes a concrete `Page_Order` schema.
See [Templates](https://github.com/AceVik/oas-forge/blob/main/docs/templates.md),
including templates shared by the crates of a workspace.

### Events

`@channel` blocks document message-driven APIs as AsyncAPI 3, sharing the
component schemas with the OpenAPI document: WebSocket handshakes
(`@ws-query ticket: String required`), JSON messages from serde types and
protobuf messages straight from your `.proto` files
(`@send Frame: proto(app.v1.Envelope)`); see
[AsyncAPI](https://github.com/AceVik/oas-forge/blob/main/docs/asyncapi.md).

## Outputs

One run writes any combination of these files. The extension (`.yaml`, `.yml`
or `.json`) selects the format.

| Builder method | CLI flag | Content |
| :--- | :--- | :--- |
| `output` | `-o`, `--output` | The full document in the version of its root: OpenAPI 3.2, 3.1 or 3.0 |
| `output_schemas` | `--output-schemas` | Only `components.schemas` |
| `output_paths` | `--output-paths` | Only `paths` |
| `output_fragments` | `--output-fragments` | Everything except the root, plus templates, for other crates to `include` |
| `output_openapi_30` | `--output-openapi-30` | An OpenAPI 3.0.3 export |
| `output_swagger2` | `--output-swagger2` | A Swagger 2.0 export |
| `output_json_schema` | `--output-json-schema` | A JSON Schema 2020-12 bundle of the component schemas |
| `output_asyncapi` | `--output-asyncapi` | The AsyncAPI 3 document of `@asyncapi` and `@channel` blocks |

[Outputs](https://github.com/AceVik/oas-forge/blob/main/docs/outputs.md)
lists what each export approximates.

## Compatibility

| Area | Supported |
| :--- | :--- |
| OpenAPI | 3.2, 3.1, 3.0 (output and embedded YAML); Swagger 2.0 export |
| serde | container, field and variant attributes, all four enum representations, `flatten`, `cfg_attr` |
| Validation | `validator` and `garde` rules as JSON Schema keywords |
| Web frameworks | axum and actix-web extractors and return types (inference); any framework through the DSL |
| Types | std (incl. `NonZero*`, `Duration`, `IpAddr`), chrono, time, jiff, uuid, ulid, url, rust_decimal, bigdecimal, bytes, indexmap, smallvec, `serde_json::Value`; your own mappings in `openapi.toml` or with `Generator::map_type` |
| Formats | YAML and JSON input and output |

Details: [compatibility tables](https://github.com/AceVik/oas-forge/blob/main/docs/compatibility.md).

## Why at build time?

Most Rust OpenAPI crates describe the API with derive and attribute macros
(such as utoipa) or assemble the document at runtime from the router (such
as aide). `oas-forge` reads the source instead:

- your application gets **no new dependency and no macros**; the
  annotations are plain doc comments that also read well in rustdoc,
- the document is **a file**: reviewed in pull requests, diffed, checked in
  CI and served as a static asset,
- **one source, many formats**: the same annotations produce OpenAPI 3.2,
  3.1 and 3.0, Swagger 2.0, JSON Schema and AsyncAPI.

The trade-off: it reads code rather than running it, so a type that only
exists after macro expansion needs a mapping or a YAML schema.

## Use in CI

```yaml
# .github/workflows/api.yml
- run: cargo install oas-forge-cli --locked
- run: oas-forge -i src -o openapi.yaml --check --deny-warnings --message-format json
```

`--check` fails when the committed document is out of date,
`--deny-warnings` when any finding is a warning, and `--strict` adds lints
for missing descriptions and unused components. In a build script,
`Generator::deny_warnings(true)` fails the build instead.
[Diagnostics](https://github.com/AceVik/oas-forge/blob/main/docs/diagnostics.md)
lists every code with its fix.

## Documentation

| Guide | Content |
| :--- | :--- |
| [Getting started](https://github.com/AceVik/oas-forge/blob/main/docs/getting-started.md) | build scripts, the CLI, configuration, embedding, CI |
| [Schemas](https://github.com/AceVik/oas-forge/blob/main/docs/schemas.md) | types, directives, examples, generics |
| [Route DSL](https://github.com/AceVik/oas-forge/blob/main/docs/route-dsl.md) | every directive, embedded YAML, inference, module defaults |
| [Templates](https://github.com/AceVik/oas-forge/blob/main/docs/templates.md) | root documents, fragments, variables, cross-crate templates |
| [AsyncAPI](https://github.com/AceVik/oas-forge/blob/main/docs/asyncapi.md) | channels and messages |
| [Compatibility](https://github.com/AceVik/oas-forge/blob/main/docs/compatibility.md) | serde, validator, garde, types and frameworks |
| [Outputs](https://github.com/AceVik/oas-forge/blob/main/docs/outputs.md) | formats, versions and what each export approximates |
| [Diagnostics](https://github.com/AceVik/oas-forge/blob/main/docs/diagnostics.md) | every code and how to fix it |
| [Architecture](https://github.com/AceVik/oas-forge/blob/main/docs/architecture.md) | the pipeline and the [decision records](https://github.com/AceVik/oas-forge/blob/main/docs/adr/README.md) |
| [Migrating from 0.1](https://github.com/AceVik/oas-forge/blob/main/docs/migration-0.2.md) | what changed in 0.2 |
| [API reference](https://docs.rs/oas-forge) | the library on docs.rs |

## Requirements and stability

- **Rust 1.89** or newer (edition 2024). Raising the minimum is a minor
  release, never a patch release.
- The library pulls in no CLI dependencies: build scripts compile `syn`,
  `serde_json`, a YAML parser, `toml`, `regex` and `walkdir`.
- Semantic versioning; the public API of every pipeline stage is
  documented and checked with `cargo-semver-checks`. Diagnostic codes are
  stable.
- `#![forbid(unsafe_code)]`, dependencies audited with `cargo deny`, CI on
  Linux, macOS and Windows.

## Contributing and security

Contributions are welcome; [CONTRIBUTING.md](https://github.com/AceVik/oas-forge/blob/main/CONTRIBUTING.md)
explains the layout and the checks. Report vulnerabilities privately as
described in [SECURITY.md](https://github.com/AceVik/oas-forge/blob/main/SECURITY.md).

## License

Licensed under the [MIT license](https://github.com/AceVik/oas-forge/blob/main/LICENSE).
