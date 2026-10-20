# 0006 – Diagnostics instead of logging

## Context
0.1.4 reported problems through `log`, which is invisible in build scripts.
Many problems (rejected routes, invalid overrides) were only logged and the
input was silently skipped.

## Decision
Every finding is a `Diagnostic { code, severity, message, location }` collected
by the generator and returned with the result.

- Codes are stable: `OF0xxx` sources/config, `OF1xxx` extraction and
  attributes, `OF2xxx` DSL, `OF3xxx` templates/generics/refs, `OF4xxx`
  merge/normalize, `OF5xxx` lints, `OF6xxx` output/targets.
- Severities: `error` (generation fails), `warning`, `note`.
- Users can raise warnings to errors (`deny_warnings`) or allow single codes.
- In build scripts the generator can print diagnostics as `cargo:warning=` and
  emit `cargo:rerun-if-changed` for every input it read.

## Consequences
Nothing is skipped silently. CI can fail on warnings; IDEs can consume JSON
diagnostics from the CLI.
