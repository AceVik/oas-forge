# Getting started

`oas-forge` runs where your code is compiled: in a build script, or as a
command in CI. Both read the same sources and the same configuration.

## In a build script

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

- `input` directories are read recursively in sorted order: `.rs` files are
  parsed, `.yaml`, `.yml` and `.json` files are merged as OpenAPI snippets,
  and `.proto` files are indexed for [protobuf payloads](asyncapi.md#protocol-buffers)
  of AsyncAPI messages. `include` adds single files. Configured outputs are never read back as
  inputs, even inside an input directory. Symbolic links are followed
  when they lead into one of the input directories; a link that leads
  elsewhere is skipped (`OF0008`), so add its target as an input or include
  to read it. Broken links and loops are reported as `OF0005`, and a file
  that is reached twice (overlapping inputs, an include inside an input
  directory) is read once.
- `emit_cargo_directives(true)` prints `cargo:rerun-if-changed` for every
  input, every configuration file read with `Config::load()` (see below)
  and the package's `Cargo.toml` (the generated root and the
  `{{CARGO_PKG_*}}` variables come from it), `cargo:rerun-if-env-changed`
  for every `{{env:NAME}}` placeholder, so the script reruns only when
  something it reads changes, and every finding as `cargo:warning`.
- A missing `include` (`OF0001`) is watched through the nearest existing
  directory above it, so creating the file reruns the script. When that
  directory is the package directory or above it (where the build output
  lives, which changes on every build), the file itself is watched, and
  Cargo reruns the script on every build until the file exists. A missing
  `openapi.toml` is not watched: after creating one, touch `build.rs`.
- Files are written atomically and only when their content changes, so an
  unchanged document does not trigger rebuilds of crates that embed it.
- `generate()` fails on errors (an invalid route, unparsable YAML, two root
  documents, …). Warnings never fail it unless you call
  `deny_warnings(true)`.

Without an `openapi`/`info` root in the sources, the root is generated from
the package's `Cargo.toml` (name, version, description, license and homepage
or repository); see [Templates](templates.md#root-documents).

### Embedding the document

To serve the document from the binary, write it to `OUT_DIR` and embed it:

```rust,no_run
// build.rs
fn main() {
    let out = std::path::Path::new(&std::env::var("OUT_DIR").unwrap()).join("openapi.yaml");
    oas_forge::Generator::new()
        .input("src")
        .output(out)
        .emit_cargo_directives(true)
        .generate()
        .expect("OpenAPI generation failed");
}
```

```rust,ignore
// src/docs.rs
pub const OPENAPI: &str = include_str!(concat!(env!("OUT_DIR"), "/openapi.yaml"));

/// Get the OpenAPI document
/// @route GET /openapi.yaml
/// @return 200: String application/yaml
pub async fn openapi() -> impl axum::response::IntoResponse {
    ([(axum::http::header::CONTENT_TYPE, "application/yaml")], OPENAPI)
}
```

Writing into the source tree (`.output("openapi.yaml")`) instead makes the
document reviewable in pull requests; check it in CI with
[`--check`](#in-ci).

### Faster builds

Build scripts are compiled without optimizations by default; `syn` is much
faster when optimized. In the crate that runs the generator:

```toml
[profile.dev.build-override]
opt-level = 3
```

## On the command line

```bash
cargo install oas-forge-cli
```

The binary is called `oas-forge`. Run it in the package directory:

```bash
oas-forge -i src -o openapi.yaml
oas-forge -i src -I static/security.yaml -o openapi.json
oas-forge -i src --output-fragments dist/fragments.yaml
oas-forge -i src -o openapi.yaml --var STAGE=prod
```

| Flag | Meaning |
| :--- | :--- |
| `-i`, `--input DIR` | Directory read recursively (repeatable) |
| `-I`, `--include FILE` | Single YAML, JSON or `.proto` file (repeatable) |
| `-o`, `--output FILE` (alias `--out`) | Full document |
| `--output-schemas FILE` | Only `components.schemas` |
| `--output-paths FILE` | Only `paths` |
| `--output-fragments FILE` | Everything without the root, plus templates for other crates |
| `--output-openapi-30 FILE` | OpenAPI 3.0.3 export |
| `--output-swagger2 FILE` | Swagger 2.0 export |
| `--output-json-schema FILE` | JSON Schema 2020-12 bundle |
| `--output-asyncapi FILE` | AsyncAPI 3 document |
| `--config FILE` | Configuration file (in addition to `Cargo.toml` and `openapi.toml`) |
| `--var NAME=VALUE` | Value of a `{{NAME}}` placeholder (repeatable) |
| `--scope NAME` | Write the document of a [scope](route-dsl.md#scopes) to every output |
| `--check` | Write nothing; fail when an output is missing or out of date |
| `--deny-warnings` | Treat warnings as errors |
| `--strict` | Also report unused components and missing descriptions |
| `--message-format human\|json` | How findings are printed |

Without any output option, the full document goes to `openapi.yaml`. The CLI
reads the package metadata from `Cargo.toml` in the working directory
(fields inherited with `field.workspace = true` come from the workspace
manifest above it), both for the generated root and for the
`{{CARGO_PKG_*}}` variables.

Exit status: `0` on success, `1` when the run fails (errors, or warnings with
`--deny-warnings`) or `--check` finds an outdated output, `2` for invalid
arguments.

Warnings are printed on standard error, also when the run fails. With
`--message-format json`, every warning and error is one JSON object per line
on standard output (notes are left out in both formats):

```json
{"severity":"warning","code":"OF2006","message":"unknown directive `@frobnicate`","file":"src/api.rs","line":12}
```

`file`, `line` and `column` are present when the finding has a location. A
fatal error is printed as `{"severity":"error","message":"…"}`, an outdated
output as `{"severity":"error","message":"output is not up to date","file":"…"}`.

## Configuration

Instead of builder calls or flags, the settings can live in
`[package.metadata.oas-forge]` of `Cargo.toml` or in an `openapi.toml` next
to it:

```toml
# openapi.toml
input = ["src"]
include = ["static/security.yaml"]
output = ["openapi.yaml"]
output_fragments = ["dist/fragments.yaml"]
output_openapi_30 = ["dist/openapi-3.0.yaml"]

[types]
# Document a Rust type with a fixed schema (by name or path).
UserId = { type = "string", format = "uuid" }
"geo::Point" = { type = "array", items = { type = "number" }, minItems = 2, maxItems = 2 }

[variables]
STAGE = "dev"
```

| Key | Type | Builder method |
| :--- | :--- | :--- |
| `input` | list of directories | `input` |
| `include` | list of files | `include` |
| `output`, `output_schemas`, `output_paths`, `output_fragments` | lists of files | `output`, … |
| `output_json_schema`, `output_openapi_30`, `output_swagger2`, `output_asyncapi` | lists of files | `output_json_schema`, … |
| `scope` | string | `scope` ([scopes](route-dsl.md#scopes)) |
| `[types]` | table of schemas | `map_type` |
| `[variables]` | table of strings | `var` |

Later layers override earlier ones key by key; `[types]` and `[variables]`
are merged by name. The order is: `Cargo.toml`, `openapi.toml`, the
`--config` file, command-line flags. Unknown keys are an error.

A build script loads the same layers with `Config::load()`:

```rust,no_run
// build.rs
fn main() {
    let config = oas_forge::Config::load().expect("valid oas-forge configuration");
    oas_forge::Generator::new()
        .with_config(config)
        .emit_cargo_directives(true)
        .generate()
        .expect("OpenAPI generation failed");
}
```

## Other generator options

| Method | Effect |
| :--- | :--- |
| `var(name, value)` | Sets a `{{name}}` placeholder; see [variables](templates.md#variables) |
| `map_type(name, schema)` | Documents a Rust type with a fixed schema |
| `infer_signatures(false)` | Turns off [signature inference](route-dsl.md#signature-inference) |
| `package(Package)` / `without_package()` | Sets or removes the package used for the generated root and the `CARGO_PKG_*` variables |
| `strict_lints(true)` | Also reports unused components and operations or schemas without description |
| `deny_warnings(true)` | Turns warnings into errors |
| `max_file_size(bytes)` | Skips larger inputs with an error (default 16 MiB) |
| `output_as(kind, path)` | Any [`OutputKind`](outputs.md) |
| `output_scoped(scope, kind, path)` | The document of a [scope](outputs.md#scopes); one run writes several |
| `scope(name)` | The scope of `document()`, `render()` and the outputs without their own |

`build()` runs the pipeline without writing and returns a `Spec`: the
document, every diagnostic, `render(kind)` for any output in memory,
`write()` and `check()`.

## Annotations that rustdoc renders as code

The annotations are doc comments, so rustdoc renders them and
`clippy::doc_markdown` (pedantic) checks them. Written like code, they read
well in rustdoc and pass the lint, and the document stays the same:

- YAML in a fenced code block (```` ```yaml ````, ```` ```yml ````,
  ```` ``` ````, `~~~`) right after an `@openapi…`, `@asyncapi` or
  `@openapi-fragment` line, also after `@openapi` in route and channel
  blocks: the fences are removed, and the section ends at the closing fence.
  `@scope` lines of the block may stand between the header and the fence
  or right after the closing fence.
- Arguments of directives in backticks: `` @return 200: `Vec<$Art>` ``,
  `` @route GET `/art` ``, `` @insert `Paging(100)` ``, flag values and list
  items (`` enum=[`art_crop`, `art_full`] ``), names on types
  (`` @openapi rename `ConsoleDay` ``, `` @openapi rename-all `camelCase` ``),
  and URLs as autolinks (`<https://example.com/art>`).

<!-- corpus: guide_rustdoc -->
```rust
//! @openapi
//! ```yaml
//! openapi: 3.1.0
//! info:
//!   title: Gallery
//!   version: 1.0.0
//! ```
//!
//! @openapi-fragment `Paging(max)`
//! ```yaml
//! - name: page
//!   in: query
//!   schema: { type: integer, minimum: 1, maximum: {{max}} }
//! ```

use serde::Serialize;

/// A piece of art.
/// @openapi
/// ```yaml
/// x-table: art
/// ```
#[derive(Serialize)]
pub struct Art {
    pub id: u64,
    pub title: String,
}

/// List the art
///
/// @route GET `/art`
/// @query-param crop: `String` enum=[`art_crop`, `art_full`] default=`art_full` "Crop"
/// @return 200: `Vec<$Art>` "The art"
/// @external-docs <https://example.com/art>
/// @openapi
/// ```yaml
/// parameters:
///   @insert `Paging(100)`
/// ```
pub async fn list_art() {}
```

## In CI

```bash
oas-forge -i src -o openapi.yaml --check --deny-warnings
```

fails when the committed `openapi.yaml` differs from what the sources
produce, or when any finding is a warning. Add `--strict` to also require
descriptions and to flag unused components, and `--message-format json` for
annotations in CI tools. In Rust, `Spec::check()` returns the outdated paths.
