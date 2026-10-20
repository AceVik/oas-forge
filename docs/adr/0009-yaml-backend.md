# 0009 – YAML backend

## Context
`serde_yaml_ng` depends on `unsafe-libyaml`, which is unmaintained
(RUSTSEC-2024-0449). `serde_yml` and `noyalib` carry their own advisories.

## Decision
YAML is read and written through a private adapter (`yaml.rs`) backed by
**serde-saphyr** (pure safe Rust, actively maintained, fuzzed, located errors).
The adapter pins the options that change meaning:

- strict booleans (`yes`/`no`/`on`/`off` stay strings, YAML 1.2 semantics),
- duplicate keys are an error,
- merge keys (`<<`) are expanded,
- the `properties` interpolation feature stays disabled (it would clash with
  `$Name` smart references),
- an input budget limits pathological documents.

Fallback if a blocker appears: `saphyr` with an own value conversion. YAML and
JSON remain fully supported for input (doc comments, included files) and output.
