# 0007 – Determinism and key order

## Decision
- Inputs are processed in sorted path order; registries iterate in sorted order.
- Output is byte-identical for the same inputs, independent of the file system,
  platform and hash seeds. A test shuffles input order to prove it.
- Key order of written documents:
  - top level follows the specification order (`openapi`, `info`,
    `jsonSchemaDialect`, `servers`, `paths`, `webhooks`, `components`,
    `security`, `tags`, `externalDocs`, then extensions);
  - `components.*` maps are sorted by name;
  - author-written YAML keeps the author's order;
  - generated schemas use a fixed keyword order (`$ref`, `type`, `format`,
    `title`, `description`, …, `properties`, `required`, …).
- Outputs are written atomically and only when their content changed, so
  build scripts do not trigger rebuild loops.
