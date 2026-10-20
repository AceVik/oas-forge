# Official specification schemas

Vendored copies of the official JSON Schemas used by the test suites to
validate every generated document offline. `manifest.json` maps each schema
URI to its file and records the retrieval date.

| Format | Source | License |
| :--- | :--- | :--- |
| Swagger 2.0, OpenAPI 3.0 / 3.1 / 3.2 | <https://spec.openapis.org/oas/> | Apache-2.0 (OpenAPI Initiative) |
| AsyncAPI 3.0 / 3.1 | <https://asyncapi.com/definitions/> | Apache-2.0 (AsyncAPI Initiative) |

The JSON Schema meta-schemas (draft-04 … 2020-12) are bundled with the
`jsonschema` crate and are not vendored here.

To refresh, download the same URIs again (follow every absolute `$ref`,
`$dynamicRef` and `$schema` that is not embedded via `$id`) and update
`manifest.json`.
