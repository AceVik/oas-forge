# 0002 – JSON Schema 2020-12 as canonical schema dialect

## Context
Targets disagree on schema features: OpenAPI 3.1/3.2 use JSON Schema 2020-12,
OpenAPI 3.0 a draft-04/05 subset with `nullable`, Swagger 2.0 draft-04 without
`oneOf`/`anyOf`, AsyncAPI 3.x a draft-07 superset.

## Decision
All schemas are produced in **JSON Schema 2020-12** (the OpenAPI 3.1/3.2
dialect). A final *lowering* pass converts them for the selected target:

| Feature | 3.1/3.2 | 3.0 | Swagger 2.0 | AsyncAPI 3 |
| :--- | :--- | :--- | :--- | :--- |
| nullable `T` | `type: [T, "null"]` / `anyOf` + `null` | `nullable: true` | `x-nullable: true` | `type: [T, "null"]` |
| `const` | kept | `enum: [v]` | `enum: [v]` | kept |
| `prefixItems` | kept | `items` + diagnostic | `items` + diagnostic | `items: [..]` |
| `exclusiveMinimum` (number) | kept | boolean form | boolean form | kept |
| `examples` (array) | kept | `example` | `example` | kept |

Anything that cannot be expressed in the target produces a diagnostic instead
of being dropped silently. The nullability style follows the `openapi` version
of the root document; without a root (fragments output) schemas stay in the
canonical dialect.

The pass lives in `schema::dialect`. It visits only schema positions (component
schemas, every `schema` key, and the subschema keywords below them), so
examples, defaults and other data that happen to look like schemas are never
rewritten. In 3.0 a `$ref` with siblings is wrapped in `allOf`, because 3.0
ignores the siblings of `$ref`.

Reflection decides what a Rust type accepts; nullability is carried next to
the schema (`MappedType::nullable`) so that parameters can turn `Option<T>`
into `required: false` without a nullable schema, while properties and bodies
render it.

## Consequences
One schema generator, many targets. Target differences live in one module and
are covered by validation against the official schemas.
