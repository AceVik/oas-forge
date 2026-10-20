# oas-forge-cli

The command-line interface of [`oas-forge`](https://crates.io/crates/oas-forge):
generate OpenAPI 3.2/3.1/3.0, Swagger 2.0, JSON Schema and AsyncAPI documents
from Rust source code and doc comments, without changing your build.

```bash
cargo install oas-forge-cli
```

The binary is called `oas-forge`. Run it in the package directory:

```bash
oas-forge -i src -o openapi.yaml
oas-forge -i src -I static/security.yaml -o openapi.json
oas-forge -i src --output-openapi-30 v3.0.yaml --output-swagger2 v2.yaml
oas-forge -i src -o openapi.yaml --check --deny-warnings   # in CI
```

| Flag | Meaning |
| :--- | :--- |
| `-i`, `--input DIR` | Directory read recursively (repeatable) |
| `-I`, `--include FILE` | YAML or JSON file to merge (repeatable) |
| `-o`, `--output FILE` | Full document (`.json` or `.yaml`) |
| `--output-schemas`, `--output-paths`, `--output-fragments FILE` | Parts of the document |
| `--output-openapi-30`, `--output-swagger2`, `--output-json-schema`, `--output-asyncapi FILE` | Exports |
| `--config FILE` | Additional configuration file |
| `--var NAME=VALUE` | Value of a `{{NAME}}` placeholder |
| `--check` | Write nothing; exit with status 1 when an output is out of date |
| `--deny-warnings` | Treat warnings as errors |
| `--strict` | Also report unused components and missing descriptions |
| `--message-format human\|json` | How findings are printed |

Without an output option the document goes to `openapi.yaml`. Settings can
also live in `[package.metadata.oas-forge]` of `Cargo.toml` or in
`openapi.toml`; the package metadata in `Cargo.toml` provides the generated
root and the `{{CARGO_PKG_*}}` variables.

See the [guide](https://github.com/AceVik/oas-forge/tree/main/docs) for the
annotations, the route DSL and every option.

## License

[MIT](https://github.com/AceVik/oas-forge/blob/main/LICENSE)
