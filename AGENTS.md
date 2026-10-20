# AGENTS.md

Instructions for AI coding agents (and humans) working on this repository.
Read this file first; details are linked.

## What this is

`oas-forge` is a compile-time OpenAPI generator for Rust. It parses Rust
source with `syn` (usually from a build script), reads doc comments and
serde/validator/garde attributes, and writes OpenAPI 3.2/3.1/3.0, Swagger 2.0,
JSON Schema and AsyncAPI 3 documents. There are no proc macros and nothing at
runtime.

- Library: `/` (crate `oas-forge`), Rust 2024, MSRV 1.89.
- CLI: `crates/oas-forge-cli` (binary `oas-forge`).
- Example workspace: `examples/` (not a workspace member).
- User guide: `docs/`; decisions: `docs/adr/`; changes since 0.1:
  `docs/migration-0.2.md`.

## Commands

```bash
cargo fmt --all
cargo clippy --workspace --all-targets --all-features -- -D warnings
cargo test --workspace --locked
cargo insta review                      # accept or reject snapshot changes
RUSTDOCFLAGS="-D warnings" cargo doc --workspace --no-deps
cargo deny check
PROPTEST_CASES=20000 cargo test --release --test property
cargo llvm-cov --workspace --summary-only
cargo build --manifest-path examples/Cargo.toml
typos                                   # spelling (config: _typos.toml)
cargo mutants --package oas-forge --test-tool nextest -f src/<changed>.rs
```

All of them must pass before a change is done. CI also checks the minimal
dependency versions (`cargo +nightly update -Z direct-minimal-versions`), the
MSRV build, `cargo publish --dry-run` on release, and audits the workflows with
`zizmor`. Actions in `.github/workflows/` are pinned to commit hashes.

## Where things are

| Stage | Module | Notes |
| :--- | :--- | :--- |
| Input discovery | `src/source.rs` | sorted walk, size limit, outputs excluded |
| Rust extraction | `src/extract/` | `reflect.rs` (types → schemas), `blocks.rs` (YAML blocks), `signature.rs` (handler inference), `constants.rs` (regex constants) |
| Doc comments | `src/docs.rs` | lines with positions, Javadoc blocks, Markdown-aware descriptions |
| Attributes | `src/attrs/` | `serde.rs`, `validate.rs` (validator + garde), `rename.rs`, `directives.rs` (`@openapi …` lines) |
| Text expansion | `src/template/lexical.rs` | the only text-level step: `@insert`, `@extend` markers, `{{vars}}`, `\$` |
| Templates | `src/template/` | `registry.rs`, `extend.rs`, `generics.rs` (`$Page<User>`), `refs.rs` (`$User`) |
| Route and channel DSL | `src/dsl/` | `mod.rs` (compiler), `params.rs`, `responses.rs`, `operation.rs`, `channel.rs`, `expand.rs` |
| Merging | `src/merge.rs` | policies `Document`, `Extend`, `Operation`, `Override` (ADR 0003) |
| Types | `src/schema/types.rs`, `shadow.rs` | `TypeExpr` parser and `TypeMapper` table; `shadow.rs` lets documented types win over built-in names |
| Dialects | `src/schema/dialect.rs` | 2020-12 → 3.0 / Swagger 2.0 / AsyncAPI lowering |
| Outputs | `src/output/` | rendering, exports, atomic writes |
| Orchestration | `src/generator.rs` | `Generator`, `Spec`, the pipeline `run` |
| Lints | `src/lint.rs` | OF5xxx checks |
| Findings | `src/diagnostics.rs` | codes, severities, locations |
| YAML | `src/yaml.rs` | the only user of `serde-saphyr` |

The pipeline is described in `docs/architecture.md`.

## Rules that must not be broken

1. **No panics on user input.** Report a `Diagnostic` (with a new, unique
   code if needed) or return an `Error` with a location. Never log; never
   drop input silently. The crates deny `clippy::indexing_slicing`,
   `string_slice` and `unwrap_used`: use `get`, `split_once`, `strip_prefix`
   and friends; `expect("reason")` only for real invariants such as constant
   regular expressions.
2. **Generic references stay raw until the generics stage.** Before it, write
   `{"$ref": "$Page<User>"}`, never `#/components/schemas/$Page<User>`;
   otherwise no concrete schema is generated. DSL types such as `$User` stay
   `{"$ref": "$User"}` too; `refs.rs` resolves them (aliases, built-ins,
   unknown names).
3. **Whole values only.** Smart references and generics are resolved on whole
   string scalars, never inside text.
4. **Unit responses have no `content`.** `@return 204:` must not produce a
   body.
5. **Byte offsets only.** Doc comments contain any Unicode. Never slice a
   `&str` with indices from `.chars()`.
6. **Determinism.** No `HashMap` iteration into output. Same input, any file
   order, any OS: byte-identical output.
7. **Public API.** No `syn` or YAML types in public signatures; add
   `#[non_exhaustive]` to public structs and enums that may grow.
8. **Documentation.** Every item, private ones included, has a doc comment
   (`missing_docs`, `clippy::missing_docs_in_private_items`); every public
   function and type has an `# Examples` doctest that runs and asserts real
   output (`no_run` only when it writes files), and fallible ones an
   `# Errors` section. Identifiers go in backticks (`clippy::doc_markdown`;
   allowed words are in `clippy.toml`). All of it is denied in `src/lib.rs`.
9. **YAML only through `src/yaml.rs`.**
10. **`serde_json::json!` keys are expressions.** `json!({ name: v })` uses the
   value of `name` as key; write `json!({ "name": v })` for a literal key.

## Tests

No test code in `src/`. Tests use the public API and live in `tests/`:

| Suite | Path | Use it for |
| :--- | :--- | :--- |
| unit | `tests/unit/` (mirrors `src/`) | a function or stage; `tests/unit/<module>.rs` for `src/<module>.rs` |
| corpus | `tests/corpus/cases/<name>/` | a feature end to end; outputs are validated against the official schemas and snapshotted |
| integration | `tests/integration/` | the generator on temporary projects, cross-crate, determinism, examples, docs |
| property | `tests/property/` | invariants over generated input |
| CLI | `crates/oas-forge-cli/tests/` | arguments, configuration layering, the binary |

Helpers are in `tests/common/` (`extract`, `schema`, `route`, `pipeline`,
`Project`, `spec_of`).

- Every behaviour change gets a test. DSL syntax: `tests/unit/dsl/`;
  reflection: `tests/unit/extract/`.
- A corpus snapshot may only change on purpose. Review with
  `cargo insta review` and list user-visible changes in
  `docs/migration-0.2.md` (until 0.2 is released; then in `CHANGELOG.md`).
- Examples in `README.md` and `docs/*.md` after `<!-- corpus: name -->` must
  equal `tests/corpus/cases/name/src/lib.rs` (`readme_*`, `guide_*`). Change
  both together.
- Coverage stays at or above 98 % of lines. Line coverage is not enough:
  run `cargo mutants` on the modules you change; every surviving mutant gets
  a test, or an entry with its reason in `.cargo/mutants.toml` when it cannot
  change behaviour.

## Writing annotations (quick reference)

```rust
/// A user.                         <- description
/// @openapi                        <- exports the type; YAML may follow
#[derive(serde::Serialize)]
pub struct User {
    /// @example 42
    pub id: u64,
}

/// Get a user                      <- summary
/// @route GET /users/{id: u64 "User id"}
/// @return 200: $User
/// @problem 404 "No such user"
pub async fn get_user() {}
```

The full syntax is in `docs/schemas.md`, `docs/route-dsl.md`,
`docs/templates.md` and `docs/asyncapi.md`.

## Commits

One logical change per commit, message in the imperative
(`feat: …`, `fix: …`, `test: …`, `docs: …`). Never commit local files
(archives, editor settings, build output).
