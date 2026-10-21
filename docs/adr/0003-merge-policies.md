# 0003 – Named merge policies

## Context
0.1.4 used three unrelated merge behaviours: the document merge appended and
de-duplicated arrays, `@extend` let the fragment overwrite everything, and DSL
overrides replaced whole arrays (YAML `parameters` dropped DSL parameters).

## Decision
Merging is done by one function with an explicit policy:

| Policy | Used for | Maps | Arrays | Scalars |
| :--- | :--- | :--- | :--- | :--- |
| `Document` | combining snippets/files | recursive | append + de-duplicate | later wins, conflict is a diagnostic |
| `Extend` | `@extend Fragment` | recursive | append + de-duplicate | **local wins**, fragment fills gaps |
| `Operation` | DSL + embedded YAML | recursive | `parameters` keyed by (`name`, `in`) and merged, everything else append + de-duplicate | explicit YAML wins, diagnostic on conflict |
| `Override` | `@openapi` YAML on structs, enums, fields, aliases | recursive | **replaced** | override wins, no diagnostic |

Positional arrays (`prefixItems`, `allOf`, `oneOf`, `anyOf`) are never
de-duplicated: under `Document` and `Operation` a differing positional array
replaces the old one and is reported, under `Extend` the local one is kept.

`Override` exists because a schema override is a deliberate correction of the
reflected schema: `required: [id]` on a struct must replace the reflected
`required` list, not be appended to it.

## Consequences
`@extend` behaves like inheritance (local values override the base). YAML
`parameters` in a route add to DSL parameters instead of replacing them; a YAML
parameter with the same `name` and `in` refines the DSL parameter (for example
adds a `description`) and keeps its schema. Overrides of reflected schemas can
still replace lists.
