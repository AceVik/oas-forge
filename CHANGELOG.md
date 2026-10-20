# Changelog

All notable changes to `oas-forge` will be documented in this file.

## [0.2.0] — Unreleased

A rewrite of the generator around explicit pipeline stages, with full serde,
validator and garde support, OpenAPI 3.2, a route DSL that embeds any OpenAPI
YAML, new output formats and diagnostics with stable codes. Everything that
worked in 0.1 keeps working unless the
[migration guide](docs/migration-0.2.md) lists it.

### Added
- **Schemas that match serde:** `rename`/`rename_all` (also
  `serialize`/`deserialize` forms and `rename_all_fields`), `flatten` for
  structs and maps, `skip`, `skip_serializing` (`writeOnly`),
  `skip_deserializing` (`readOnly`), `skip_serializing_if`, `default`,
  `transparent`, `deny_unknown_fields`, `into`/`from`, known `with` modules,
  `cfg_attr`, and all four enum representations including untagged variants.
- **validator and garde:** `length`, `range` (floats, negatives, exclusive
  bounds), `email`, `url`, `ip`, `contains`, `does_not_contain`, `regex`
  (resolved from `static`/`LazyLock` constants), `required`, garde length
  modes and `inner(…)`.
- **Type table:** integer ranges, `NonZero*`, `i128`/`u128`, `char`, sets
  with `uniqueItems`, arrays `[T; N]`, tuples, `Duration`, `SystemTime`,
  `chrono`, `time`, `jiff`, `uuid`, `ulid`, `url`, decimals, `bytes`, IP
  addresses, `IndexMap`/`IndexSet`; `Generator::map_type` and `[types]` for
  custom mappings.
- **Route DSL 2.0:** any Operation Object field as embedded YAML,
  `@openapi operation|path|document` scopes, `@param <in>`, struct expansion
  (`@query-params $Paging`), parameter flags (`default`, `enum`, `min`,
  `max`, `pattern`, `format`, `style`, `explode`, …), `@body` with several
  media types and `optional`, `@return` with ranges, `default`, media types
  and `stream<T>`, `@return-header`, `@problem` (RFC 9457), `@summary`,
  `@description`, `@deprecated`, `@operation-id`, `@external-docs`,
  `@server`, `@security a + b` and `@security none`, `@extend` in routes.
- **OpenAPI 3.2:** `QUERY`, `additionalOperations`, `querystring`
  parameters, streamed responses (`itemSchema`). OpenAPI 3.1 webhooks with
  `@webhook`.
- **Module defaults:** `@route-defaults` with `@prefix`, parameters, tags,
  security and responses for every route in a module.
- **Signature inference** for axum and actix-web handlers (`Path`, `Query`,
  `Json`, `Form`, `Multipart`, `Json<T>` return types); `@no-infer`.
- **Outputs:** OpenAPI 3.0.3 and Swagger 2.0 exports, a JSON Schema 2020-12
  bundle and AsyncAPI 3 (`@asyncapi` blocks and the `@channel` DSL with
  `@send`/`@receive`). What a target cannot express is reported.
- **Root from `Cargo.toml`:** without an `openapi`/`info` root, the root is
  generated from the package metadata (`Package`).
- **Generics without `@openapi<T>`:** generic structs and enums are
  instantiated where they are used (`Page<User>` → `Page_User`).
- **Directives:** `@openapi skip`, `@example`, `#[deprecated]` on types,
  fields, variants and handlers; overrides on enum variants.
- **Variables:** all `CARGO_PKG_*`, `{{env:NAME}}`, `Generator::var`,
  `[variables]`, `--var`; `cargo:rerun-if-env-changed` for every
  environment variable used.
- **Diagnostics** with stable codes (`OF0001`–`OF6002`), severities and
  source locations; `Diagnostic::to_json`.
- **Lints:** dangling references, duplicate operationIds, undefined
  security schemes, path parameter mismatches; strict lints for unused
  components and missing descriptions.
- **Library API:** `Generator::build` returns a `Spec` (document,
  diagnostics, `render`, `write`, `check`, `write_cargo_directives`); every
  pipeline stage is a public module.
- **CLI:** `--check`, `--deny-warnings`, `--strict`,
  `--message-format json`, `--var`, the export flags; the package metadata
  of `Cargo.toml` is used.
- Files are written atomically and only when they change.

### Changed
- The CLI is a separate crate: `cargo install oas-forge-cli` (the binary is
  still `oas-forge`). The library has no CLI dependencies and no `cli`
  feature.
- Rust 1.89 or newer is required.
- Output changes where 0.1 was wrong: unsigned integers have `minimum: 0`
  and `u32` is `int64`; `Option` fields are nullable; rejected routes fail
  the run; `@extend` lets local values win; smart references only replace
  whole values; unknown references are pointers that the lints report;
  top-level `tags` are tag objects. The [migration guide](docs/migration-0.2.md)
  lists every change with its corpus case.
- Findings are diagnostics instead of `log` messages.
- `Error` is `#[non_exhaustive]` and carries locations instead of foreign
  error types.

### Fixed
- Non-ASCII text before a `$Ref` no longer corrupts the output or panics.
- `@return`, `@security` and module tag lines with unbalanced quotes,
  brackets or parentheses no longer panic.
- Blueprint parameters are substituted as whole identifiers (`$T` no longer
  rewrites `$Tag`).
- `Cow<'a, str>` and other types with lifetimes map correctly.
- A snippet that is not a mapping no longer replaces everything merged
  before it; it is an error with its location.
- Doc lines before `@openapi` are no longer emitted as a separate snippet.
- `rename_all` on tagged enums renames only the variants, as serde does.
- Generated output files are no longer read back as inputs.
- Unsigned integers, `Option` fields, `$Vec<T>` bodies, `@return` of
  generics with several arguments (`$Result<A, B>`) and nested generic
  arguments are documented correctly.
- A final line break of block scalars survives variable substitution.

### Security
- `serde_yaml_ng` (and the unmaintained `unsafe-libyaml`, RUSTSEC-2024-0449)
  replaced by `serde-saphyr`, a pure Rust YAML library; documents are size-
  and depth-limited.
- `anyhow` advisory RUSTSEC-2026-0190 resolved; `cargo deny` checks
  advisories, licenses, bans and sources in CI.
- Input files are size-limited (16 MiB by default), nesting of fragments and
  generics is bounded, and `#![forbid(unsafe_code)]` holds for both crates.

## [0.1.4] — 2026-03-13

### Added
- **Cross-Crate Template Transport:** Schemas, blueprints and fragments are now serialized into `x-oas-forge-templates` / `x-oas-forge-fragments` vendor extensions inside fragment output (`output_fragments`). Downstream crates can `.include()` these files — the registry is automatically hydrated and vendor keys are stripped from the final output.
- **TDD Examples Workspace:** `examples/` directory with `shared-models` and `microservice` crates demonstrating cross-crate generic template transport (`Paginated<T>` → `Paginated_User`).
- Imported YAML schemas are now registered for smart reference resolution (`$SchemaName` → `$ref`).
- `Blueprint` and `Fragment` types now implement `Serialize` / `Deserialize`.

### Fixed
- **Description Slurping:** Multi-line `@openapi` YAML payloads were incorrectly collected as the schema `description`. A state flag now stops description collection once the `@openapi` block begins.
- **Generics Panic:** Unbalanced generic brackets (e.g. `$Foo<Bar`) no longer cause a slice-out-of-bounds panic; the raw text is emitted instead.
- **Conditional Compilation:** `clap::Parser` derive and CLI attributes are now gated behind `#[cfg(feature = "cli")]`. The crate compiles cleanly with `default-features = false`.
- **DSL Validation Panics:** Missing or unused path parameters in `@route` definitions now log an error and skip the route instead of panicking.
- **DSL Inline Schema Parsing:** `@return` with inline YAML/JSON objects (`{ type: array, ... }`) now parses correctly via `serde_yaml_ng` instead of `serde_json`.
- **Vendor Extension Leaking:** `x-oas-forge-*` keys are stripped from the merged output before any file write strategy, preventing accidental leakage into full specs or schema-only outputs.

### Changed
- **`serde_yaml` → `serde_yaml_ng`:** Replaced the deprecated `serde_yaml` crate with the maintained fork `serde_yaml_ng` (v0.10).
- `scan_directories` now returns `(Vec<Snippet>, Registry)` to support template transport.

## [0.1.3] — 2026-01-15

### Added
- Adjacently tagged enum support (`#[serde(tag = "type")]`) with `oneOf` / `discriminator` generation.
- Validation attribute extraction (`#[validate(...)]`) for OpenAPI constraints (`minLength`, `maxLength`, `format: email`, etc.).
- Enum variant validation for both struct and tuple variants.
- `rename_all` / `rename` support for structs and enums via `@openapi` and `#[serde]`.

## [0.1.2] — 2026-01-07

### Added
- Route DSL (`@route`, `@return`, `@body`, `@query-param`, `@security`, `@tag`).
- Fragment system (`@openapi-fragment`, `@insert`, `@extend`).
- Generic blueprint system (`@openapi<T>`) with monomorphization.
- Smart `$ref` substitution.
- Multiple output strategies (full spec, schemas-only, paths-only, fragments).

## [0.1.1] — 2026-01-05

### Fixed
- Description parsing cleanup (leading whitespace trimming).

## [0.1.0] — 2026-01-04

### Added
- Initial release: AST-based OpenAPI 3.1 generation from Rust doc comments.
