# Migrating from 0.1 to 0.2

0.2 is a rewrite. Everything that worked in 0.1 keeps working unless it is
listed below. Each output change was accepted on purpose; the black-box corpus
(`tests/corpus/`) shows the exact before/after document of every case named
here.

## Installation

| 0.1 | 0.2 |
| :--- | :--- |
| `cargo install oas-forge` | `cargo install oas-forge-cli` (the binary is still called `oas-forge`) |
| `oas-forge = { version = "0.1", default-features = false }` in `[build-dependencies]` | `oas-forge = "0.2"`; the library has no CLI dependencies and no `cli` feature any more |

## Library API

Most build scripts only use `Generator::new().input(..).output(..).generate()`.
That still works unchanged. Everything else moved:

- **Modules.** The 0.1 modules (`scanner`, `visitor`, `preprocessor`,
  `generics`, `index`, `merger`, `type_mapper`, `doc_parser`) are replaced by
  one module per pipeline stage: `source`, `extract`, `template`, `dsl`,
  `merge`, `schema`, `output`, `generator`, `diagnostics`. See ADR 0001 and
  ADR 0008.
- **In-memory results.** `Generator::build()` runs the pipeline without writing
  and returns a `Spec` with the document, the template registry and all
  diagnostics. `Spec::render(kind)` returns any output strategy as a value.
- **New builder options.** `var(name, value)` for `{{name}}` placeholders,
  `deny_warnings(true)`, `max_file_size(bytes)`, `emit_cargo_directives(true)`
  (prints `cargo:warning=` and `cargo:rerun-if-changed=` in build scripts) and
  `output_as(kind, path)`.
- **Errors.** `Error` is `#[non_exhaustive]` and no longer wraps foreign error
  types:
  - `Io(io::Error)` became `Io { path, source }`.
  - `SourceMapped { .. }` and `Parse { file, source }` became `Yaml { location, message, context }`
    and `Parse { location, message }`.
  - `MultipleRootsFound` carries the `locations` of every root.
  - A missing output is `NoOutput` (0.1: an `Io` error of kind `InvalidInput`).
  - New: `Config` for invalid configuration files and `Diagnostics` when
    `generate()` finds error-level diagnostics.
- **Warnings.** Nothing is logged through the `log` crate any more. Findings are
  `Diagnostic`s with a stable code (`OF2002`), a severity and a source location;
  the CLI prints them, build scripts get them as `cargo:warning=` lines with
  `emit_cargo_directives(true)`.

## Behaviour changes

### Written files
- Files are written atomically and only when their content changes, so an
  unchanged spec no longer triggers rebuilds of crates that watch it.
- Configured output files are never read back as input (0.1 read an
  `openapi.yaml` inside an input directory on the next run and reported a
  second root).

### Route DSL
- **Rejected routes are errors** (`dsl_rejected_routes`). A route with an
  undeclared or unused path parameter, or without method and path, is reported
  as `OF2001`–`OF2003` with its location and `generate()` fails. 0.1 logged the
  problem and emitted the raw DSL text, which then failed as invalid YAML.
- **Stable operationIds for file-level routes** (`dsl_virtual_routes`). Routes
  in `//!` blocks are named after method and path (`get_legacy_proxy`) instead
  of their line number (`virtual_route_3`). Several routes in one block and a
  route after the root block are now separate operations.
  *Action:* update clients that use the old generated operationIds.
- **Description paragraphs** (`dsl_basic`). A blank doc line between paragraphs
  is kept as a blank line in `description`, so Markdown paragraphs render.
- **`@route` on `impl` methods** (`dsl_impl_methods`) and **module tags on
  `@route` handlers** (`dsl_module_tags`) work.
- **`@body $Vec<T>`** (`dsl_vec_shorthand_body`) is an array of `T` instead of a
  bogus media type.
- **`@extend` inside routes** (`dsl_extend_in_route`) is applied.
- **YAML `parameters` in a route are merged** (`dsl_yaml_overrides`) with the
  DSL parameters by `name` and `in`; 0.1 replaced the DSL parameters.
- **Embedded YAML must be valid** (`readme_dsl`). An embedded block with a
  duplicate key, or one that is not a mapping, is reported (`OF2005`) and
  ignored. 0.1 accepted duplicate keys silently.
- Unknown directives (`@frobnicate`) are reported (`OF2006`) instead of being
  ignored silently.

### Templates and references
- **`@extend`: local values win** (`fragment_extend`). Values written next to
  `@extend` override the fragment, like inheritance. 0.1 let the fragment win.
- **Smart references only replace whole values** (`smart_references`,
  `unicode_docs`). `$ref: $User` still becomes `#/components/schemas/User`, but
  `$User` inside a sentence stays text. Unknown names are reported (`OF3007`).
- **Renamed types are found under their Rust name** (`struct_renamed_reference`):
  a field of type `Profile` points to `UserProfile` when `Profile` is renamed.
- **Unquoted placeholders stay strings** (`readme_root_variables`,
  `version_placeholder`). `version: {{CARGO_PKG_VERSION}}` is quoted before
  parsing, so a version such as `1.10` is no longer turned into the number 1.1.
- Missing fragment arguments (`fragment_insert`), unknown blueprints
  (`blueprint_generics`) and invalid `@openapi` overrides are reported
  (`OF3003`, `OF3004`, `OF1001`); the output is unchanged.

### Extraction
- **Only lines that start with `@openapi` export an item**
  (`export_trigger_in_prose`). A doc comment that mentions `@openapi` in a
  sentence no longer exports the struct.
- **A root document on a unit struct** (`root_unit_struct`) is the root instead
  of a schema named after the struct.
- **Separator lines end file-level blocks** (`readme_fragments`). A line such as
  `//! ---` or a gap in the `//!` comment closes the current `@openapi…` block,
  so prose after a fragment is no longer appended to its body.

### Output
- **Fragments carry a format marker** (all `fragments` outputs).
  `components.x-oas-forge-format: 1` versions the cross-crate template format
  (ADR 0005); template bodies keep the field order of the Rust source. Fragment
  files written by 0.1 (without the marker) are still read.
- **Top-level `tags` are tag objects** (`dsl_module_tags`): `tags: [Admin]`
  becomes `tags: [{name: Admin}]`, as the OpenAPI schema requires.
- **Lints.** Dangling `#/components/...` references (`OF5001`) and duplicate
  operationIds (`OF5002`) are reported as warnings.

### Error messages
- Locations point at the YAML text instead of the `@openapi` header line, so
  the reported line is the first YAML line (`error_invalid_yaml`,
  `error_non_mapping_snippet`, `readme_root`).
- Rust syntax errors include the column (`error_invalid_rust`).
- "Multiple roots" lists every root location (`error_multiple_roots`).
