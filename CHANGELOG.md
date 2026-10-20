# Changelog

All notable changes to `oas-forge` will be documented in this file.

## [Unreleased]

### Fixed
- **Unicode Panic in Generics:** Non-ASCII text (e.g. umlauts in doc comments) before a `$Ref` corrupted the output or panicked, because char indices were used as byte offsets. Any such snippet previously failed the whole generation with a YAML error.
- **`@return` Panics:** An unmatched trailing quote (`@return 200: User"` or `@return 200: User "`) no longer panics. Inline JSON schemas without a description (`@return 200: { "type": "string" }`) are no longer mistaken for a description, and an empty type (`@return 200:`) yields a response without body instead of a broken `$ref`.
- **`@security` Panic:** An unclosed scope list (`@security OAuth2(`) no longer panics; `OAuth2(read` no longer drops the last character and `OAuth2()` yields an empty scope list instead of `[""]`.
- **Module Tag Panic:** A `]` before the `[` in a module doc line containing `tags:` no longer panics; empty tag lists (`tags: []`) no longer inject an empty tag.
- **Blueprint Parameter Substitution:** Placeholders are replaced as whole identifiers in a single pass. `$T` no longer rewrites `$Tag` into `$Userag`, and an argument that matches another parameter name is no longer substituted twice.
- **Lifetimes in Wrapper Types:** Lifetime arguments are skipped, so `Cow<'a, str>` maps to `string` instead of `$ref: $Cow`.
- **CLI Default Output:** Without any output option the CLI now writes `openapi.yaml` as documented instead of failing; empty output lists count as unset. The success message lists every written file.
- **Silent Data Loss in the Merger:** A snippet that was not a YAML mapping replaced everything merged before it, so e.g. an empty `/// @openapi` block wiped all schemas. Empty snippets are now skipped.
- **Documentation Above `@openapi`:** Plain doc lines before the first `@openapi` header (such as a summary line on a handler) are no longer emitted as a snippet of their own.
- **Field Names in Tagged Enums:** `#[serde(rename_all)]` on a tagged enum now renames only the variants, as serde does. Variant fields follow the variant's own `rename_all` or the enum's `rename_all_fields`.

### Added
- `-I` as short flag for `--include`, as already shown in the README. The README CLI examples now use the correct binary name `oas-forge`.
- Support for `#[serde(rename_all_fields = "...")]` on tagged enums, and `doc_parser::serde_attr` to read string-valued serde attributes.

### Changed
- **Non-Mapping Snippets Are an Error:** A top-level snippet that is not a YAML mapping (plain text in an `@openapi` block, a YAML file containing only a list, …) now fails generation with its file, line and context instead of silently replacing the merged document. Projects that hit this previously produced an incomplete spec.
- **Test Suite:** All tests moved out of `src/` into four suites under `tests/` (`unit` mirroring `src/`, `integration`, `cli`, `property`). Line coverage is ~99 %.
- Route DSL regexes are compiled once instead of on every `@route`.

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
