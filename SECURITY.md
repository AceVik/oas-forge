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
outputs; it does not execute code from the sources, open network
connections or read environment variables other than `CARGO_PKG_*`,
`OUT_DIR` and those named in `{{env:NAME}}` placeholders.

Untrusted input (for example sources from a pull request in CI) can at most
make a run fail or produce a wrong document. To keep that bounded:

- input files over 16 MiB are skipped (`Generator::max_file_size`),
- symbolic links in input directories are followed only into input
  directories (`OF0008`),
- YAML documents are limited in size and nesting depth,
- fragment inserts, generic instantiation and chains of type aliases have
  fixed depth limits;
  generic reference parsing is also limited to 128 nested generic lists,
- fragment expansion and variable substitution have total text and work
  budgets, including acyclic fanout and parameter substitution; the
  concrete schemas of generic instantiation have a total size budget,
- outputs use exclusively created, unique temporary files and are replaced
  atomically at the configured paths; an existing output is compared only
  when it is a regular file, reading no more than a current one can hold,
- Rust sources are parsed and walked on a thread with a 256 MiB stack
  (32 MiB on 32-bit targets), so nesting in the thousands of levels is
  parsed instead of aborting the process (`syn` recurses once per level);
  input nested deeper than that still overflows it, which aborts the
  process: the run fails, with no other effect. `TypeExpr::parse` accepts
  at most 32 nested levels,
- both crates use `#![forbid(unsafe_code)]`.

Dependencies are checked with `cargo deny` (advisories, licenses, sources)
in CI.
