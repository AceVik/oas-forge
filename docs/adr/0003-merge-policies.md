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
| `Operation` | DSL + embedded YAML | recursive | `parameters` keyed by (`name`, `in`), everything else append + de-duplicate | explicit YAML wins, diagnostic on conflict |

Positional arrays (`prefixItems`, `allOf`, `oneOf`, `anyOf`) are never
de-duplicated.

## Consequences
`@extend` behaves like inheritance (local values override the base). YAML
`parameters` in a route add to DSL parameters instead of replacing them.
