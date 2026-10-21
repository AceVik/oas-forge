# Architecture

This page is for contributors. The decisions behind it are recorded in the
[ADRs](adr/README.md).

## Workspace

| Crate | Path | Contents |
| :--- | :--- | :--- |
| `oas-forge` | `/` | the library: every pipeline stage, no CLI dependencies |
| `oas-forge-cli` | `crates/oas-forge-cli` | the `oas-forge` binary: arguments, configuration layering, output of findings |
| examples | `examples/` | a separate workspace of two crates that use the library from build scripts |

## The pipeline

`Generator::build` (`src/generator.rs`) runs these stages:

```text
source ──► extract ──► register ──► lexical ──► parse ──► extend ──► DSL
                      (templates)   (@insert,              (@extend)  (@route,
                                    {{vars}})                         @channel)
   ──► generics ──► refs ──► merge ──► normalize ──► lower ──► lint ──► output
       ($Page<T>)  ($User)            (expand params)  (dialect)
```

1. **Sources** (`src/source.rs`): walk the inputs in sorted order, read
   `.rs`, `.yaml`, `.yml`, `.json` and `.proto` files, skip outputs and
   files over the size limit. `.proto` files are only indexed
   (`src/proto.rs`) for `proto(package.Message)` payloads of channels.
2. **Extraction** (`src/extract/`): parse each Rust file with `syn` and turn
   annotations into typed `Item`s: reflected schemas (`reflect.rs`, driven by
   the attribute models in `src/attrs/`), free YAML documents and named
   schemas (`blocks.rs`), fragments, blueprints, routes with their handler
   signature (`signature.rs`), channels and regex constants
   (`constants.rs`).
3. **Registration**: templates (fragments, blueprints, Rust-name aliases,
   type aliases without `@openapi`) go into the `Registry`
   (`src/template/registry.rs`); reflected schemas become snippets right
   away; texts wait until every template is known.
4. **Lexical expansion** (`src/template/lexical.rs`), the only text-level
   step: `@insert`, `@extend` markers, `{{variables}}` and `\$` escapes. Its
   output is parsed to `serde_json::Value` immediately (`src/yaml.rs`).
5. **`@extend`** (`src/template/extend.rs`) merges fragments structurally.
6. **DSL** (`src/dsl/`): `@route`, `@webhook` and `@channel` blocks compile
   to document fragments.
7. **Generics** (`src/template/generics.rs`): every `$Name<Args>` is
   instantiated from its blueprint, to a fixed point with a depth limit.
   References to type aliases without `@openapi` are replaced by the
   alias's target first (`src/template/aliases.rs`).
8. **References** (`src/template/refs.rs`): `$Name` becomes
   `#/components/schemas/Name`, built-in types become their schema, regex
   constants their pattern.
9. **Merge** (`src/merge.rs`): snippets are merged in source order under an
   explicit policy; OpenAPI and AsyncAPI snippets go to separate documents.
   Without a root, the package root is used (`src/package.rs`). Stages 1 to
   8 run once (`generator::prepare`); this stage and the following ones run
   once per requested scope (`generator::assemble`), each with the shared
   snippets and those of its scope (`src/scope.rs`).
10. **Normalization**: struct expansions (`@query-params $T`) become
    parameters (`src/dsl/expand.rs`); the AsyncAPI document shares the
    component schemas its messages use; a scoped document is pruned to
    what its operations reach; a canonical copy (2020-12) is kept for
    exports.
11. **Lowering** (`src/schema/dialect.rs`): schemas and documents are
    converted for the root's OpenAPI version.
12. **Lints** (`src/lint.rs`) check the merged documents.
13. **Outputs** (`src/output/`) render and write each configured kind.

## Data model

- Documents are `serde_json::Value` with `preserve_order`; YAML is only an
  input and output format (`src/yaml.rs`, ADR 0009).
- Schemas are generated in JSON Schema 2020-12 and lowered per target
  (ADR 0002).
- Every snippet carries its `Location`; diagnostics point at the annotation
  that caused them (ADR 0006).
- `TypeExpr` (`src/schema/types.rs`) is the one parser for Rust type
  expressions, shared by extraction, the DSL and the generics stage;
  `TypeMapper` maps it to a schema.

## Rules

These hold everywhere; reviews check them.

1. **No panics on input.** Anything a user writes can be wrong; report a
   diagnostic or return an `Error` with a location. `unwrap` is only allowed
   on invariants of the code itself (constant regexes).
2. **Nothing is dropped silently.** Ignored input produces at least a note.
3. **Whole values only.** Smart references and generic references are
   resolved on whole string scalars (`$User`, `$Page<User>`), never inside
   text.
4. **Generic references stay raw until the generics stage.** Code before it
   writes `{"$ref": "$Page<User>"}`, never
   `#/components/schemas/$Page<User>`.
5. **No body for unit responses.** A response without type (`204`) has no
   `content`.
6. **Byte offsets only.** Doc comments contain any Unicode; never slice a
   `&str` with indices from `.chars()`.
7. **Deterministic output.** No hash-ordered iteration into output; sorted
   inputs; specified key order (ADR 0007).
8. **Stable public API.** Public types are `#[non_exhaustive]` where they may
   grow; no `syn` or YAML library types in public signatures (ADR 0008).
9. **YAML in one place.** Only `src/yaml.rs` uses the YAML library.
10. **`#![forbid(unsafe_code)]`** and `#![deny(missing_docs)]`.

## Tests

No test code lives in `src/`. The suites in `tests/` use the public API:

| Suite | Path | Purpose |
| :--- | :--- | :--- |
| unit | `tests/unit/` | mirrors `src/`: `tests/unit/<module>.rs` tests `src/<module>.rs` |
| corpus | `tests/corpus/` | black-box cases: every output of every case is validated against the official schemas (`tests/schemas/`) and snapshotted |
| integration | `tests/integration/` | the generator on temporary projects, cross-crate templates, determinism, the `examples/` workspace, documentation examples |
| property | `tests/property/` | `proptest` invariants: no stage panics, round trips hold |
| CLI | `crates/oas-forge-cli/tests/` | argument parsing and the binary end to end |

A corpus case is a directory `tests/corpus/cases/<name>/` with a
`case.toml` and sources. A change of any snapshot must be deliberate: review
it with `cargo insta review` and list user-visible changes in the
[migration guide](migration-0.2.md). Documentation examples marked with
`<!-- corpus: name -->` are the sources of `readme_*` and `guide_*` cases.

```bash
cargo test --workspace                                  # everything
cargo test --test unit                                  # one suite
cargo insta review                                      # review snapshot changes
PROPTEST_CASES=20000 cargo test --release --test property
cargo llvm-cov --workspace --summary-only               # coverage
cargo build --manifest-path examples/Cargo.toml         # real build scripts
```

## Adding a feature

1. Write the corpus case or unit test first.
2. Put the code in the stage it belongs to; keep `generator.rs` an
   orchestrator.
3. New findings get a new diagnostic code (never reuse one) and a row in
   [Diagnostics](diagnostics.md).
4. Document the syntax in the guide and add a line to the changelog.
