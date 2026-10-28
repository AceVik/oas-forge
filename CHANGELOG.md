# Changelog

All notable changes to `oas-forge` will be documented in this file.

## [Unreleased]

### Changed
- Refresh locked dependencies in the main and example workspaces, including
  `annotate-snippets` 0.12.17, `encoding_rs` 0.8.42, `indexmap` 2.14.2 and the
  ICU crates 2.4. Rust 1.89 remains supported.
- Example crates declare their minimum Rust version, MIT license and
  dependency version bounds so they also pass the repository's dependency
  policy checks.

### Fixed
- Combined `@security` requirements retain all scopes when a scheme is
  repeated. Plus signs inside scopes and commas inside quoted scopes no
  longer split requirements or scope names.
- Output writes use exclusively created, unique temporary files, preventing
  symlink attacks and collisions between concurrent writers. Replacing a Unix
  output no longer widens its file permissions. On Windows, which refuses to
  replace a file while another writer replaces it, the replacement is
  retried for about a second.
- Generic reference parsing rejects more than 128 nested generic lists before
  exhausting the stack. Malformed generic references report error `OF3015`.
- Fragment inserts and extensions have total work and text budgets, preventing
  exponential expansion even without cycles. Oversized substituted fragment
  bodies are rejected before allocation (error `OF3016`). The expansions
  of a run also share 32 MiB of pasted text (inserted fragment lines,
  `@extend` bodies and the copies of `@route-defaults` in each route):
  each block stayed within its 16 MiB, but a hundred blocks inserting one
  large fragment, or 1500 routes under 1500 lines of defaults, filled
  gigabytes. Text written in the inputs does not count. The copy of a
  `.proto` file in each `proto(...)` message counts too; 40 messages of
  one 8 MiB file took 1.8 GB. So do the copies of a block and of its
  module defaults for its `@route` lines after the first (a block is
  compiled once per line): 500 lines over 500 `@param` lines took 2 GB,
  400 lines under 4000 lines of defaults 9.5 GB; the lines that do not
  fit are left out with one finding. Each pasted or copied line costs 256 bytes besides
  its text (not the `.proto` copies, which stay one string): a short line
  such as `@param query q: u8` becomes kilobytes of document, and 32 MiB of
  them, pasted by a 150 KB input, took 11 GB. A route or channel block that the expansion cut
  (at 65 536 lines, or because a fragment did not fit) is left out with
  its finding; before, the lines left were compiled as an operation of
  part of the block.
- Channel blocks that define one message name differently are compared
  through a set; 10 000 of them took a minute. So are the messages of one
  block (20 000 `@send` lines of one name took minutes), the messages its
  operations refer to, the lines of its `@openapi message` sections
  (grouped once; 40 000 messages with a section each took over a minute)
  and its `@ws-query` and `@ws-header` names (a later declaration still
  replaces the earlier one in its place; 100 000 took a minute).
- Variable substitution is size limited and preserves newlines and control
  characters in whole YAML string values. Multiline prose and subsequent
  YAML lines keep their original source locations.
- A finding with line breaks, for example one quoting a YAML string, is
  printed as one `cargo:warning=` per line. Before, its second line reached
  Cargo as a directive of its own, such as `cargo:rustc-cfg=…`.
- Findings, locations and errors display control characters and
  bidirectional formatting characters escaped (`\n`, `\u{1b}`, …; new
  function `diagnostics::printable`), also in the CLI and in the context of
  a YAML error. Before, text from the inputs could move the cursor, recolor
  or overwrite a terminal line, or start a line that looked like another
  finding. `Diagnostic::message` and the JSON output keep the text as it is.
  Configuration errors keep their line breaks.
- A symbolic link in an input directory that leads outside every input
  directory is skipped with the new warning `OF0008`. Before, a link such
  as `src/x.yaml -> ~/.config/gh/hosts.yml` merged that file into the
  document. Links into any input are still followed; add a link's target
  as an input or include to read it.
- An output path that leads to a FIFO, a device or another non-regular
  file is replaced without being read, and of a regular file no more is
  read than a current output can hold. Before, a committed link
  `openapi.yaml -> /dev/zero` filled the memory and one to a FIFO blocked
  the run.
- Indexing a `.proto` file checks its messages for duplicates in
  logarithmic time; a file with a few hundred thousand messages took
  minutes before.
- A chain of more than 64 type aliases is reported (new warning `OF3017`)
  and not resolved. Each link was resolved by a recursive call, so a chain
  of 20 000 aliases overflowed the stack and aborted the run.
- Generic instantiation has a total size budget of 16 MiB for the concrete
  schemas of a run (new error `OF3018`). A blueprint whose instances refer
  to two larger instances of itself grew exponentially within the depth
  limit and ran until the memory was exhausted; each body is measured
  before it is built. The targets of replaced type aliases use the same
  budget (in the generics and the references stage): a chain of aliases
  whose target names the next alias twice, such as
  `type T0<X> = (T1<X>, T1<X>);`, doubled with every link, so 20 links ran
  for minutes and wrote 700 MB. The budget counted text, but memory grows
  per value: each value of a concrete schema or alias target now costs 32
  bytes besides its text and each instance 256, and the inline schemas of
  std containers (`$Vec<…>`) and the references wrapped into `{ $ref: … }`
  while an instance is built count too. A template of 1000 properties
  `{ $ref: '$T' }` used with 400 arguments `Vec<Vec<…<u8>>>` of growing
  depth (68 KB) took 4.9 GB before the budget stopped it, now 320 MB; one
  of 4000 properties used with 400 own types (172 KB) 2.3 GB, now 470 MB.
  A template whose body names two more instances of itself
  (`$Two<Vec<T>>`, `$Two<Option<T>>`; 483 bytes) created 30 000 small
  instances within the depth limit and took 1.6 GB, now 290 MB. About 220 000
  properties `{ $ref: … }` across all instances fit; a template of 2000 of
  them used with 200 types (88 KB, 22 MB of output) now exceeds the budget
  after 109 instances.
- Rust sources are parsed on a thread with a 256 MiB stack (32 MiB on
  32-bit targets) in `Generator::build`, `generate_from_sources` and
  `extract::extract`. `syn` recurses once per nesting level, so about 150
  nested types overflowed the 8 MiB main thread of a build script (about
  20 on Windows, whose main thread has 1 MiB) and aborted the process.
  `TypeExpr::parse` returns `None` for types nested more than 32 levels.
- An annotation that adds entries to a `security` or `servers` list that
  another annotation set (at the root, on a path or an operation), or adds
  such a list to an operation or path that another annotation defined, is
  reported (`OF4001`). The lists are joined, or the operation's own list
  replaces the document's, so a second `security: [{}]` made
  authentication optional without a finding. So is an annotation that adds
  to an operation or path whose `security` or `servers` another annotation
  set, so the order of the files does not matter, and one that adds a key
  to a security scheme another annotation defined, such as an OAuth2 flow
  with another authorization URL.
- A `security` list written in a route or next to `@extend` wins over the
  one of a fragment or of the YAML of `@route-defaults`, as documented for
  defaults. The lists were joined, so `security: [{}]` in a shared fragment
  or module defaults made every route that used it public.
- Merging annotations takes time in proportion to each annotation: the
  items of a list were serialized again for every annotation that added to
  it, and operation parameters were matched by scanning the list, so a few
  thousand annotations adding to one list took minutes. Top-level tags are
  merged by name through an index; 40 000 tags took a minute. Struct
  expansion (`@query-params $Struct`, inferred `Query<T>`) checks
  duplicates and required fields through sets; a struct with 10 000 fields
  took a minute and a half. The parameters struct expansion adds to a
  document share 16 MiB (each costs 256 besides its JSON text), since every
  operation copies the struct's fields: 40 000 fields for 20 routes took
  4.8 GB; an expansion that cannot fit is reported without copying a
  field, and the fields of each struct are counted once (once the budget
  was used up, 200 routes expanding 20 000 fields still copied and
  serialized them all, for a minute, and 2000 routes walked them all); the
  walks of a document visit at most 4 194 304 schemas, properties and
  required names, since a struct that many structs flatten was walked for
  each of them; and each
  referenced schema is followed once, since a schema whose `allOf` refers
  to itself 60 times took two minutes. Findings about type aliases (ambiguous names,
  cycles, chains) are reported once through a set; each was compared with
  all findings so far, so 40 000 uses of an ambiguous alias took minutes.
  The lists a route block builds are indexed as well: `@param` lines (a
  later declaration of a name replaces the earlier one in its place; 20 000
  took two minutes), the parameters of `@route-defaults` YAML a route
  declares again, `@security` requirements and their scopes (a repeated
  requirement is one whichever order its schemes have), `@tag` and `@scope`
  names (100 000 names took a minute), the placeholders of a path, and the
  findings a block with several `@route` lines or the routes of a module
  report once (2000 invalid default lines under 2000 routes took forty
  seconds). Scope pruning finds the parents of used tags with one lookup
  per name; going through all tags again for each level took 15 s for a
  chain of 10 000 tags.
- The Swagger 2.0 export reports what it changed silently: `writeOnly`
  kept as `x-writeOnly` (`OF6001`), and security schemes without `type`,
  OAuth2 schemes without `flows` or a known flow, which it leaves out.
  Security requirements it removes because they name such schemes are a
  warning (`OF6002`) instead of a note.
- A scope whose document has no root because the root of the sources is
  scoped elsewhere is reported (new warning `OF4002`): the document lost
  the root's global `security` and `servers`, and only a dropped note said
  so, although the line that caused it does not touch `security`.
- A `Cargo.toml` next to a scanned `src` directory that cannot be read or
  has no `[package] name` is reported (new warning `OF0007`) instead of
  silently naming the crate after its directory.
- `mod` declarations are followed as rustc follows them: `#[path]` inside
  inline `mod` blocks and on them, and files declared by a file that
  `#[path]` moved, get the module path rustc gives them (it decides the
  `crate.module.Name` of colliding types and which type a path means); a
  file that `#[path]` names is read like a `mod.rs`, so its modules are
  next to it.
- Two documented generic types of one name in different modules were merged
  silently (every `Page<T>` used the last one). Templates are now qualified
  like other types (`crate.module.Page`, instances `crate.module.Page_User`,
  each reported as `OF1004`), and a generic type resolves its template by
  module path like any other type; a template and a schema of one name do
  not collide.
- A type argument (or template) that does not name one documented type,
  such as `Page<User>` with two documented `User`s, is reported (`OF3014`)
  and nothing is instantiated, instead of an instance that pointed to a
  schema that does not exist.
- A type alias without `@openapi` used as a type argument stands for its
  target: `Page<Owner>` with `type Owner = User;` is the instance
  `Page_User` (it was a second, identical `Page_Owner`), and
  `Page<List<User>>` with `type List<T> = Vec<T>;` is `Page_Vec_User`.
  `AliasItem` has the new field `target`.
- A field whose type is a type alias of an `Option` (`type MaybeId =
  Option<u64>;`, generic ones and aliases of aliases too) is optional like
  an `Option` field: it was nullable but required. Serialize-only types and
  `skip_serializing_if = "Option::is_none"` follow the `Option` rules, and
  DSL parameters of such a type are optional.
- `@openapi document` (and `path`/`operation`) blocks on types keep the
  indentation of their YAML: a block with two top-level keys was a YAML
  error, because only its first line lost the doc comment's margin.

### Security
- `{{env:NAME}}` placeholders read only the environment variables you allow
  with `Generator::allow_env`, `allow_env = [...]` in the configuration or
  `--allow-env NAME` (all new). Before, the sources chose the name, so a
  source from an untrusted change (a pull request built in CI) could copy
  any secret of the build environment, such as `GITHUB_TOKEN`, into the
  document or a warning. Other `{{env:…}}` placeholders are kept, quoted
  where they are a whole YAML value, and reported (`OF3009`);
  `cargo:rerun-if-env-changed` is printed for allowed names only. **If you
  use `{{env:…}}`, allow its names.** `generate_from_sources` and
  `lexical::substitute` take `{{env:NAME}}` from the variable `env:NAME`.

## [0.2.0] — 2026-10-21

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
- **Scopes:** `@scope` on routes, webhooks, channels, module defaults and
  YAML blocks; `Generator::output_scoped` writes several documents (e.g.
  public, admin, a second service) from one run, each with its own root and
  only the components it uses (`Spec::render_scoped`, `--scope`).
- **WebSocket channels:** `@ws-query` / `@ws-header` (handshake bindings),
  embedded YAML in `@channel` blocks (`@openapi message Name`, `operation`,
  `document`) and a media type per message.
- **protobuf payloads:** `.proto` files are inputs; `proto(pkg.Message)`
  gives an AsyncAPI multi-format payload with the message's `.proto` text.
- **Route blocks:** several `@route`/`@webhook` lines in one block;
  `@description` replaces the prose, `@summary` sets the summary and leaves
  all prose to the description, `@internal` keeps developer notes out of
  the document; the same directives on types, fields and variants
  (`@summary` is the schema `title`); `optional` flag for parameters;
  `required`/`optional` for response headers (optional by default).
- **rustdoc-friendly annotations:** directive arguments may be wrapped in
  backticks, URLs may be autolinks (`<https://…>`), and YAML may sit in
  fenced code blocks, so annotated crates pass `clippy::doc_markdown`.
- **Type names resolve by module path:** `use` declarations, module paths
  and crates decide which documented type a field refers to; ambiguous
  names are reported (OF3014) instead of guessed, and colliding component
  names are qualified (`crate.module.Name`).
- `@route-defaults` in module files (`//!` at the top, or `///` on
  `mod name;`); the `#[default]` variant of an enum becomes its `default`
  and that of `#[serde(default)]` fields of its type.
- **API documentation:** every public item has a running example (100 %
  of the items rustdoc counts), fallible functions document their errors,
  and every internal item is documented; lints keep it that way.
- Files are written atomically and only when they change; line endings are
  ignored when comparing, so CRLF checkouts are not outdated.
- **Release artifacts:** prebuilt CLI binaries for Linux, macOS and Windows
  with `SHA256SUMS`, build provenance attestations and the licenses of all
  compiled-in crates; `cargo binstall oas-forge-cli`.

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
- Type aliases without `@openapi` are no longer components; references to
  them resolve to their target.
- Output types (deriving `Serialize` only) list every field serde always
  writes as `required`, also `Option` fields (nullable); a field with
  `skip_serializing_if = "Option::is_none"` is optional and not nullable.
- Responses are sorted by status; several lines for one status are one
  response whose descriptions are combined.
- Lint findings (`OF5xxx`) carry the file and line of the part they concern,
  which changes their `cargo:warning` and `--message-format json` lines.
- `cargo:rerun-if-changed` also covers the package manifest and missing
  include paths (through their nearest existing directory).
- Exports: lowering below 3.2 also cleans components and callbacks;
  `contentEncoding: base64` becomes `format: byte` and binary
  `contentMediaType` `format: binary` in 3.0 and Swagger 2.0 (a binary
  multipart part is a Swagger `file` parameter); AsyncAPI message headers
  are lowered like payloads.
- Regex constants for `regex(path = …)` resolve by path; constants of the
  same name with different patterns are reported (`OF1011`).
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
- Found in a review of the 0.2 code, each with a test or corpus case (the
  [migration guide](docs/migration-0.2.md#correctness-review) lists the
  output changes):
  - serde: explicit renames, deserialize-only renames, required `with`
    fields, tuple structs with skipped members, struct tags, `serde_repr`,
    flattened maps next to flattened structs, closed struct variants,
    generic tagged enums and generic type aliases;
  - nested generic parameters; offset-free date and time formats;
    documented types win over built-in types of the same name;
    `@openapi<T>` on type aliases;
  - Javadoc comments, Markdown line breaks, fragments in item doc comments,
    module tag lists, free text after `@openapi`;
  - route DSL: `default` responses and single query parameters no longer
    stop inference, nested module security and tags replace the outer ones
    (security was widened to "outer or inner"), defaults support variables,
    fragments, YAML and inline prefix parameters and are expanded once per
    module (one finding, at its line), paths are joined with one
    `/`, invalid inline types are reported, findings after `@insert` point at
    their line;
  - fragments: `@extend` runs the full text stage, cycles and extra arguments
    are reported, `\$` in regular expressions is kept;
  - outputs: unique Swagger `produces`, no dangling Swagger references,
    `paths: {}` where a version requires it, fragments without the generated
    root, sorted schema outputs, tags with the same name merged;
  - generator: overlapping inputs, symbolic links, findings of included
    templates, watched configuration files, workspace-inherited package
    fields in the CLI, warnings of failed CLI runs;
  - lints: properties named `mapping` and example data are no longer taken
    for references; lints and exports run in linear time (23–31 % fewer
    instructions on large projects).
- Found while documenting a real project (about 200 routes, 7 WebSockets)
  with the 0.2 code: `@route-defaults` in module files were ignored;
  unannotated aliases polluted the document; function-pointer types leaked
  token text into references; flags after a quoted description and escaped
  quotes broke parameters; fragment responses were replaced silently;
  multi-line `@description` kept stray spaces; `@openapi` one-liners
  swallowed the following lines; prose next to `@openapi` blocks was read
  as YAML; `$Name` in schema lists of inline YAML became a string;
  `deprecated`/`example` of expanded parameters landed in the schema;
  `enum` overrides dropped `null` of nullable fields; findings in module
  defaults were repeated per route.
- Found by mutation testing (`cargo mutants`, 91–93 % of mutants caught
  before, the survivors now covered or documented as equivalent): flags
  after a quoted parameter description, Swagger hosts of URLs with an empty
  authority, custom type mappings to references used as `$Name`, `$ref`
  values that are no schema name (now OF3007), `~1`/`~0` in reference
  pointers of the path parameter lint and the AsyncAPI schema sharing, and
  the merge order: YAML blocks, routes and files are merged with the
  reflected schemas in source order, so a block no longer overrides the
  struct below it.

### Security
- `serde_yaml_ng` (and the unmaintained `unsafe-libyaml`, RUSTSEC-2024-0449)
  replaced by `serde-saphyr`, a pure Rust YAML library; documents are size-
  and depth-limited.
- `anyhow` advisory RUSTSEC-2026-0190 resolved; `cargo deny` checks
  advisories, licenses, bans and sources in CI.
- Input files are size-limited (16 MiB by default), nesting of fragments and
  generics is bounded, and `#![forbid(unsafe_code)]` holds for both crates.
- Panicking constructs are denied crate-wide (`clippy::indexing_slicing`,
  `string_slice`, `unwrap_used`).
- CI: actions pinned to commit hashes, workflow audit with `zizmor`, minimal
  dependency versions, weekly runs against the newest dependencies.

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
