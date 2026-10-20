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

Rust 1.89 or newer is required (0.1 declared no minimum; edition 2024 needs 1.85).

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

### Schemas follow serde
Reflected schemas now describe exactly what serde writes (ADR 0002). The
changes in every corpus snapshot come from the first three entries.

- **`Option<T>` is nullable** (`struct_basic`, `readme_schemas`, …). Besides
  being optional it accepts `null`: `type: [string, "null"]`, or
  `anyOf: [{$ref}, {type: "null"}]` for references. For an `openapi: 3.0.x`
  root this is written as `nullable: true` (`root_openapi_30`).
  Query, header and path parameters only become optional.
- **Integer ranges** (`type_mapping`). Unsigned types have `minimum: 0`;
  `u32` is `int64` (its values do not fit `int32`); `i8`, `i16`, `u8`, `u16`
  and `u32` carry their `minimum`/`maximum`; `i128`/`u128` have no format.
  *Action:* clients generated from the spec may use wider integer types.
- **Data-carrying enum variants are documented** (`enum_externally_tagged`,
  `enum_untagged`). Externally tagged enums with data become `oneOf` of a
  string enum and single-key objects, untagged enums `anyOf` of the variant
  shapes; 0.1 dropped data variants. Internally tagged newtype variants carry
  the fields of their struct via `allOf` (`enum_internally_tagged`).
- **Struct shapes** (`struct_tuple_and_unit`): newtype structs are their
  field's schema, tuple structs and tuples fixed-length arrays (`prefixItems`),
  unit structs and `()` are `null`. A unit struct whose `@openapi` block
  defines a schema keeps working as a virtual type.
- **serde attributes** (`struct_serde_attributes`,
  `serde_with_and_conversions`, `serde_enums_mixed`): `skip`, `default`,
  `skip_serializing_if`, `skip_serializing` (`writeOnly`),
  `skip_deserializing` (`readOnly`), `flatten` (`allOf`, maps become
  `additionalProperties`), `deny_unknown_fields`, `transparent`,
  `into`/`from`/`try_from`, known `with` modules, `rename(serialize = ..)`,
  per-variant `untagged`, `rename_all_fields`, raw identifiers (`r#type` is
  `type`) and attributes behind `cfg_attr`.
- **Casing matches serde exactly.** Field and variant names use serde's two
  rules (`rename_all = "snake_case"` leaves a field called `lastName`
  unchanged, like serde).
- **More types** (`type_mapping`): sets have `uniqueItems`, `[T; N]` has
  `minItems`/`maxItems`, `char` is one character, maps with integer keys
  restrict `propertyNames`, and `VecDeque`, `BTreeSet`, `IndexMap`,
  `NonZero*`, `IpAddr`, `Duration`, `SystemTime`, `PathBuf`, `Bytes` and
  the `jiff`/`time` types are mapped. `time` types assume
  `time::serde::rfc3339` (time's default serde format is an array).
  *Action:* map other formats with `Generator::map_type` or `[types]` in
  `openapi.toml`.
- **Validation** (`struct_validation`, `garde_validation`): `validator`
  `length` is applied by type (`minItems` on `Vec`, `minProperties` on maps),
  `range` accepts negative, float and exclusive bounds, `equal`, `contains`,
  `does_not_contain`, `ip`, `required` and `regex(path = ..)` are supported;
  `garde` is supported, including its length modes. Constraints whose value
  is a constant are reported (`OF1006`) instead of silently dropped.

### Route DSL 2.0
Every 0.1 route block still compiles; these outputs changed:

- **Smaller operations.** Empty `tags: []` and `parameters: []` are no longer
  written (all DSL snapshots).
- **Valid responses.** A response without description gets the status code's
  reason phrase (`404` → `Not Found`) instead of an empty string, and an
  operation without any `@return` gets a `default` response, because OpenAPI
  requires at least one (`readme_dsl`).
- **Bodies are required** unless written `@body optional T` or typed
  `Option<T>`.
- **Framework wrappers.** `Json<T>` and `Form<T>` are documented as `T`
  (`dsl_responses`).
- **Signature inference** (`signature_inference`). Handlers using axum or
  actix-web extractors (`Path`, `Query`, `Json`, `Form`, `Multipart`) and
  returning `Json<T>` get the missing path parameters, query parameters, body
  and success response. Explicit DSL and YAML always win.
  *Action:* `@no-infer` on a route or `Generator::infer_signatures(false)`
  switches it off.
- **More YAML keys.** Besides `parameters`, `requestBody`, `responses`,
  `security`, `externalDocs`, `callbacks`, `servers` and `x-…`, lines
  starting with `tags:`, `deprecated:` or `operationId:` now begin embedded
  YAML. *Action:* move such prose into `@description`.
- **Methods.** Methods other than the nine standard ones (including `QUERY`)
  go to `additionalOperations` (OpenAPI 3.2) instead of an invalid path item
  key; method names must be HTTP tokens. Derived operationIds use the
  lowercase method.
- **`@security none`** alone means `security: []`; next to other `@security`
  lines it adds the anonymous alternative `{}` (optional authentication).
- **Version limits.** With an `openapi: 3.0.x` or `3.1.x` root, 3.2-only
  constructs (`QUERY`, `additionalOperations`, `querystring` parameters,
  streamed `itemSchema` responses) are removed with a warning; with 3.0, also
  `webhooks` (`root_openapi_31_drops_32`).
- Struct expansion (`@query-params $Pagination`) only works for structs
  documented in the same run; a struct from another crate's fragments file
  is not expanded (warning `OF2008`).

### Output formats
- **Fragments stay canonical.** The fragments output always uses the JSON
  Schema 2020-12 dialect, even when the crate's own root is `openapi: 3.0.x`;
  the consuming crate lowers them for its root.
- **AsyncAPI is a separate document.** YAML files and doc blocks with an
  `asyncapi:` root, `@asyncapi` blocks and `@channel` blocks go to the
  AsyncAPI output (`output_asyncapi`), not into the OpenAPI document. 0.1
  merged an `asyncapi` YAML file into the OpenAPI specification.
- New outputs: `output_json_schema` (a JSON Schema 2020-12 bundle),
  `output_openapi_30`, `output_swagger2` and `output_asyncapi`. What a target
  cannot express is reported (`OF6001` notes, `OF6002` warnings) when the
  output is configured; `Spec::render_with_diagnostics` returns the findings
  for any output.

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
- `{"$ref": "$String"}` and other built-in type names (from blueprints such
  as `$Page<String>`) become the type's schema instead of a dangling reference.
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

### YAML
The YAML library changed from `serde_yaml_ng` to `serde-saphyr` (ADR 0009). The
meaning of documents is the same, with these exceptions:
- Merge keys (`<<: *base`) are expanded. 0.1 kept `<<` as an ordinary key.
- A `null` key (`~: x`) and complex keys (`? [a, b]`) are errors. 0.1 turned
  them into text.
- Documents with more than about 250 000 nodes after alias expansion or more
  than 256 nesting levels are rejected.
- Written YAML quotes version-like strings (`openapi: "3.1.0"`) and status codes
  (`"200":`) with double quotes. The values are unchanged.

### Error messages
- YAML errors point at the offending line instead of the `@openapi` header,
  and the context marks it (`error_invalid_yaml`, `error_non_mapping_snippet`,
  `readme_root`, `readme_dsl`).
- Rust syntax errors include the column (`error_invalid_rust`).
- "Multiple roots" lists every root location (`error_multiple_roots`).
