# oas-forge

[![Crates.io](https://img.shields.io/crates/v/oas-forge.svg)](https://crates.io/crates/oas-forge)
[![Docs.rs](https://docs.rs/oas-forge/badge.svg)](https://docs.rs/oas-forge)
[![CI](https://github.com/AceVik/oas-forge/actions/workflows/ci.yml/badge.svg)](https://github.com/AceVik/oas-forge/actions/workflows/ci.yml)
[![License](https://img.shields.io/crates/l/oas-forge.svg)](LICENSE)

**Compile-time OpenAPI for Rust.** `oas-forge` reads your Rust code and doc
comments in `build.rs` (or from the command line) and writes an OpenAPI
document. There are no macros and nothing runs in your binary.

- **Schemas match what serde writes**: `rename`, `rename_all`, `flatten`,
  `skip`, `default`, `transparent`, all four enum representations and more.
  The constraints of [`validator`](https://crates.io/crates/validator) and
  [`garde`](https://crates.io/crates/garde) become JSON Schema keywords.
- **A compact route DSL** above your handlers (`@route`, `@return`,
  `@problem`, …) that accepts any embedded OpenAPI YAML. Parameters, bodies
  and responses are inferred from axum and actix-web extractors.
- **Every format from one source**: OpenAPI 3.2, 3.1 and 3.0, a Swagger 2.0
  export, a JSON Schema bundle and AsyncAPI 3 for event channels.
- **Generics** (`Page<User>`), reusable fragments, cross-crate templates and
  `{{variables}}`.
- **Built for CI**: deterministic output, written only when it changes,
  validated against the official schemas in the test suite, `--check` for
  stale files and diagnostics with stable codes.

## Quick start

```toml
# Cargo.toml
[build-dependencies]
oas-forge = "0.2"
```

```rust,no_run
// build.rs
fn main() {
    oas_forge::Generator::new()
        .input("src")
        .output("openapi.yaml")
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

`cargo build` writes `openapi.yaml`. Without an `openapi`/`info` block in
the sources, the root comes from `Cargo.toml`:

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

## Command line

```bash
cargo install oas-forge-cli
oas-forge -i src -o openapi.yaml
oas-forge -i src -o openapi.yaml --check          # CI: fail when the file is stale
oas-forge -i src --output-openapi-30 v3.0.yaml --output-swagger2 v2.yaml
```

The same settings fit in `[package.metadata.oas-forge]` of `Cargo.toml` or in
an `openapi.toml`; see [Getting started](docs/getting-started.md).

## A short tour

**Types.** Structs, enums and type aliases with an `@openapi` line are
exported as they serialize:

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

`Payment` becomes a `oneOf` with a discriminator, `address` an `allOf`, the
`validator` rules `format: email`, `minLength` and `maxLength`, and `tags` an
array with `uniqueItems`. See [Schemas](docs/schemas.md) and the
[compatibility tables](docs/compatibility.md).

**Routes.** The DSL covers the common cases; any other field of the
OpenAPI operation can be written as YAML in the same block:

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

See the [route DSL reference](docs/route-dsl.md) for parameters, bodies,
streaming responses, webhooks, module defaults and signature inference.

**More:** generic types are instantiated where they are used
(`Page<User>` becomes `Page_User`), YAML fragments are reused with
`@insert` and `@extend`, and `{{CARGO_PKG_VERSION}}` or `{{env:NAME}}`
placeholders are filled in. See [Templates](docs/templates.md).

## Outputs

| Builder method | CLI flag | Content |
| :--- | :--- | :--- |
| `output` | `-o`, `--output` | The full document; the root's `openapi` version (3.2, 3.1 or 3.0) decides the schema dialect |
| `output_schemas` / `output_paths` | `--output-schemas` / `--output-paths` | Only `components.schemas` or `paths` |
| `output_fragments` | `--output-fragments` | Everything without a root, plus templates, for other crates to `include` |
| `output_openapi_30` | `--output-openapi-30` | An OpenAPI 3.0.3 export |
| `output_swagger2` | `--output-swagger2` | A Swagger 2.0 export |
| `output_json_schema` | `--output-json-schema` | A JSON Schema 2020-12 bundle of the component schemas |
| `output_asyncapi` | `--output-asyncapi` | The AsyncAPI 3 document of `@asyncapi` and `@channel` blocks |

The file extension (`.yaml`, `.yml` or `.json`) selects the format. What a
target cannot express is reported, never dropped silently. See
[Outputs](docs/outputs.md).

## Documentation

- [Getting started](docs/getting-started.md): build scripts, the CLI,
  configuration and CI
- [Schemas](docs/schemas.md): types, directives, examples and generics
- [Route DSL](docs/route-dsl.md): every directive and how YAML is embedded
- [Templates](docs/templates.md): fragments, variables, root documents and
  cross-crate templates
- [AsyncAPI](docs/asyncapi.md): channels and messages
- [Compatibility](docs/compatibility.md): serde, validator, garde, types and
  web frameworks
- [Outputs](docs/outputs.md): formats, versions and what each export loses
- [Diagnostics](docs/diagnostics.md): every code and how to fix it
- [Architecture](docs/architecture.md) and the
  [decision records](docs/adr/README.md)
- [Migrating from 0.1](docs/migration-0.2.md)
- API reference on [docs.rs](https://docs.rs/oas-forge)

## Requirements

Rust 1.89 or newer. The library has no CLI dependencies; build scripts only
compile `syn`, `serde_json`, a YAML parser, `toml`, `regex` and `walkdir`.

## Contributing and security

See [CONTRIBUTING.md](CONTRIBUTING.md) and [SECURITY.md](SECURITY.md).

## License

[MIT](LICENSE)
