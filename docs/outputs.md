# Outputs

One run can write any number of files. Each is configured with a builder
method, a configuration key or a CLI flag:

| Kind (`OutputKind`) | Builder / key / flag | Content | Needs |
| :--- | :--- | :--- | :--- |
| `Full` | `output` / `-o` | the full OpenAPI document | an OpenAPI root |
| `Schemas` | `output_schemas` | `components.schemas` only | |
| `Paths` | `output_paths` | `paths` only | |
| `Fragments` | `output_fragments` | everything but the root, plus templates, for other crates | |
| `OpenApi30` | `output_openapi_30` | the document as OpenAPI 3.0.3 | an OpenAPI root |
| `Swagger20` | `output_swagger2` | the document as Swagger 2.0 | an OpenAPI root |
| `JsonSchema` | `output_json_schema` | the component schemas as a JSON Schema 2020-12 bundle | |
| `AsyncApi` | `output_asyncapi` | the AsyncAPI 3 document | an AsyncAPI root |

The extension selects the format: `.json` writes JSON, anything else YAML.
Both contain the same document. `Spec::render(kind)` returns any output as a
`serde_json::Value` without writing.

## Writing

- A file is written only when its content changes, and atomically (a
  temporary file in the same directory, then renamed). An unchanged
  document does not touch the file, so crates that embed it are not
  rebuilt.
- Configured outputs are never read as inputs.
- `Spec::check()` (CLI `--check`) lists outputs whose file is missing or
  different, without writing.

## Determinism

The same sources produce byte-identical files on every platform and in any
file order: inputs are read in sorted order, the top-level keys follow the
specification order (`openapi`, `$self`, `info`, `jsonSchemaDialect`, `servers`,
`paths`, `webhooks`, `components`, `security`, `tags`, `externalDocs`), the
entries of every `components` section are sorted by name, and everything
else keeps the order of the sources. See
[ADR 0007](adr/0007-determinism-and-key-order.md).

## Versions

Schemas are generated in JSON Schema 2020-12, the dialect of OpenAPI 3.1 and
3.2. The `openapi` version of the root decides what the full document uses:

| Root | Schemas | Other differences |
| :--- | :--- | :--- |
| `3.2.x` | 2020-12 | everything is kept |
| `3.1.x` | 2020-12 | 3.2 features are removed with a warning (`OF6002`): `QUERY` and `additionalOperations`, `querystring` parameters, streamed responses |
| `3.0.x` | 3.0 (see below) | additionally webhooks are removed (`OF6002`) |

This applies to the full, schemas and paths outputs. The fragments output
always keeps 2020-12, because the crate that includes it lowers it for its
own root.

## OpenAPI 3.0

`output_openapi_30` (or a `3.0.x` root) converts the schemas:

| 2020-12 | 3.0 |
| :--- | :--- |
| `type: [T, "null"]`, `anyOf: [S, {type: "null"}]` | `type: T`, `nullable: true` (a reference is wrapped in `allOf`) |
| `type: [A, B]` | `anyOf` of both types |
| `const: v` | `enum: [v]` |
| `exclusiveMinimum: n` | `minimum: n`, `exclusiveMinimum: true` |
| `prefixItems` (tuples) | `items: { anyOf: [...] }` (reported) |
| `examples: [a, …]` | `example: a` |
| siblings of `$ref` | `allOf: [{ $ref }, siblings]` |
| other keywords (`$defs`, `unevaluatedProperties`, `propertyNames`, `contentEncoding`, …) | removed (reported, `OF6001`) |

Besides the 3.1/3.2 features above, the export removes what 3.0 does not
have, each reported once: `info.summary`, `license.identifier` (silently when
it equals the name), `jsonSchemaDialect`, `$self`, tag `summary`, `parent`
and `kind`, response `summary`, `components.pathItems` and `mediaTypes`, and
`mutualTLS` security schemes.

## Swagger 2.0

`output_swagger2` builds a Swagger 2.0 document from the 3.0 form:

- `servers[0]` becomes `host`, `basePath` and `schemes` (more servers:
  `OF6001`).
- `components` become `definitions`, `parameters`, `responses` and
  `securityDefinitions`; references are rewritten.
- Request bodies become a `body` parameter or `formData` parameters,
  media types become `consumes` and `produces`.
- Parameters and headers get their primitive keywords directly; object or
  reference schemas are documented as strings.
- Schema keywords Swagger lacks are kept as extensions: `nullable` as
  `x-nullable`, `oneOf`/`anyOf`/`not` as `x-oneOf`/…, `writeOnly` and
  `deprecated` as `x-writeOnly`/`x-deprecated`; the discriminator becomes a
  property name.

Left out or approximated, each reported once (`OF6001`, `OF6002`): cookie
parameters, `TRACE` operations, callbacks, response links, status ranges
(`2XX` becomes `200`), operation and path item `servers`, path item
`summary`/`description`, HTTP authentication other than basic (documented as
an `Authorization` header), OpenID Connect, mutual TLS, API keys in cookies,
more than one OAuth2 flow per scheme, `info.summary`, `license.identifier`
and tag extensions.

## JSON Schema

`output_json_schema` writes

```json
{ "$schema": "https://json-schema.org/draft/2020-12/schema", "$defs": { "User": { … } } }
```

with every component schema; references point into `$defs`. The bundle
validates against the 2020-12 meta-schema and suits code generators and
validators that do not read OpenAPI.

## Fragments

`output_fragments` writes the document without `openapi`, `info` and
`servers`, plus the generic templates and fragments for other crates; see
[cross-crate templates](templates.md#cross-crate-templates). It also works
when the crate has no root.

## Lints

Every run checks the document for what the official schemas cannot
express, with warnings:

| Code | Finding |
| :--- | :--- |
| `OF5001` | a `$ref` to a component that does not exist |
| `OF5002` | two operations with the same `operationId` |
| `OF5005` | a security requirement that names an undefined scheme |
| `OF5006` | a path placeholder without a path parameter, or the reverse |
| `OF5003` (strict) | a component nothing outside `components` reaches |
| `OF5004` (strict) | an operation without summary or description, a schema without description |

The strict lints run with `strict_lints(true)` or `--strict`. The test suite
also validates every corpus output against the official schemas for
OpenAPI 3.0, 3.1 and 3.2, Swagger 2.0, AsyncAPI 3.0 and 3.1, and the JSON
Schema 2020-12 meta-schema.
