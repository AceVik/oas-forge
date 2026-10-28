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
  indentation are kept (the shared indentation is removed).
- `@summary text` sets the summary; then no plain line is the summary, and
  all plain lines (the first one too) are the description.
- `@description text` replaces the plain lines as description. It continues
  on the following plain lines up to the next directive (there, lines such
  as `responses: …` are text, not YAML); several `@description` lines add
  up.
- `@internal` leaves the plain lines after it, up to the next directive,
  out of the document: notes for developers, anywhere in the block (also
  before `@route`), that stay in the Rust docs only.

  ```rust,ignore
  /// `POST /decks`: creates a deck.
  ///
  /// @internal
  /// We store the version first because of #312.
  /// @route POST /decks
  /// @summary Create a deck
  /// @description Creates a deck for the signed-in player.
  /// The name must be unique per player.
  /// @return 201: $Deck
  pub async fn create_deck() {}
  ```
- A directive line starts with `@`. Unknown directives are reported
  (`OF2006`) and ignored; malformed ones too (`OF2004`).
- Arguments may be written in Markdown backticks, so that the annotations
  read as code in rustdoc and pass `clippy::doc_markdown`:
  `` @return 404: `$ErrorBody` "No such deck" `` is the same as
  `@return 404: $ErrorBody "No such deck"`. A backtick at the start of a
  token wraps everything up to the next backtick before whitespace, `:` or
  `,` (`` @route GET `/decks/{id: u64 "Deck id"}` ``, a whole inline schema
  `` `{ type: integer }` `` or flag `` `default=20` ``). A flag value
  (`` default=`20` ``) and the items of a list (`` enum=[`crop`, `full`] ``)
  may be in backticks too, and the URL of `@server` and `@external-docs` an
  autolink (`<https://docs.example.com>`). Backticks inside quoted
  descriptions and in `@summary` and `@description` stay Markdown.
- Descriptions in directives are quoted: `"No such deck"`. Inside the
  quotes `\"` is a quote and `\\` a backslash; everything else, Markdown
  included, is kept as written (`` "not `{\"enabled\": true}`" ``).
- A line that starts with a field of the Operation Object (`tags:`,
  `externalDocs:`, `operationId:`, `parameters:`, `requestBody:`,
  `responses:`, `callbacks:`, `deprecated:`, `security:`, `servers:`) or an
  extension (`x-…:`) starts embedded YAML, which runs until the next
  directive, or until a blank line followed by prose: after a blank line,
  a line at the indentation of the first YAML line (or less) that is no
  `key:` line and no `#` comment, such as a paragraph or a Markdown list
  (`- item`), ends the YAML and continues the description as a new
  paragraph. More deeply indented lines are still YAML. Lines that
  continue a `@description` never start YAML. `@openapi` starts YAML
  explicitly (it runs up to the next directive, prose or not), see
  [embedded OpenAPI](#embedded-openapi). An `@extend` line merges a
  fragment into the operation without starting YAML: plain lines after it
  are still description (or still left out after `@internal`).
- A block may contain several `@route` and `@webhook` lines, for a handler
  that serves several paths: each line gets the operation, with the inline
  parameters of its own line. With one line, the operationId is the
  handler's name; with several, each gets the name, the method and the path
  (`@route OPTIONS /client/alive` on `preflight` is
  `preflight_options_client_alive`), and `@operation-id` is reported and
  ignored, as it cannot be unique.
- [`{{variables}}`](templates.md#variables) work on every line. A
  placeholder that is a whole value is quoted only in embedded YAML
  (`x-since: {{VERSION}}` stays a string); in the summary, the description
  and directive arguments the value is inserted as it is.

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
| `@summary` | `text` | `summary`; all plain lines are the description |
| `@description` | `text` | `description`, instead of the plain lines |
| `@internal` | | the plain lines after it are left out |
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
| `@return-header` | `CODE Name: [Type] [required \| optional] ["description"]` | a header of that response |
| `@problem` | `CODE ["description"]` | an RFC 9457 `application/problem+json` response |
| `@no-infer` | | no [signature inference](#signature-inference) for this route |
| `@scope` | `name[, name…]` | the documents the route belongs to, see [scopes](#scopes) |
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

The type defaults to `String`.

**A query, header or cookie parameter is required unless its type is
`Option<T>`, it has a `default=…` or it is marked `optional`**, as axum
rejects a request that lacks a field that is not an `Option`. Path
parameters are always required (`optional` on one is reported).

| Flag | Effect |
| :--- | :--- |
| `required` | `required: true`, even with `Option` or a default |
| `optional` | `required: false`, whatever the type (`required` wins over it, with a finding) |
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
documented struct `Pagination` (`Option` fields are optional). A field's
description, `deprecated` (`#[deprecated]` or `@openapi deprecated: true`)
and examples (`@example`) become the parameter's, not its schema's. Fields of `#[serde(flatten)]` structs are included; a
flattened `Option<T>` makes its fields optional. A path such as
`models::Pagination` names the struct by its last segment. It works for
`path`, `header` and `cookie` too. Parameters written explicitly win over
expanded ones with the same name. The parameters that expansion adds to a
document share 16 MiB (each costs 256 besides its JSON text), and
collecting the fields of a document's expansions visits at most 4,194,304
schemas, properties and required names; an expansion beyond either is
left out and reported (`OF3016`).

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
- A response header (`@return-header`) is optional, as in OpenAPI, unless
  the flag `required` follows the type (`@return-header 201 Location: String
  required "URL of the new item"`); `optional` may be written for clarity.
  The type defaults to `String`.
- Status ranges (`2XX`) and `default` are allowed.
- `stream<T>` uses `text/event-stream` unless a media type is given and
  documents the items with `itemSchema` (OpenAPI 3.2; older targets get an
  empty media type and a diagnostic).
- `@problem` adds a response with the media type `application/problem+json`
  and the `ProblemDetails` schema of RFC 9457 to `components.schemas`.
- An operation without responses gets `default: { description: Default response }`.
- Each status has one response. Lines with the same status (also from
  `@insert`ed fragments, [module defaults](#module-defaults) and embedded
  YAML) add their media types, and their descriptions are combined in
  source order (module defaults first), without repeats: one stays as it
  is, several become a Markdown list. A line without description adds
  none. A later line with another schema for a media type replaces it.

  ```text
  @insert SessionRefusals              ← @return 403: $Error "The account is banned"
  @return 403: $Error "The password is wrong"
  ```

  gives one `403` response described as `- The account is banned` and
  `- The password is wrong`.

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
Repeating a scheme within one line combines its scopes without duplicates.
A `+` inside parentheses belongs to the scope; quote a scope containing a
comma, for example `oauth("read,write")`.

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

Right after `@openapi`, the YAML may be a fenced code block
(```` ```yaml ````, ```` ``` ````, `~~~`) for rustdoc: the fences are removed
and the YAML ends at the closing fence; plain lines after it are prose
again.

The YAML is merged with what the directives produced: parameters by name and
location, responses by status code, everything else key by key. When both
set the same value differently, the YAML wins and a note is reported at the
YAML line that sets it. The description of a response is the exception: it
is combined with the descriptions of the directives (see
[responses](#responses)), unless the YAML gives one of their media types
another content. YAML that is not a mapping is reported (`OF2005`)
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
it, including nested modules (for modules in files of their own see
[below](#modules-in-their-own-files)):

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
itself: the route's tags and security win (also over `security:` in the
YAML of the defaults). A response for a status the route
has too is combined with the route's (media types add up, descriptions of
the defaults first, see [responses](#responses)); when both give one media
type a different schema, the route's response wins and a note (`OF4001`)
says so. A `@return-header` of the route replaces the one of the defaults
with the same status and name (compared without regard to case), schema
and all. Likewise, the `@tag`
and `@security` lines of an inner module replace those of the outer modules
(in the example, `invoice` is tagged `Billing` only); within one module,
several lines add up. Webhooks get no path parameters from the defaults.

The defaults are a block like a route's: `{{variables}}`, `@insert` and
`@extend` work, `@no-infer` turns signature inference off for every route
of the module, and embedded YAML (`x-rate-limit: 10`, `@openapi path`, …)
fills what the route leaves open; YAML and `@extend` of an inner module win
over those of outer ones. Each module's defaults are read once for all its
routes, so a problem in them (an unset `{{env:…}}`, an unknown fragment, a
malformed directive, invalid YAML) is reported once, at its own line, also
when the module is in another file. Module doc lines of the form
`tags: [A, B]` (from 0.1) still tag the operations inside.

### Modules in their own files

A module in a file of its own takes its defaults in either place: on the
declaration (`/// @route-defaults` above `mod name;`) or at the top of the
module's file (`//! @route-defaults`, which also works inside an inline
`mod name { … }`). They nest along the module tree like inline modules,
also through files without routes:

```text
src/lib.rs            /// @route-defaults       ← everything in routes/
                      /// @security bearer
                      pub mod routes;

src/routes/mod.rs     //! @route-defaults       ← this file and its modules
                      //! @prefix /v1
                      /// @route-defaults       ← routes/decks.rs only
                      /// @tag Decks
                      pub mod decks;
                      pub mod social;

src/routes/decks.rs   /// @route GET /decks     → GET /v1/decks, Decks, bearer

src/routes/social.rs  //! @route-defaults
                      //! @prefix /social
                      //! @tag Social
                      /// @route GET /devices   → GET /v1/social/devices, Social, bearer
```

The module's file is found the way Rust finds it: `name.rs` or
`name/mod.rs` next to `lib.rs`, `main.rs` or a `mod.rs`, in a directory
named after any other file (`social.rs` declares `social/feed.rs`), below
the directories of inline modules, or the file of a `#[path]` attribute.
The files are linked after all of them are read, so their order does not
matter. In a file comment, the defaults end at the next block header
(`@openapi`, `@route`, …), separator line or gap in the comment.

## Scopes

One source set can describe several APIs: a public listener and an admin
listener of one service, or several services read in one run. `@scope`
assigns a block to the documents of the named scopes:

```rust,ignore
/// @openapi
/// @scope admin
/// openapi: 3.1.0
/// info: { title: Gateway administration, version: 1.0.0 }
/// servers: [{ url: "http://127.0.0.1:9000" }]
fn admin_root() {}

/// The loopback admin listener.
/// @route-defaults
/// @scope admin
/// @security admin_token
pub mod admin {
    /// List reports
    /// @route GET /reports
    /// @return 200: Vec<$Report>
    pub async fn list_reports() {}
}
```

- `@scope name[, name…]` works in route and webhook blocks, in
  `@route-defaults` (inherited like `@tag`: an inner module's `@scope`
  replaces the outer one, the route's own replaces the module's), in
  `@channel` blocks and as a line of a YAML block (`@openapi`, `@asyncapi`,
  root blocks included), so each scope can have its own root with its own
  title and servers. Names consist of letters, digits, `.`, `-` and `_`;
  several lines add up. When the YAML is a fenced code block, the `@scope`
  lines may stand between the header and the opening fence or right after
  the closing fence (`@openapi document` blocks on types included).
- A block without `@scope` is shared: it is part of every scope's document.
  A scope whose document gets no root because the only root is scoped
  elsewhere loses the root's global `security` and `servers`; that is
  reported (`OF4002`).
  Reflected types, `@openapi-type` schemas, templates, fragments and YAML
  files are always shared.
- The documents of the scopes are merged separately, so the same path may
  exist in several scopes (`GET /health` of two services) without a
  conflict. Each scoped document keeps only the components that its
  operations reach (security schemes and `x-` sections stay) and the
  top-level tags they use, with their parents.
- [`output_scoped(scope, kind, path)`](outputs.md#scopes) writes the
  document of a scope; `scope(name)`, the configuration key `scope` and
  the CLI flag `--scope` select the scope of every other output. Without a
  scope, every block is merged into one document, `@scope` or not (two
  scoped roots are then an error).

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
