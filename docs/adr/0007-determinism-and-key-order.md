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
  - the `responses` of every operation (in `paths`, `webhooks`,
    `components.pathItems`, callbacks and `components.callbacks`) are
    sorted by status, wherever the codes came from (fragments, inference,
    the route's own `@return` lines, YAML): status codes ascending, a range
    right after the codes of its class (`200`, `201`, `2XX`, `400`, `404`,
    `4XX`, …, with the case of the range kept), then `default`, then other
    keys (extensions) in their order. `components.responses` maps names
    and is sorted by name like the other `components.*` maps;
  - author-written YAML keeps the author's order;
  - generated schemas use a fixed keyword order (`$ref`, `type`, `format`,
    `title`, `description`, …, `properties`, `required`, …).
- Outputs are written atomically and only when their content changed, so
  build scripts do not trigger rebuild loops.
