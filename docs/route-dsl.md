# Route DSL

A `@route` line turns the doc comment of a function into an OpenAPI
operation. The other lines of the block describe it: plain lines are the
summary and description, lines that start with `@` are directives, and
standard OpenAPI YAML can be written in between.

```rust,ignore
/// List users
///
/// Returns one page of users, newest first.
/// @route GET /orgs/{org: String "Organization slug"}/users
/// @tag Users
/// @query-param page: u32 default=1 "Page number"
/// @return 200: $Page<User> "One page of users"
/// @problem 404 "No such organization"
pub async fn list_users() {}
```

The handler's name is the `operationId` (`list_users`). The block is
complete on its own; signature inference can add what the handler's
parameters say, see [below](#signature-inference).

## The block

- The first plain line is the `summary`; the following plain lines are the
  `description`. Blank lines separate paragraphs; Markdown and relative
  indentation are kept. `@summary text` and `@description text` set them
  explicitly (`@description` continues on the following plain lines).
- A directive line starts with `@`. Unknown directives are reported
  (`OF2006`) and ignored; malformed ones too (`OF2004`).
- A line that starts with a field of the Operation Object (`tags:`,
  `externalDocs:`, `operationId:`, `parameters:`, `requestBody:`,
  `responses:`, `callbacks:`, `deprecated:`, `security:`, `servers:`) or an
  extension (`x-…:`) starts embedded YAML, which runs until the next
  directive. `@openapi` starts YAML explicitly, see
  [embedded OpenAPI](#embedded-openapi). An `@extend` line merges a
  fragment into the operation without starting YAML: plain lines after it
  are still description.
- One block contains one `@route` or `@webhook`.

## Paths

`@route METHOD /path` takes any HTTP method. `GET`, `PUT`, `POST`,
`DELETE`, `OPTIONS`, `HEAD`, `PATCH`, `TRACE` and (OpenAPI 3.2) `QUERY` are
fields of the path item; other methods such as `PURGE` go to
`additionalOperations` (OpenAPI 3.2).

A placeholder can declare its parameter inline:

| Placeholder | Parameter |
| :--- | :--- |
| `{id}` | must be declared elsewhere (`@path-param`, `@path-params`, YAML or the handler signature) |
| `{id: u64}` | `id`, an integer |
| `{id: u64 "The user id"}` | with a description |
| `{slug "Readable id"}` | a string with a description |

Every placeholder needs a parameter and every path parameter a placeholder;
otherwise the route is rejected with an error (`OF2002`, `OF2003`). A path
that does not start with `/` gets one.

Without a function (in a `//!` file comment, a *virtual route*), the
operationId is derived from the method and path: `GET /users/{id}` becomes
`get_users_id`. `@operation-id` sets it explicitly. Routes on `impl`
methods work like functions.

## Directives

| Directive | Syntax | Result |
| :--- | :--- | :--- |
| `@route` | `METHOD /path` | The operation |
| `@webhook` | `[METHOD] name` | `webhooks.name` (OpenAPI 3.1+); the method defaults to `POST` |
| `@summary` | `text` | `summary` |
| `@description` | `text` | `description` |
| `@tag` | `Name[, Name…]` | `tags` (several lines add up) |
| `@deprecated` | | `deprecated: true` (also from `#[deprecated]` on the handler) |
| `@operation-id` | `id` | `operationId` |
| `@external-docs` | `URL ["description"]` | `externalDocs` |
| `@server` | `URL ["description"]` | an entry of `servers` |
| `@security` | `scheme[(scope, …)] [+ scheme…]` or `none` | an entry of `security` |
| `@param` | `LOCATION name: [Type] [flags] ["description"]` | a parameter in `path`, `query`, `header`, `cookie` or `querystring` |
| `@path-param`, `@query-param`, `@header-param`, `@cookie-param`, `@querystring-param` | `name: [Type] [flags] ["description"]` | the same, with the location in the name |
| `@path-params`, `@query-params`, `@header-params`, `@cookie-params` | `$Struct` | one parameter per field of a documented struct |
| `@body` | `[optional] Type [media…] ["description"]` | `requestBody` |
| `@return` | `CODE: [Type] [media…] ["description"]` | a response |
| `@return-header` | `CODE Name: Type ["description"]` | a header of that response |
| `@problem` | `CODE ["description"]` | an RFC 9457 `application/problem+json` response |
| `@no-infer` | | no [signature inference](#signature-inference) for this route |
| `@openapi` | `[operation \| path \| document]` | starts [embedded YAML](#embedded-openapi) |
| `@insert`, `@extend` | `Fragment[(args)]` | [fragments](templates.md#fragments) |

## Types

Wherever the DSL takes a type:

| Written | Schema |
| :--- | :--- |
| `$User` | a reference to the schema `User` (renamed types are found under their Rust name) |
| `User`, `models::User` | the same, as a Rust type |
| `u64`, `String`, `Uuid`, `Vec<$User>`, `Option<String>`, `HashMap<String, u32>` | as in the [type table](compatibility.md#types) |
| `$Page<User>`, `Page<User>` | the instantiated generic `Page_User` |
| `Json<T>`, `Form<T>`, `Box<T>` | `T` |
| `{ type: string, format: binary }` | inline YAML or JSON |
| `unit`, `()` or nothing | no body (for `@return`) |
| `stream<T>` | a stream of `T` (OpenAPI 3.2 `itemSchema`), for `@return` |
| `api-error` | a reference to the schema `api-error` (a schema name that is no Rust type) |

A built-in name written bare (`Duration`, `Value`, `Uuid`) refers to the
documented schema of that name when the sources define one, like a field
of a reflected struct ([own types with built-in names](schemas.md#types-from-other-crates));
a path (`std::time::Duration`) and a custom mapping keep the built-in or
mapped type. Text that is none of these, such as invalid inline YAML, is
reported (`OF2004`) and the directive ignored.

## Parameters

```text
@query-param name: Type [flags...] ["description"]
```

The type defaults to `String`. `Option<T>` makes a parameter optional;
otherwise query, header and cookie parameters are required unless they have
a default. Path parameters are always required.

| Flag | Effect |
| :--- | :--- |
| `required` | `required: true`, even with `Option` or a default |
| `deprecated` | `deprecated: true` |
| `default=VALUE` | `schema.default`; the parameter becomes optional |
| `example=VALUE` | `example` (quote values with spaces: `example="red shoes"`) |
| `enum=[a, b]` | `schema.enum` |
| `min=N`, `max=N` | `minimum`/`maximum`, or `minLength`/`maxLength` for strings |
| `pattern=REGEX` | `schema.pattern` |
| `format=NAME` | `schema.format` |
| `style=NAME`, `explode`, `explode=false` | serialization (`form`, `simple`, `spaceDelimited`, …) |
| `allow-reserved`, `allow-empty` | `allowReserved`, `allowEmptyValue` |

Values are YAML, so `default=10` is a number and `enum=[a, b]` a list.
Flags may come in any order, also after the description; an unknown flag is
reported and ignored.

`@query-params $Pagination` adds one query parameter per field of the
documented struct `Pagination` (field descriptions included; `Option`
fields are optional). Fields of `#[serde(flatten)]` structs are included; a
flattened `Option<T>` makes its fields optional. A path such as
`models::Pagination` names the struct by its last segment. It works for
`path`, `header` and `cookie` too. Parameters written explicitly win over
expanded ones with the same name.

A `querystring` parameter (OpenAPI 3.2) describes the whole query string
with a schema; it is written as
`content: { application/x-www-form-urlencoded: { schema } }`.

## Request bodies

```text
@body $NewUser
@body optional $Note text/plain "A note"
@body $Upload multipart/form-data
```

The media type defaults to `application/json`. Several media types can
follow the type, and several `@body` lines add media types to one body. A
body is required unless it is `optional` or an `Option<T>`.

## Responses

```text
@return 200: $User
@return 201: $User "Created"
@return 204:
@return 200: Vec<$Item> application/json application/x-ndjson "The items"
@return 2XX: $User
@return default: $Error "Unexpected error"
@return 200: stream<$Event> "Server-sent events"
@return-header 201 Location: String "URL of the new item"
@problem 404 "No such user"
```

- The description defaults to the reason phrase of the status (`OK`,
  `Created`, …). OpenAPI requires one.
- A response without type has no `content`, as `204` must not.
- Status ranges (`2XX`) and `default` are allowed.
- `stream<T>` uses `text/event-stream` unless a media type is given and
  documents the items with `itemSchema` (OpenAPI 3.2; older targets get an
  empty media type and a diagnostic).
- `@problem` adds a response with the media type `application/problem+json`
  and the `ProblemDetails` schema of RFC 9457 to `components.schemas`.
- An operation without responses gets `default: { description: Default response }`.

## Security

```text
@security bearer
@security oauth(read, write) + api_key
@security none
```

Each `@security` line is one alternative; `+` joins schemes that must all be
satisfied. `none` alone makes the operation public (`security: []`); next
to other lines it allows anonymous access (`{}`). The schemes must be defined
in `components.securitySchemes` (lint `OF5005`).

## Embedded OpenAPI

Any field the DSL has no directive for can be written as YAML in the block:

```rust,ignore
/// Upload an avatar
/// @route PUT /users/{id: u64}/avatar
/// @body { type: string, format: binary } image/png image/jpeg
/// @return 204:
/// callbacks:
///   processed:
///     '{$request.header.X-Callback}':
///       post:
///         responses: { '204': { description: Received } }
/// x-rate-limit: 10
pub async fn upload_avatar() {}
```

`@openapi` chooses where YAML goes:

| Block | Target |
| :--- | :--- |
| `@openapi` or `@openapi operation` | the Operation Object |
| `@openapi path` | the Path Item (`servers`, shared `parameters`, `summary`) |
| `@openapi document` | the whole document (e.g. `components` that only this route needs) |

The YAML is merged with what the directives produced: parameters by name and
location, responses by status code, everything else key by key. When both
set the same value differently, the YAML wins and a note is reported at the
YAML line that sets it. YAML that is not a mapping is reported (`OF2005`)
and ignored.

## Webhooks

`@webhook [METHOD] name` describes a request your API sends, with the same
directives as a route (OpenAPI 3.1 and later; the 3.0 and Swagger 2.0
exports leave webhooks out with a diagnostic):

```rust,ignore
/// An order was paid.
/// @webhook orderPaid
/// @body $Order
/// @return 204: "Acknowledged"
pub async fn order_paid() {}
```

## Module defaults

`@route-defaults` in the doc comment of a module applies to every route in
it, including nested modules:

<!-- corpus: guide_route_defaults -->
```rust
//! @openapi
//! openapi: 3.1.0
//! info: { title: Tenants, version: 1.0.0 }
//! components:
//!   securitySchemes:
//!     bearer: { type: http, scheme: bearer }

/// Tenant administration.
/// @route-defaults
/// @prefix /tenants/{tenant}
/// @path-param tenant: String "Tenant slug"
/// @tag Tenants
/// @security bearer
/// @problem 401 "Not signed in"
pub mod tenants {
    /// List users
    /// @route GET /users
    /// @return 200: Vec<String>
    pub fn list_users() {}

    /// Billing endpoints.
    /// @route-defaults
    /// @prefix /billing
    /// @tag Billing
    pub mod billing {
        /// Get the invoice
        /// @route GET /invoices/{id: u64}
        /// @security none
        /// @return 200: String
        pub fn invoice() {}
    }
}
```

`@prefix` is prepended to every path; nested prefixes add up, joined with
exactly one `/`, and may declare parameters inline
(`@prefix /tenants/{tenant: String "Tenant slug"}`). The route `/` under a
prefix is the prefix itself (`/tenants/{tenant}`, without trailing slash);
other paths keep a trailing slash they are written with. Parameters, responses
and other directives are added to each route unless the route sets the same
itself: the route's tags, security and responses win. Likewise, the `@tag`
and `@security` lines of an inner module replace those of the outer modules
(in the example, `invoice` is tagged `Billing` only); within one module,
several lines add up. Webhooks get no path parameters from the defaults.

The defaults are a block like a route's: `{{variables}}`, `@insert` and
`@extend` work, `@no-infer` turns signature inference off for every route
of the module, and embedded YAML (`x-rate-limit: 10`, `@openapi path`, …)
fills what the route leaves open; YAML and `@extend` of an inner module win
over those of outer ones. Each module's defaults are expanded once, so a
problem in them (an unset `{{env:…}}`, an unknown fragment) is reported once,
at its line. Module doc lines of the form `tags: [A, B]` (from 0.1) still
tag the operations inside.

## Signature inference

For axum and actix-web handlers, the signature fills what the block leaves
open:

| Extractor | Adds |
| :--- | :--- |
| `Path<T>`, `web::Path<T>` | the types of undeclared path parameters (a tuple in path order; a struct is expanded); `Option<Path<T>>` the same (path parameters are always required) |
| `Query<T>` | one query parameter per field of `T`; in `Option` they are all optional |
| `Json<T>`, `Form<T>` | the request body (`application/json`, `application/x-www-form-urlencoded`); in `Option` it is optional |
| `Multipart` | a `multipart/form-data` body |
| `-> Json<T>`, `-> Result<Json<T>, E>`, `-> (StatusCode, Json<T>)` | the `200` response |

```rust,ignore
/// Update a user
/// @route PUT /users/{id}
pub async fn update_user(
    Path(id): Path<u64>,
    Json(body): Json<UpdateUser>,
) -> Result<Json<User>, ApiError> {
    todo!()
}
```

Everything written in the block (and its module defaults) wins: inference
only fills gaps. The `200` response is inferred unless the block documents a
`2XX` response; a `default` response, usually the error, does not count.
`Query<T>` is expanded unless the block has `@query-params` or a
`querystring` parameter; single query parameters written in the block win
over expanded ones of the same name. Extractor types may be paths
(`axum::extract::Query<models::Paging>`). A path extractor that does not
fit the placeholders is reported (`OF2007`).
`@no-infer` turns inference off for one route,
`Generator::infer_signatures(false)` for all.

## A complete example

<!-- corpus: guide_route_dsl -->
```rust
use serde::{Deserialize, Serialize};

/// @openapi
/// openapi: 3.2.0
/// info: { title: Shop, version: 2.0.0 }
/// components:
///   securitySchemes:
///     oauth:
///       type: oauth2
///       flows:
///         clientCredentials:
///           tokenUrl: https://auth.example.com/token
///           scopes: { read: Read items, write: Change items }
pub fn api() {}

/// An item for sale.
/// @openapi
#[derive(Serialize, Deserialize)]
pub struct Item {
    pub id: u64,
    pub name: String,
}

/// How to page through results.
/// @openapi
#[derive(Deserialize)]
pub struct Paging {
    /// Page number, starting at 1.
    pub page: Option<u32>,
    /// Items per page.
    pub size: Option<u8>,
}

/// List items
/// @route GET /items
/// @tag Items
/// @query-params $Paging
/// @query-param sort: String enum=[name, price] default=name "Sort order"
/// @return 200: Vec<$Item> application/json application/x-ndjson "The items"
/// @return-header 200 X-Total-Count: u64 "Number of items"
/// @security oauth(read)
pub async fn list_items() {}

/// Add an item
/// @route POST /items
/// @tag Items
/// @body $Item
/// @return 201: $Item "Created"
/// @problem 409 "An item with this name exists"
/// @security oauth(write)
pub async fn add_item() {}

/// Watch the stock
/// @route GET /items/{id: u64}/stock
/// @tag Items
/// @return 200: stream<u32> "Stock levels as server-sent events"
/// @security none
pub async fn stock() {}

/// Search items
/// @route QUERY /items
/// @tag Items
/// @body { type: object, properties: { text: { type: string } } }
/// @return 200: Vec<$Item>
pub async fn search() {}

/// An item was sold out.
/// @webhook soldOut
/// @body $Item
/// @return 204:
pub async fn sold_out() {}
```
