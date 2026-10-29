# Security policy

## Supported versions

| Version | Supported |
| :--- | :--- |
| 0.2.x | yes |
| < 0.2 | no; please upgrade (see the [migration guide](docs/migration-0.2.md)) |

## Reporting a vulnerability

Please report vulnerabilities privately through
[GitHub security advisories](https://github.com/AceVik/oas-forge/security/advisories/new),
not in public issues. Include the version, a minimal input that shows the
problem and what you expected. You will get an answer within a week; fixes
are released as patch versions and announced in the advisory.

## Threat model

`oas-forge` runs at build time on the sources of your own crate. It reads
files from the configured input directories and writes the configured
outputs; it does not execute code from the sources or open network
connections. It reads the environment variables `CARGO_PKG_*`,
`CARGO_MANIFEST_DIR` and `OUT_DIR` (the package), those you allow for
`{{env:NAME}}` placeholders (`Generator::allow_env`, `allow_env`,
`--allow-env`), and in the CLI `NO_COLOR` and `CLICOLOR_FORCE` (colors).
A source cannot choose which environment variables are read.

Untrusted input (for example sources from a pull request in CI) can at most
make a run fail or produce a wrong document. To keep that bounded:

- input files over 16 MiB are skipped (`Generator::max_file_size`),
- symbolic links in input directories are followed only into input
  directories (`OF0008`),
- YAML documents are limited in size and nesting depth, and to four
  nodes and sixteen bytes of scalars per byte of their text, so that
  aliases cannot expand a small document into a large one,
- fragment inserts, generic instantiation and chains of type aliases have
  fixed depth limits;
  generic reference parsing is also limited to 128 nested generic lists,
- fragment expansion and variable substitution have total text and work
  budgets, including acyclic fanout and parameter substitution, per block
  and for the text the whole run pastes (fragments, `@extend` bodies,
  copies of route defaults, of a block and its defaults for each of its
  `@route` lines and of `.proto` files, each pasted line counted with 256
  bytes besides its text); struct parameter expansion has a size and a
  work budget per document, and the `formData` parameters of a Swagger
  2.0 export the same size budget; the
  concrete schemas of generic instantiation and the targets of replaced
  type aliases have a total size budget (each value counted with 32 bytes
  besides its text and each instance with 256, so that filling it takes
  about half a gigabyte),
- outputs use exclusively created, unique temporary files and are replaced
  atomically at the configured paths; an existing output is compared only
  when it is a regular file, reading no more than a current one can hold,
- Rust sources are parsed and walked on a thread with a 256 MiB stack
  (32 MiB on 32-bit targets): `syn` recurses once per nesting level, and
  the main thread of a build script holds only about 150 nested types
  (about 20 on Windows). The large stack holds several thousand levels; a
  crafted file of a few kilobytes (about 8 000 nested references
  `&&&…`) still overflows it, which aborts the process: the run fails,
  with no other effect. `TypeExpr::parse` accepts at most 32 nested
  levels,
- findings and errors print control characters escaped, so the text of an
  input cannot rewrite a terminal line or start a Cargo directive,
- an annotation that adds to a `security` or `servers` list another one
  set is reported (`OF4001`),
- keys of the reserved prefix `x-oas-forge-`, the generator's own markers,
  written in a source are removed and reported (`OF0009`), so that a
  source cannot pose as a stage of the generator: a forged marker could
  replace the object around it, a security requirement for example,
  without a trace. A name that the route or channel DSL or reflection
  would make such a key (a `@security` scheme, a header, status code,
  webhook or media type, a
  `serde(rename)`, a channel or message name, …) is rejected with the
  same error, each stage takes its marker only in the shape it writes
  it, and a key of the prefix left in a finished document, the AsyncAPI
  document included, is removed from it with a finding,
- both crates use `#![forbid(unsafe_code)]`.

The configuration is trusted like `build.rs`: `openapi.toml`,
`[package.metadata.oas-forge]` in `Cargo.toml` and `--config` choose the
inputs, includes, outputs and allowed environment variables, so a change
to them can read any file or variable and write anywhere. The CLI reads
the first two from its working directory, and the lists of all layers add
up (`allow_env` of an `openapi.toml` stays allowed whatever `--config`
says). To check an untrusted change in CI, run it from a directory outside
the checkout, with the inputs and outputs as flags and a `--config` file
that the change cannot edit, and with `--deny-warnings`: a source that
adds to or redefines security requirements and schemes (`OF4001`), or
whose security a Swagger 2.0 export cannot express (`OF6002`), is
reported as a warning, which then fails the run.

Memory use grows with the size of the annotations, by roughly two
hundred bytes per byte: inputs with many megabytes of route or channel
blocks need gigabytes. Budgets bound what a small input can expand into,
not the size of the inputs: filling the budget for pasted text with short
lines from about a hundred kilobytes of annotations takes about a gigabyte
and a second or two, and filling it together with the budgets of struct
expansion and generic instantiation about 1.5 GB and a few seconds. Each scope may have a root of its own (see
[scopes](docs/route-dsl.md#scopes)); such a root replaces the shared one
for its scope, including the global `security`, so review a new scoped
root like a change of the root.

Outputs are written at the paths you configure. Like any program, the
generator follows a symbolic link to a directory on such a path, so do not
configure outputs below a directory that an untrusted change can replace
with a link.

Dependencies are checked with `cargo deny` (advisories, licenses, sources)
in CI.
