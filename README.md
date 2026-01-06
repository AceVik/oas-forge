# OpenAPI Specs Forge (oas-forge)

[![Crates.io](https://img.shields.io/crates/v/oas-forge.svg)](https://crates.io/crates/oas-forge)
[![Docs.rs](https://docs.rs/oas-forge/badge.svg)](https://docs.rs/oas-forge)
[![License](https://img.shields.io/crates/l/oas-forge.svg)](LICENSE)
[![Build Status](https://github.com/AceVik/oas-forge/actions/workflows/release.yml/badge.svg)](https://github.com/AceVik/oas-forge/actions)
[![Latest Release](https://img.shields.io/github/v/release/AceVik/oas-forge)](https://github.com/AceVik/oas-forge/releases/latest)

**The zero-runtime OpenAPI 3.1 compiler for Rust.**

`oas-forge` extracts, links, and merges code-first documentation into a standard `openapi.yaml` file at compile time. It eliminates the need for runtime macros that bloat your binary and crash on startup.

### 🏗️ Architecture

```ascii
[Source Code] --> [Scanner] --> [AST Parsing] --> [Registry]    
                                                     |
                                                     v
[Static YAML] --> [Merger] <--- [Monomorphizer] <--- [Fragments]
                       |
                       v
                 [openapi.yaml]
```
**Note:** `oas-forge` is in early development and not tested on wide field. 
**There may be bugs.** The Route DSL and Type Reflection features are actively evolving. Feedback and contributions are welcome!
---

## 🛠️ Integration Guide

```toml
[build-dependencies]
oas-forge = "0.1"
```

### Method A: build.rs (Fluent API)
```rust,no_run
use oas_forge::Generator;

println!("cargo:rerun-if-changed=src");
println!("cargo:rerun-if-changed=Cargo.toml");

Generator::new()
    .input("src") // Recursive input directory (where to search for doc comments)
    .input("lib") // Multiple inputs supported
    .include("static/security.yaml") // Merge static oas content - optional
    .include("static/skeleton.json") // Multiple includes supported (yaml and json supported)
    .output("openapi.yaml") // Full Spec (Strict: requires openapi/info root)
    // This is for lib exports to be used in other projects (in .include(...))
    .output_fragments("fragment.json") // Headless Spec (Paths + Components, no Root)
    // More fine-grained outputs (Relaxed: no validation)
    .output_schemas("schemas.json") // Components/Schemas only
    .output_paths("routes.yaml") // Paths only
    .generate()
    .expect("Failed to generate OpenAPI spec");

    // Note: Output file extension determines format (yaml/json)
```

### Method B: Cargo.toml Metadata
Configure in your manifest to keep `build.rs` minimal.

**Cargo.toml**:
```toml,ignore
# Same possibilities as Method A, but configured via Cargo.toml
[package.metadata.oas-forge]
input = ["src", "lib"]
include = ["static/skeleton.yaml"]

# Full Specs
output = ["openapi.yaml"]

# Granular Exports
output_fragments = ["dist/lib-spec.yaml"]
output_schemas = ["frontend/api-types.json"]
output_paths = ["gateway/routes.yaml"]
```

**build.rs**:
```rust,ignore
use oas_forge::{config::Config, Generator};

fn main() {
  println!("cargo:rerun-if-changed=src");
  println!("cargo:rerun-if-changed=Cargo.toml");
  
  let config = Config::load();

  if let Err(e) = Generator::new()
    // Use Cargo.toml metadata
    .with_config(config)
    .generate()
  {
    eprintln!("Warning: Failed to generate OpenAPI docs: {}", e);
  }
}
```

### Method C: CLI
Ideal for CI/CD pipelines.

```bash,ignore
cargo install oas-forge
# Run generation
oas_forge -i src -o openapi.yaml 
oas_forge -i src -I static/openapi.yaml -o openapi.yaml
oas_forge -i src -i lib -I static/skeleton.yaml -I static/security.json -o openapi.yaml

# You get the idea. More complex example:
oas-forge \
  -i src \
  --output openapi.yaml \
  --output-fragments dist/lib.yaml \
  --output-schemas types.json \
  --output-paths routes.yaml
```

---

## ✨ Feature Reference

### ⚡ Route DSL

The Route DSL allows you to define API operations directly above your handler functions. This avoids verbose YAML and keeps your code clean.

#### Basic Example

```rust,ignore
/// List Users
/// 
/// Returns a paginated list of users.
///
/// @route GET /users
/// @tag Users
/// @query-param page: Option<u32> "Page number"
/// @return 200: $Vec<User> "List of users"
async fn list_users() { ... }
```

#### Advanced Features

**1. Inline Path Parameters**
Define path parameters, their types, and descriptions directly in the route string.

```rust,ignore
/// @route GET /users/{id: u32 "The unique ID"}
fn get_user(id: u32) { ... }
```
* **Result**: Automatically registers `id` as `in: path`, `required: true`, with `schema: {type: integer, format: int32}`.

**2. Flexible Parameter Syntax**
Define path, query, header, or cookie parameters (`@path-param`, `@query-param`, `@header-param`, `@cookie-param`). Attributes like `deprecated`, `required`, or `example` can be placed in any order after the type.
Path parameters are name validated against the route path.

```rust,ignore
/// @route GET /search
/// @query-param filter: Option<String> deprecated example="Alice" "Filter by name"
/// @header-param X-Trace-ID: Uuid required "Tracing ID"
fn search() { ... }
```
* `deprecated`: Sets `deprecated: true`.
* `example="Alice"`: Sets `example: "Alice"`.
* `Option<T>`: Infers `required: false` (unless `required` flag is explicitly present).

**3. Smart Responses (`@return`)**
The DSL infers schemas and handles generics automatically.

```rust,ignore
/// @route POST /users
/// @return 201: $User "Created"          <- JSON Ref to User schema
/// @return 200: $Page<User> "List"       <- Monomorphized Generic ($Page<User> -> Page_User)
/// @return 204: "Deleted"                <- Unit Type (No Content body)
/// @return 400: "Invalid Input"          <- String Literal (Description only)
fn create_user() { ... }
```

**4. Request Body (`@body`)**
Link a Struct as the request body. Defaults to `application/json` if no MIME type is specified.

```rust,ignore
/// @route POST /users
/// @body $CreateRequest application/json
fn create() { ... }
```

**5. Security (`@security`)**
Apply security schemes defined in your root spec or fragments.

```rust,ignore
/// @route GET /protected
/// @security oidc("read", "write")
/// @security basic()
fn protected() { ... }
```

**6. Mixing Raw YAML (Overrides)**
You can mix standard OpenAPI YAML attributes directly within the DSL block. This is useful for complex scenarios or when using `@insert` with fragments containing bare YAML keys.
Supported top-level keys: `parameters`, `requestBody`, `responses`, `security`, `callbacks`, `externalDocs`, `servers`.

```rust,ignore
/// @route GET /complex
/// @tag Items
///
/// # You can inject raw parameters alongside DSL
/// parameters:
///   - name: raw_param
///     in: query
///     schema: { type: string }
///
/// # Or override the description
/// externalDocs:
///   url: https://example.com/docs
///   description: More info
fn complex_handler() {}
```

### 🏛️ Legacy / Manual Mode
You don't have to use the DSL. `oas-forge` fully supports "Old School" OpenAPI definitions where you simply write raw YAML in your doc comments. This gives you full control.

```rust,ignore
/// @openapi
/// paths:
///   /manual/endpoint:
///     get:
///       tags: [Manual]
///       summary: Fully manual definition
///       description: This is standard OpenAPI YAML.
///       responses:
///         '200':
///           description: OK
///           content:
///             text/plain:
///               schema: { type: string }
async fn manual_handler() {}
```

#### Integration Example: Serving with Axum
Here is a complete pattern for serving your dynamic OpenAPI spec and Swagger UI using `axum`.

```rust,ignore
use axum::{
    Router,
    http::header,
    response::{Html, IntoResponse},
    routing::get,
};
use std::env;

// Embed the generated file
const OPENAPI_SPEC: &str = include_str!("../../openapi.yaml");

pub fn router() -> Router {
    Router::new()
        .route("/openapi.yaml", get(serve_spec))
        .route("/swagger", get(serve_ui))
}

/// @openapi
/// paths:
///   /docs/openapi.yaml:
///     get:
///       tags: [System]
///       summary: Get OpenAPI Specification
///       description: Returns the dynamic OpenAPI specification.
///       responses:
///         '200':
///           description: The OpenAPI YAML file.
///           content:
///             application/yaml:
///               schema: { type: string }
async fn serve_spec() -> impl IntoResponse {
    let spec = OPENAPI_SPEC.replace("$$OIDC_URL", &env::var("OIDC_URL").unwrap_or_default());
    ([(header::CONTENT_TYPE, "application/yaml")], spec)
}

async fn serve_ui() -> impl IntoResponse {
    let html = r#"<!DOCTYPE html>
<html lang="en">
<head>
  <title>Swagger UI</title>
  <link rel="stylesheet" href="https://unpkg.com/swagger-ui-dist/swagger-ui.css" />
</head>
<body>
  <div id="swagger-ui"></div>
  <script src="https://unpkg.com/swagger-ui-dist/swagger-ui-bundle.js"></script>
  <script>
    window.onload = () => {
      SwaggerUIBundle({
        url: '/docs/openapi.yaml',
        dom_id: '#swagger-ui',
        presets: [SwaggerUIBundle.presets.apis, SwaggerUIBundle.SwaggerUIStandalonePreset],
        layout: "BaseLayout",
      });
    };
  </script>
</body>
</html>"#;
    Html(html)
}
```

### 📦 Schema Extraction
Annotate your data structures, enums and types with doc comments to generate OpenAPI schemas.

```rust,ignore
// Defines a custom PhoneNumber type. (As example for types from other crates.)
//! @openapi-type PhoneNumber
//! type: string
//! format: tel
//! pattern: "^\\+?[1-9]\\d{1,14}$"
//! description: E.164 formatted phone number.
//! example: "+491701234567"

/// E-Mail address.
/// @openapi
/// example: "johnd@mail.com"
/// format: email
pub type Email = String;

/// Description of user roles within the system.
/// @openapi
pub enum Role {
  Admin,
  Moderator,
  User,
}

/// Description of a User entity.
/// @openapi
pub struct User {
  pub id: u32,

  /// The username of the user.
  /// @openapi example: "johndoe"
  pub username: Option<String>,

  /// The email address of the user.
  /// @openapi example: "johndoe@mail.com"
  pub email: Email,

  pub phone: PhoneNumber,

  /// The role assigned to the user.
  /// @openapi default: "User"
  pub role: Role,

  pub is_active: bool,

  /// The timestamp when the user was created.
  /// @openapi example: "2024-01-01T12:00:00"
  pub created_at: chrono::NaiveDateTime,
}
```
**generates**:
```yaml,ignore
components:
  schemas:
    PhoneNumber:
      type: string
      format: tel
      pattern: ^\+?[1-9]\d{1,14}$
      description: E.164 formatted phone number.
      example: '+491701234567'
    Role:
      description: Description of user roles within the system.
      enum:
        - Admin
        - Moderator
        - User
      type: string
    Email:
      description: E-Mail address.
      example: johnd@mail.com
      format: email
      type: string
    User:
      description: Description of a User entity.
      properties:
        created_at:
          description: The timestamp when the user was created.
          example: 2024-01-01T12:00:00
          format: date-time
          type: string
        email:
          $ref: '#/components/schemas/Email'
          description: The email address of the user.
          example: johndoe@mail.com
        id:
          format: int32
          type: integer
        is_active:
          type: boolean
        phone:
          $ref: '#/components/schemas/PhoneNumber'
        role:
          $ref: '#/components/schemas/Role'
          default: User
          description: The role assigned to the user.
        username:
          description: The username of the user.
          example: johndoe
          type: string
      required:
        - id
        - email
        - phone
        - role
        - is_active
        - created_at
      type: object
``` 
**Note:**
- If a property type is `Option<T>`, it is considered optional in the schema and it is not listed under `required`.
- Primitive types are automatically mapped to their OpenAPI equivalents (e.g., `u32` to `integer` with `format: int32`).
- Custom types annotated with `@openapi-type` can be defined for more complex schema definitions.
- Enums are represented as string enums in the schema.
- Doc comments can include additional OpenAPI attributes like `example`, `format`, `pattern`, and `description`.
- The generator supports common Rust types and can be extended for more complex scenarios.

### 🏷️ Renaming & Implicit Export Safety (v0.1.2+)

**Implicit Safety:** Enums now require the `@openapi` tag to be exported to the schema. Enums without this tag are ignored, even if public.

**Renaming:** You can rename fields, variants, and structs using **Serde** attributes or **@openapi** directives.
**Precedence Order:**
1. Manual `@openapi rename` / `@openapi rename-all`
2. `#[serde(rename = "...")]` / `#[serde(rename_all = "...")]`
3. Default Rust Name

#### Example: Renaming & Serde Support

```rust,ignore
/// @openapi
/// @openapi rename-all camelCase
#[derive(serde::Serialize)] // Optional, used for reference
pub enum UserRole {
    /// @openapi rename "admin_user"
    Admin,
    Moderator, // -> "moderator" (camelCase)
    User       // -> "user"
}

/// @openapi rename "UserProfile"
#[serde(rename_all = "snake_case")]
pub struct Profile {
    pub first_name: String, // -> "first_name"
    
    /// @openapi rename "lastName"
    pub last_name: String,  // -> "lastName" (override)
}
```

### 🧬 Template schemas with generics
Define reusable schema templates with generics using the `$` prefix.

```rust,ignore
/// Paginated response wrapper.
/// @openapi<T>
pub struct PaginatedResponse<T> {
  /// @openapi example: 100
  pub total_count: u32,

  /// @openapi
  /// example: 1
  /// minimum: 1
  pub page: u32,

  /// @openapi
  /// example: 10
  /// minimum: 1
  /// maximum: 1000
  pub limit: u32,

  pub data: Vec<T>,
} 
```

**Usage:**
```rust,ignore
// More about this in the Route DSL section below.
/// @return 200: $PaginatedResponse<User> "Success"
```
**generates**:
```yaml,ignore
components:
  schemas:
    PaginatedResponse_User:
      description: Paginated response wrapper.
      properties:
        data:
          items:
            $ref: '#/components/schemas/User'
          type: array
        limit:
          example: 10
          format: int32
          maximum: 1000
          minimum: 1
          type: integer
        page:
          example: 1
          format: int32
          minimum: 1
          type: integer
        total_count:
          example: 100
          format: int32
          type: integer
      required:
      - total_count
      - page
      - limit
      - data
      type: object 
```
**Note:**
- The Monomorphizer pass generates concrete schemas for each unique instantiation of the generic template (e.g., `$PaginatedResponse<User>` becomes `PaginatedResponse_User`).
- Multiple generic parameters are supported (e.g., `$Result<T, E>` => `Result_T_E`).

### 🌳 Root Documentation
Every OpenAPI specification needs a root definition containing metadata like the API version, title, and global security schemes. `oas-forge` requires exactly one such root definition in your project.
You can define this using a standard `@openapi` block, typically on a unit struct or at the top of your `main.rs` / `lib.rs`.

#### 1. Basic Root Definition
The root definition must contain the `openapi` version and the `info` object.

```rust,ignore
/// @openapi
/// openapi: 3.1.0
/// info:
///   title: My Awesome API
///   version: 1.0.0
///   description: >
///     This is the main entry point for the API documentation.
///     You can use Markdown here.
``` 

#### 2. Variable Substitution

`oas-forge` automatically injects environment variables into your documentation. The most common use case is syncing the API version with your crate version.

* `{{CARGO_PKG_VERSION}}`: Replaced by the version from `Cargo.toml`.

```rust,ignore
/// @openapi
/// openapi: 3.1.0
/// info:
///   title: Starr API
///   # Automatically uses the version from Cargo.toml
///   version: {{CARGO_PKG_VERSION}}
```

#### 3. Defining Global Security Schemes

The root definition is the perfect place to define `securitySchemes` (Components) and global `security` requirements.

```rust,ignore
/// @openapi
/// openapi: 3.1.0
/// info:
///   title: Secure API
///   version: 1.0.0
/// components:
///   securitySchemes:
///     # Define a JWT Bearer scheme
///     BearerAuth:
///       type: http
///       scheme: bearer
///       bearerFormat: JWT
///     # Define an OAuth2/OIDC scheme
///     OidcAuth:
///       type: openIdConnect
///       openIdConnectUrl: [https://auth.example.com/.well-known/openid-configuration](https://auth.example.com/.well-known/openid-configuration)
/// # Apply BearerAuth globally to all routes (optional)
/// security:
///   - BearerAuth: []
```

#### 4. Attaching to Application Logic (Recommended)

You don't need to create a "lifeless" dummy struct just for documentation. You can attach the root definition directly to your main application entry point or router function. This keeps the documentation close to the actual code logic.

```rust,ignore
/// @openapi
/// openapi: 3.1.1
/// info:
///   title: User Management API
///   version: {{CARGO_PKG_VERSION}}
///   description: API documentation for the user service.
/// components:
///   securitySchemes:
///     oidcAuth:
///       type: openIdConnect
///       openIdConnectUrl: $$OIDC_URL/.well-known/openid-configuration
///       description: OIDC Authentication via Rauthy.
pub fn create_app() -> Router<AppState> {
  Router::new()
    .nest("/docs", swagger::router())
    .nest("/users", users::router())
}
```

### 🧩 Fragments & Mixins
Fragments allow you to define reusable OpenAPI snippets (like common error responses, standard parameters, or security schemes) and inject them into your operation definitions.

#### 1. Defining Fragments
Fragments are usually defined in a shared file (e.g., `lib.rs` or `docs.rs`) using module-level comments (`//!`). They can accept parameters using the `{{variable}}` syntax.

```rust,ignore
//! ---------------------------------------------------------------------------
//! A. Fragment for a List (e.g., Parameters)
//! ---------------------------------------------------------------------------
//! @openapi-fragment PaginationParams
//! - name: page
//!   in: query
//!   description: Page number
//!   schema: { type: integer, default: 1 }
//! - name: limit
//!   in: query
//!   schema: { type: integer, default: 10 }

//! ---------------------------------------------------------------------------
//! B. Fragment for an Object (e.g., Responses or Security)
//! ---------------------------------------------------------------------------
//! @openapi-fragment Secured(role)
//! security:
//!   - oidcAuth: [ {{role}} ]
//! responses:
//!   '401':
//!     description: Unauthorized
//!   '403':
//!     description: Forbidden - Requires {{role}} role
``` 
#### 2. Usage: `@insert` vs. `@extend`
While both keywords inject content, they work at different levels. Choosing the right one is crucial for valid YAML generation.

| Keyword | Mechanism | Best Used For | Behavior |
| :--- | :--- | :--- | :--- |
| **`@insert`** | **Textual Substitution** | **Lists / Arrays** | Acts like "Copy & Paste". Replaces the line with the fragment text *before* parsing. Essential for inserting items into a YAML list (like `parameters`). |
| **`@extend`** | **Deep Merge** | **Objects / Maps** | Parses the fragment as YAML and deeply merges it into the current structure. Ideal for adding fields to a struct or responses to an operation. |

#### Example: Using `@insert` (For Lists)

Use `@insert` when you want to add elements to a list, such as query parameters.

```rust,ignore
/// @openapi
/// paths:
///   /users:
///     get:
///       parameters:
///         # Inserts the list items directly here
///         @insert PaginationParams
```

#### Example: Using `@extend` (For Objects)

Use `@extend` when you want to merge properties, such as adding security requirements or standard responses to an existing block.

```rust,ignore
/// @openapi
/// paths:
///   /admin/dashboard:
///     get:
///       summary: Admin Area
///       
///       # Merges security and 401/403 responses into this operation
///       @extend Secured("admin")
///
///       responses:
///         '200':
///           description: OK
```

### 📚 Reference: Default Type Mappings

`oas-forge` automatically recognizes common Rust types (including popular crates like `chrono`, `uuid`, and `url`) and maps them to their OpenAPI equivalents.

| Rust Type | OpenAPI Type | Format | Notes |
| :--- | :--- | :--- | :--- |
| `bool` | `boolean` | - | |
| `String`, `&str`, `char` | `string` | - | |
| `i8`, `i16`, `i32`, `u8`, `u16`, `u32` | `integer` | `int32` | |
| `i64`, `u64`, `isize`, `usize` | `integer` | `int64` | `usize` and `isize` are treated as 64-bit. |
| `f32` | `number` | `float` | |
| `f64` | `number` | `double` | |
| `Uuid` | `string` | `uuid` | e.g., from `uuid` crate |
| `NaiveDate` | `string` | `date` | e.g., from `chrono` crate |
| `DateTime`, `NaiveDateTime` | `string` | `date-time` | e.g., from `chrono` crate |
| `NaiveTime` | `string` | `time` | e.g., from `chrono` crate |
| `Url`, `Uri` | `string` | `uri` | |
| `Decimal`, `BigDecimal` | `string` | `decimal` | String representation to preserve precision. |
| `ObjectId` | `string` | `objectid` | MongoDB/BSON identifier |
| `serde_json::Value` | - | - | Maps to `{}` (Any Type). |

## 📜 License

This project is licensed under the [MIT license](LICENSE).
