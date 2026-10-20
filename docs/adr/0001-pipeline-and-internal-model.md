# 0001 – Pipeline stages and the internal document model

## Context
Up to 0.1.4 every stage passed YAML *text* to the next one. References and
generics were rewritten with regular expressions over raw text, so they also
fired inside descriptions and examples, broke on non-ASCII text and could not
report where a problem came from.

## Decision
The pipeline has explicit stages with typed inputs and outputs:

1. **source** – discover files deterministically (sorted, generated outputs excluded).
2. **extract** – parse Rust with `syn` into typed items (schemas, documents,
   routes, fragments, blueprints) carrying their source location.
3. **lexical** – the only text-level stage: `@insert`, the `$Vec<T>` shorthand,
   `{{…}}` variables, escapes. Its output is parsed to a value immediately.
4. **structural** – everything else works on `serde_json::Value`
   (`preserve_order`): `@extend`, DSL compilation, generic instantiation,
   smart-reference resolution, nullability, lowering.
5. **merge → normalize → lint → output.**

Smart references and generics are resolved on **whole string scalars**
(`$User`, `$Page<User>`), never inside other text. Every value produced from a
source carries an *origin* (file, line) kept next to it, so diagnostics and
errors point at the annotation that caused them.

## Consequences
- `$User` inside a description stays literal text (previously it was replaced).
- Errors and warnings always have a location.
- Each stage is testable and usable on its own (see 0008).
