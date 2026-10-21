# oas-forge guide

`oas-forge` turns Rust code and doc comments into OpenAPI, Swagger 2.0, JSON
Schema and AsyncAPI documents at build time. This guide explains every
annotation and option; the API reference is on
[docs.rs](https://docs.rs/oas-forge).

| Page | What it covers |
| :--- | :--- |
| [Getting started](getting-started.md) | Build scripts, the CLI, configuration files, CI checks, serving the document |
| [Schemas](schemas.md) | Which types are exported, doc comments, `@openapi` overrides, renaming, `skip`, `@example`, generics, custom types |
| [Route DSL](route-dsl.md) | `@route`, parameters, bodies, responses, security, embedded OpenAPI YAML, webhooks, module defaults, signature inference |
| [Templates](templates.md) | Root documents, YAML blocks and files, fragments (`@insert`, `@extend`), variables, cross-crate templates, merge rules |
| [AsyncAPI](asyncapi.md) | `@asyncapi` documents and the `@channel` DSL |
| [Compatibility](compatibility.md) | serde, validator and garde attributes, the type table, axum and actix-web extractors |
| [Outputs](outputs.md) | Output kinds, OpenAPI versions, what the 3.0 and Swagger 2.0 exports lose, determinism |
| [Diagnostics](diagnostics.md) | Every diagnostic code, its cause and its fix |
| [Architecture](architecture.md) | The pipeline, the modules and the rules contributors follow |

Also: the [decision records](adr/README.md) and the
[migration guide from 0.1](migration-0.2.md).

## Conventions

Examples marked as tested in this guide and in the README are corpus cases
(`tests/corpus/cases/`): the test suite builds them, validates every output
against the official OpenAPI, Swagger, JSON Schema and AsyncAPI schemas and
compares the result with a reviewed snapshot.
