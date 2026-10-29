<h1 align="center">
  <a href="https://github.com/AceVik/oas-forge">
    <img src="https://raw.githubusercontent.com/AceVik/oas-forge/main/docs/assets/banner.svg" alt="oas-forge: compile-time OpenAPI for Rust" width="100%">
  </a>
</h1>

<p align="center">
  <a href="https://crates.io/crates/oas-forge-cli"><img src="https://img.shields.io/crates/v/oas-forge-cli.svg?logo=rust" alt="crates.io"></a>
  <a href="https://github.com/AceVik/oas-forge/releases/latest"><img src="https://img.shields.io/github/v/release/AceVik/oas-forge?label=binaries" alt="Prebuilt binaries"></a>
  <a href="https://github.com/AceVik/oas-forge/actions/workflows/ci.yml"><img src="https://github.com/AceVik/oas-forge/actions/workflows/ci.yml/badge.svg" alt="CI"></a>
  <a href="https://github.com/AceVik/oas-forge/blob/main/LICENSE"><img src="https://img.shields.io/crates/l/oas-forge-cli.svg" alt="License: MIT"></a>
</p>

**The command line of [`oas-forge`](https://crates.io/crates/oas-forge).**
Point it at your Rust sources and get **OpenAPI 3.2, 3.1 or 3.0**, **Swagger
2.0**, **JSON Schema** and **AsyncAPI 3** documents, written from your types,
their serde, validator and garde attributes and the annotations in your doc
comments. Your crate stays as it is: no build script, no macros, no new
dependency.

- **Works with any build.** Run it by hand, from a script, a pre-commit hook
  or a CI job; the documents are files you review and commit.
- **Made for CI.** `--check` fails when a committed document is out of date,
  `--deny-warnings` on any finding, and `--message-format json` prints
  findings for tools. The output is byte-identical on every platform.
- **Configured once.** Flags, an `openapi.toml` or
  `[package.metadata.oas-forge]` in `Cargo.toml`; afterwards a bare
  `oas-forge` does the job.

## Library or command line?

Both read the same annotations and write the same documents. They differ in
when they run:

| | `oas-forge` in `build.rs` | `oas-forge-cli` |
| :--- | :--- | :--- |
| Runs | on every `cargo build` | when you call it: by hand, in a script, in CI |
| Your crate | gets a build dependency and a build script | stays unchanged |
| Settings | builder methods or the configuration files | flags or the configuration files |
| Best for | documents that always match the build, embedding with `include_str!` | committed documents checked in CI, workspaces, release scripts |

For the build-script setup see the
[`oas-forge` README](https://github.com/AceVik/oas-forge#readme).

## Install

```bash
cargo install oas-forge-cli --locked     # from source
cargo binstall oas-forge-cli             # prebuilt, with cargo-binstall
```

Prebuilt binaries for Linux (x86_64, aarch64, musl), macOS (Intel, Apple
silicon) and Windows are attached to every
[GitHub release](https://github.com/AceVik/oas-forge/releases), with
`SHA256SUMS` and build provenance you can verify:

```bash
gh attestation verify oas-forge-v0.2.0-x86_64-unknown-linux-gnu.tar.gz --repo AceVik/oas-forge
```

The binary is called `oas-forge`; `oas-forge --help` lists every option.

## Tutorial: your first document

A small notes service, from annotated code to a document that CI keeps
up to date.

### 1. Annotate the code

`@openapi` exports a type; `@route`, `@return` and friends document a
handler. Everything else comes from the code: field types, `Option`, serde
attributes.

<!-- corpus: readme_cli -->
```rust
use serde::{Deserialize, Serialize};

/// A note.
/// @openapi
#[derive(Serialize)]
pub struct Note {
    pub id: u64,
    pub text: String,
    pub done: bool,
}

/// What a client sends to add a note.
/// @openapi
#[derive(Deserialize)]
pub struct NewNote {
    /// @example "Buy milk"
    pub text: String,
}

/// List notes
/// @route GET /notes
/// @query-param done: Option<bool> "Only done or only open notes"
/// @return 200: Vec<$Note>
pub async fn list_notes() {}

/// Add a note
/// @route POST /notes
/// @body $NewNote
/// @return 201: $Note "Created"
/// @problem 422 "The text is empty"
pub async fn add_note() {}
```

### 2. Generate the document

Run the tool in the package directory, next to `Cargo.toml`:

```bash
oas-forge -i src -o openapi.yaml
```

`openapi.yaml` now holds an OpenAPI 3.1 document. The sources define no
`openapi`/`info` root of their own, so it comes from `Cargo.toml`:

```yaml
openapi: "3.1.1"
info:
  title: notes                          # [package] name, version, …
  version: "0.3.0"
  description: Notes for the whole team
  license:
    name: MIT
    identifier: MIT
paths:
  /notes:
    get:
      summary: List notes
      operationId: list_notes
      parameters:
      - name: done
        in: query
        required: false                 # an Option is optional
        description: Only done or only open notes
        schema:
          type: boolean
      responses:
        "200":
          # … an array of Note
    post:
      summary: Add a note
      operationId: add_note
      # … the NewNote request body and the 201 response
      responses:
        "422":
          description: The text is empty
          content:
            application/problem+json:
              schema:
                $ref: "#/components/schemas/ProblemDetails"
components:
  schemas:
    # … NewNote, Note and ProblemDetails (RFC 9457, added for @problem)
```

### 3. Read the findings

Mistakes are reported with a stable code and the place in the source. A
typo, `@body $NewNot` instead of `@body $NewNote`, gives:

```text
warning[OF3007]: `$NewNot` does not name a known schema (src/lib.rs:26)
warning[OF5001]: `$ref: #/components/schemas/NewNot` points to a component that does not exist (src/lib.rs:26)
```

Line 26 is where the doc comment of `add_note` starts. Warnings do not stop
the run; with `--deny-warnings` they are errors and the exit status is 1.
[Diagnostics](https://github.com/AceVik/oas-forge/blob/main/docs/diagnostics.md)
explains every code and how to fix it.

### 4. Save the settings

Write the settings into an `openapi.toml` next to `Cargo.toml` (or into
`[package.metadata.oas-forge]` of `Cargo.toml`):

```toml
# openapi.toml
input = ["src"]
output = ["openapi.yaml"]
```

From now on a bare `oas-forge` writes the document.

### 5. Keep it up to date in CI

Commit `openapi.yaml`. In CI, `--check` writes nothing and fails when the
committed document differs from what the sources produce:

```bash
oas-forge --check --deny-warnings
```

A complete workflow is in [GitHub Actions](#github-actions) below.

## Recipes

### Several formats in one run

Every output flag can be repeated, and the file extension (`.yaml`, `.yml`
or `.json`) selects the format:

```bash
oas-forge -i src \
  -o openapi.yaml -o openapi.json \
  --output-openapi-30 openapi-3.0.yaml \
  --output-swagger2 swagger.json \
  --output-json-schema schemas.json
```

What OpenAPI 3.0 or Swagger 2.0 cannot express is reported, never dropped
silently; [Outputs](https://github.com/AceVik/oas-forge/blob/main/docs/outputs.md)
lists what each export approximates.

### A workspace

Run the tool in the directory of the package that names the API, and read
the crates that hold its handlers and types. Fields inherited with
`version.workspace = true` and the like come from the workspace manifest:

```bash
cd crates/api
oas-forge -i src -i ../model/src -o ../../docs/openapi.yaml
```

A virtual workspace root has no `[package]` to name the document, so a run
there fails with *No Root OpenAPI definition found*. Either run in a
package directory, or bring a root of your own:

```yaml
# api.yaml
openapi: 3.1.0
info:
  title: Notes
  version: "{{VERSION}}"
```

```bash
oas-forge -i crates/api/src -i crates/model/src -I api.yaml --var VERSION=1.4.0 -o docs/openapi.yaml
```

### Several documents from one code base

`@scope` assigns a route, a root or a channel to the documents of the named
scopes; blocks without a scope are shared by all of them:

```rust,ignore
/// Purge all notes
/// @scope admin
/// @route DELETE /notes
/// @return 204:
pub async fn purge() {}
```

```bash
oas-forge -i src --scope public -o openapi.yaml    # the shared routes only
oas-forge -i src --scope admin -o admin.yaml       # the shared routes and the admin ones
```

Without `--scope`, every block goes into one document. Each scoped document
keeps only the components its operations use. One run writes one scope;
[`Generator::output_scoped`](https://docs.rs/oas-forge/latest/oas_forge/struct.Generator.html#method.output_scoped)
writes several from a build script. See
[Scopes](https://github.com/AceVik/oas-forge/blob/main/docs/route-dsl.md#scopes).

### Variables

`{{NAME}}` placeholders in the sources and YAML files are filled from
`--var`, from `[variables]` in the configuration, from `Cargo.toml`
(`{{CARGO_PKG_VERSION}}`, …) and from the environment variables you allow
(`{{env:API_URL}}` with `--allow-env API_URL` or `allow_env = ["API_URL"]`);
other `{{env:…}}` placeholders are kept and reported:

```bash
oas-forge -i src -o openapi.yaml --var STAGE=staging --allow-env API_URL
```

### Event-driven APIs

`@asyncapi` and `@channel` blocks describe message channels, WebSockets
included; their payloads are serde types or protobuf messages from `.proto`
files in the input directories or given with `-I`:

```bash
oas-forge -i src -I proto/app/v1/frames.proto -o openapi.yaml --output-asyncapi asyncapi.yaml
```

The AsyncAPI document shares the component schemas with the OpenAPI
document; see [AsyncAPI](https://github.com/AceVik/oas-forge/blob/main/docs/asyncapi.md).

## GitHub Actions

```yaml
# .github/workflows/api.yml
name: API documents
on: [push, pull_request]
jobs:
  openapi:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v7
      # Installs the prebuilt binary with cargo-binstall.
      - uses: taiki-e/install-action@v2
        with:
          tool: oas-forge-cli
      - run: oas-forge --check --deny-warnings
```

`cargo install oas-forge-cli --locked` works too; it builds from source. For
tools that read findings, add `--message-format json`.

## Reference

### Options

| Option | Meaning |
| :--- | :--- |
| `-i`, `--input DIR` | Directory read recursively for Rust, YAML, JSON and `.proto` files |
| `-I`, `--include FILE` | A single YAML, JSON or `.proto` file |
| `-o`, `--output FILE` | The full document in the version of its root (alias `--out`) |
| `--output-schemas FILE` | Only `components.schemas` |
| `--output-paths FILE` | Only `paths` |
| `--output-fragments FILE` | Everything except the root, plus templates, for other crates to include |
| `--output-openapi-30 FILE` | An OpenAPI 3.0.3 export |
| `--output-swagger2 FILE` | A Swagger 2.0 export |
| `--output-json-schema FILE` | A JSON Schema 2020-12 bundle of the component schemas |
| `--output-asyncapi FILE` | The AsyncAPI 3 document |
| `--scope NAME` | Write the document of this scope to every output |
| `--config FILE` | An additional configuration file |
| `--var NAME=VALUE` | The value of a `{{NAME}}` placeholder |
| `--allow-env NAME` | An environment variable `{{env:NAME}}` may read |
| `--check` | Write nothing; fail when an output is missing or out of date |
| `--deny-warnings` | Treat warnings as errors |
| `--strict` | Also report unused components and missing descriptions |
| `--message-format human\|json` | How findings are printed (default `human`) |
| `-h`, `--help` | Print the help |
| `-V`, `--version` | Print the version |

`--input`, `--include`, `--var`, `--allow-env` and every output option can
be repeated.
Without any output option, the full document goes to `openapi.yaml`.
Relative paths are relative to the working directory.

### Configuration

Settings are read in this order, and each layer overrides the one before it
key by key:

1. `[package.metadata.oas-forge]` in `Cargo.toml` of the working directory
2. `openapi.toml` in the working directory
3. the file given with `--config`
4. the command-line flags

The keys are the long option names with underscores; `[types]` maps Rust
types to schemas, `[variables]` sets placeholders and `allow_env` lists
the environment variables `{{env:NAME}}` may read. Unknown keys are an
error. The lists and tables (`allow_env`, `variables`, `types`, `files`)
add up over the layers instead: a later layer adds entries and replaces
those of the same name, but cannot take away what an earlier one allows.

```toml
# openapi.toml
input = ["src"]
include = ["static/security.yaml"]
output = ["openapi.yaml"]
output_swagger2 = ["dist/swagger.json"]

[types]
UserId = { type = "string", format = "uuid" }

[variables]
STAGE = "production"
```

`Cargo.toml` of the working directory also provides the generated root
(`info.title`, `info.version`, `info.description`, `info.license`, …) and the
`{{CARGO_PKG_*}}` variables.

### Exit status

| Status | Meaning |
| :--- | :--- |
| `0` | The documents were written, or with `--check` every output is up to date |
| `1` | An error, a warning with `--deny-warnings`, or with `--check` an output that is missing or out of date |
| `2` | Invalid arguments |

### Machine-readable findings

With `--message-format json`, every warning and error is one JSON object per
line on standard output:

```json
{"severity":"warning","code":"OF3007","message":"`$NewNot` does not name a known schema","file":"src/lib.rs","line":26}
```

`file`, `line` and `column` are present when the finding has a location. A
fatal error is printed as `{"severity":"error","message":"…"}`, an outdated
output with `--check` as
`{"severity":"error","message":"output is not up to date","file":"openapi.yaml"}`.

## Used by

<table>
  <tr>
    <td align="center" width="200">
      <a href="https://baylee.app"><img src="https://raw.githubusercontent.com/AceVik/oas-forge/main/docs/assets/used-by/baylee.png" alt="Baylee" width="160"></a>
    </td>
    <td>
      <a href="https://baylee.app"><b>Baylee</b></a>: a free, non-commercial 3D table for a trading card game; play with friends, against a house AI or a language model. The public and admin HTTP APIs of its game server, its WebSocket protocol (AsyncAPI 3 with protobuf messages) and its feedback service are documented with oas-forge.<br>
      <a href="https://baylee.app">baylee.app</a> · <a href="https://github.com/AceVik/baylee">GitHub</a>
    </td>
  </tr>
</table>

Using oas-forge in your project?
[Open a pull request](https://github.com/AceVik/oas-forge/pulls) to add it
here.

## Documentation

| Guide | Content |
| :--- | :--- |
| [Getting started](https://github.com/AceVik/oas-forge/blob/main/docs/getting-started.md) | build scripts, the command line, configuration, CI |
| [Schemas](https://github.com/AceVik/oas-forge/blob/main/docs/schemas.md) | types, directives, examples, generics |
| [Route DSL](https://github.com/AceVik/oas-forge/blob/main/docs/route-dsl.md) | every directive, embedded YAML, inference, module defaults, scopes |
| [AsyncAPI](https://github.com/AceVik/oas-forge/blob/main/docs/asyncapi.md) | channels, WebSockets and protobuf messages |
| [Outputs](https://github.com/AceVik/oas-forge/blob/main/docs/outputs.md) | formats, versions and what each export approximates |
| [Diagnostics](https://github.com/AceVik/oas-forge/blob/main/docs/diagnostics.md) | every code and how to fix it |

## License

Licensed under the [MIT license](https://github.com/AceVik/oas-forge/blob/main/LICENSE).
Release archives contain `THIRD-PARTY-LICENSES.md` with the licenses of all
compiled-in crates.
