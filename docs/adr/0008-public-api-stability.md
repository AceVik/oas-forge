# 0008 – Public API stability rules

## Decision
Every pipeline stage is a documented, semver-stable library API. To keep that
promise affordable:

- No foreign types in public signatures except `serde_json::Value` and std
  types. `syn` and the YAML backend stay private (stages take source text).
- Public enums and option structs are `#[non_exhaustive]`; option structs have
  `Default` plus builder-style `with_*` setters.
- Public data types implement `Debug`, `Clone`, `PartialEq` where sensible.
- `#![deny(missing_docs)]`; every public item documents its behaviour and errors.
- CI runs `cargo-semver-checks` against the last release once 0.2.0 is out.
