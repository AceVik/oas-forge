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
- **Build scripts.** `emit_cargo_directives(true)` also prints
  `cargo:rerun-if-env-changed` for every `{{env:NAME}}` in the inputs;
  `Spec::write_cargo_directives` writes the directives to any writer.
- **New builder options.** `var(name, value)` for `{{name}}` placeholders,
  `deny_warnings(true)`, `max_file_size(bytes)`, `emit_cargo_directives(true)`
  (prints `cargo:warning=` and `cargo:rerun-if-changed=` in build scripts),
  `output_as(kind, path)`, `map_type(name, schema)`, `infer_signatures(bool)`,
  `package(Package)` / `without_package()` and `strict_lints(true)`.
- **CI checks.** `Spec::check()` lists the configured outputs whose files are
  missing or differ from what `Spec::write()` would write.
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
  `$User` inside a sentence stays text. Unknown names are reported (`OF3007`);
  in a `$ref` they still become `#/components/schemas/Name`, so the
  reference is well-formed and the lints report it as dangling (`OF5001`).
  0.1 left `$ref: $Name` in the output.
- **DSL types are smart references.** `@return 200: $User` is resolved like
  `$ref: $User` in YAML: a renamed type is found under its Rust name and an
  unknown name is reported where it is written.
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
- **Lints** (`lints_strict`). Dangling `#/components/...` references
  (`OF5001`), duplicate operationIds (`OF5002`), security requirements that
  name an undefined scheme (`OF5005`) and path placeholders without a path
  parameter or the reverse (`OF5006`) are reported as warnings. Fixtures that
  relied on schemes defined elsewhere now get `OF5005` (`dsl_security`,
  `route_defaults`, `fragment_extend`, `readme_dsl`, …). With
  `strict_lints(true)` (CLI `--strict`) components that nothing outside
  `components` reaches (`OF5003`) and operations and schemas without a
  description (`OF5004`) are reported too.

### Roots, generics and Rust attributes
- **A missing root is generated from `Cargo.toml`** (`package_root`). When no
  source defines `openapi` and `info`, the root is built from the package name,
  version, description, license and homepage or repository (note `OF0004`).
  0.1 failed with `NoRootFound`. Build scripts use the running package by
  default, the CLI reads `Cargo.toml` in the working directory. A root in the
  sources always wins; an `info` block without `openapi` is completed.
  *Action:* `Generator::without_package()` restores the 0.1 error.
- **Generic types are instantiated where they are used** (`auto_generics`,
  `struct_generic_params`). `struct Page<T>` without `@openapi<T>` is a
  template: `Page<User>` as a field, body or return type becomes `Page_User`.
  0.1 emitted `Page` once with dangling `$T` references; an unused generic type
  is no longer emitted at all.
- **`#[deprecated]`** on structs, enums, fields, variants and handlers sets
  `deprecated: true` on the schema or operation (`rust_attributes`).
- **New directives** (`rust_attributes`): `@openapi skip` leaves out an item,
  field or variant; `@example value` adds a YAML value (or plain text) to the
  `examples` of a type, field or variant (named OpenAPI examples of media
  types and parameters are written as embedded YAML). Variants also accept
  `@openapi` overrides; 0.1 ignored them.
- **Types with the same name** in different modules are still merged into one
  schema, now with warning `OF1004`. Rename one of them
  (`#[serde(rename = "AdminUser")]` or `@openapi rename "AdminUser"`).
- **Variables** (`variables`). Every `CARGO_PKG_*` variable Cargo sets for
  build scripts is predefined (`Generator::package` sets them for its package,
  the CLI takes them from `Cargo.toml` in the working directory),
  `{{env:NAME}}` reads the environment (unset:
  warning `OF3009`, the placeholder stays), and `[variables]` in the
  configuration or `--var NAME=VALUE` define more. Placeholders also work in
  type overrides and `@example` lines.

### Correctness review
A review of the finished 0.2 code fixed these cases; most of them also change
what 0.1 wrote:
- **serde names** (`serde_semantics`). An explicit `rename` is kept as
  written even when it equals the field name and `rename_all` is set;
  `rename(deserialize = "…")` alone no longer changes the documented name.
- **Required fields**. A field with `with` or `deserialize_with` is required
  even when it is an `Option`, unless it has `default`: serde rejects a
  missing field there (`serde_with_and_conversions`).
- **Shapes**. A tuple struct with a skipped member stays an array;
  `#[serde(tag = "…")]` on a struct adds the tag property; `serde_repr`
  enums are integers; a flattened map next to flattened structs becomes
  `unevaluatedProperties`, so the values of the other fields are no longer
  constrained by the map's value schema.
- **Date and time without offset**. chrono's `NaiveDateTime` and jiff's
  civil `DateTime` are `format: date-time-local`, `NaiveTime` and the civil
  and `time` times `format: time-local` (`type_mapping`); RFC 3339 formats
  rejected these values.
- **Generics**. Type parameters inside nested generic fields
  (`first: Envelope<T>`, `children: Vec<Tree<T>>`) are bound; 0.1 emitted
  `Envelope_T` with a dangling `T`. Generic type aliases are templates;
  unused ones, such as `type Result<T> = …`, are no longer emitted.
- **Doc comments**. `/** … */` comments are read like rustdoc reads them;
  descriptions keep the line breaks of code blocks, lists and headings;
  free text after `@openapi` (`/// @openapi The id`) is reported (`OF1001`)
  instead of becoming the schema; module tags only come from `tags: [..]`
  lists of names.
- **Fragments**. `@openapi-fragment` blocks in the doc comment of a type are
  registered; `@extend` fragments get variables, escapes and nested
  `@insert`/`@extend` like `@insert` fragments; cycles are reported once
  (`OF3010`) and too many arguments too (`OF3011`). `\$` stays `\$` inside
  `pattern` values, where it is a regular expression escape.
- **Type names**. A documented type named like a built-in type
  (`Duration`, `Value`, `Uuid`, `Url`, …) is referenced wherever the bare
  name is used, in fields as in the route DSL; paths such as
  `std::time::Duration` and mappings from `map_type`/`[types]` keep the
  built-in schema. An `@openapi-type Uuid` schema now replaces the built-in
  mapping for bare uses.
- **Inputs**. A file reached through two inputs (an `include` inside an
  input directory, overlapping inputs) is read once instead of causing a
  false "multiple roots" error; symbolic links in input directories are
  followed, broken ones are reported (`OF0005`).

New warnings can fail builds with `deny_warnings`: `OF0006`, `OF1010`,
`OF1011`, `OF2009`, `OF3012`–`OF3014`, `OF0005`, `OF1001` and
`OF1008` for directive mistakes, `OF1009`, `OF3010`, `OF3011`, and `OF3002`/
`OF3008` for broken templates in included files, which were dropped silently.

### Integration in a real project
Documenting a large service with 0.2 before the release led to these
changes. Those that change 0.1 output:
- **Type aliases are exported only with `@openapi`** (`transparent_aliases`,
  `type_aliases`). 0.1 made every `type X = …` a component. Now an alias
  without `@openapi` has no schema of its own; fields and routes that use
  it get the schema of its target. *Action:* add `/// @openapi` to aliases
  that should stay components.
- **Output types are exact about `required`** (`readme_schemas`,
  `guide_schemas`). In a type that only derives `Serialize`, every field
  serde always writes is required, `Option` fields included (they are
  nullable). `skip_serializing_if = "Option::is_none"` makes a field
  optional and not nullable. Types that derive `Deserialize` keep the 0.1
  rules.
- **Type references resolve by module path** (`module_qualified_names`).
  0.1 matched the last segment of a type name, so two `User` types in
  different crates or modules were confused silently. Now `use`
  declarations and paths decide; an ambiguous name is reported (`OF3014`)
  and left unresolved; two documented types with the same component name
  are named `crate.module.Name` (`OF1004`). *Action:* give such types
  distinct names with `@openapi rename "…"`.
- **Responses are sorted by status**, and several `@return` lines (from the
  block, `@insert` and module defaults) for one status are merged into one
  response with all their descriptions.
- **Flags may follow a quoted description** (`@query-param q: u32 "Text"
  default=1`); 0.1 made them part of the description.

### Command line
- `--check` writes nothing and exits with status 1 when an output is missing or
  out of date; `--deny-warnings` turns warnings into errors; `--strict` enables
  the strict lints; `--message-format json` prints one JSON object per finding.

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

### Documentation
The README was rewritten. Every example of the 0.1 README still works and is
kept as a corpus case (`v01_readme_*`); the new examples are corpus cases as
well (`readme_*`, `guide_*`), so they are validated against the official
schemas on every test run.

### Error messages
- YAML errors point at the offending line instead of the `@openapi` header,
  and the context marks it (`error_invalid_yaml`, `error_non_mapping_snippet`,
  `readme_root`, `readme_dsl`).
- Rust syntax errors include the column (`error_invalid_rust`).
- "Multiple roots" lists every root location (`error_multiple_roots`).
