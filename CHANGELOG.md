# Changelog

All notable changes to `oas-forge` will be documented in this file.

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
