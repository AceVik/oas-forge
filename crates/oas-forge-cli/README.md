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
Generate OpenAPI 3.2/3.1/3.0, Swagger 2.0, JSON Schema and AsyncAPI documents
from Rust source code and doc comments, without touching your build: no
build script, no macros, no new dependency in your crate.

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

## Usage

The binary is called `oas-forge`. Run it in the package directory:

```bash
oas-forge -i src -o openapi.yaml
oas-forge -i src -I static/security.yaml -o openapi.json
oas-forge -i src --output-openapi-30 openapi-3.0.yaml --output-swagger2 swagger.json
oas-forge -i src -o openapi.yaml --check --deny-warnings   # in CI
```

| Flag | Meaning |
| :--- | :--- |
| `-i`, `--input DIR` | Directory read recursively (repeatable) |
| `-I`, `--include FILE` | YAML, JSON or `.proto` file (repeatable) |
| `-o`, `--output FILE` | Full document (`.json` or `.yaml`) |
| `--output-schemas`, `--output-paths`, `--output-fragments FILE` | Parts of the document |
| `--output-openapi-30`, `--output-swagger2`, `--output-json-schema`, `--output-asyncapi FILE` | Exports |
| `--config FILE` | Additional configuration file |
| `--var NAME=VALUE` | Value of a `{{NAME}}` placeholder |
| `--scope NAME` | Write the document of one scope (`@scope`) to every output |
| `--check` | Write nothing; exit with status 1 when an output is out of date |
| `--deny-warnings` | Treat warnings as errors |
| `--strict` | Also report unused components and missing descriptions |
| `--message-format human\|json` | How findings are printed |

Without an output option the document goes to `openapi.yaml`.

## Configuration

Settings can also live in `[package.metadata.oas-forge]` of `Cargo.toml` or
in `openapi.toml`; flags on the command line win. The package metadata in
`Cargo.toml`, including fields inherited from the workspace, provides the
generated root (`info.title`, `info.version`, …) and the `{{CARGO_PKG_*}}`
variables.

```toml
# openapi.toml
input = ["src"]
include = ["static/security.yaml"]
output = ["openapi.yaml"]
output_swagger2 = ["swagger.json"]

[variables]
STAGE = "production"
```

## In CI

```yaml
- run: cargo binstall oas-forge-cli --no-confirm
- run: oas-forge -i src -o openapi.yaml --check --deny-warnings --message-format json
```

`--check` exits with status 1 when the committed document differs from what
the sources produce; `--message-format json` prints one finding per line for
annotations.

## Documentation

The [guide](https://github.com/AceVik/oas-forge/tree/main/docs) covers the
annotations, the route DSL, every option and every diagnostic code. The
[`oas-forge` README](https://github.com/AceVik/oas-forge#readme) has a quick
tour.

## License

Licensed under the [MIT license](https://github.com/AceVik/oas-forge/blob/main/LICENSE).
Release archives contain `THIRD-PARTY-LICENSES.md` with the licenses of all
compiled-in crates.
