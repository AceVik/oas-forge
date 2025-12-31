# oas-forge

[![Crates.io](https://img.shields.io/crates/v/oas-forge.svg)](https://crates.io/crates/oas-forge)
[![Docs.rs](https://docs.rs/oas-forge/badge.svg)](https://docs.rs/oas-forge)
[![License](https://img.shields.io/crates/l/oas-forge.svg)](LICENSE)
[![Build Status](https://github.com/AceVik/oas-forge/actions/workflows/release.yml/badge.svg)](https://github.com/AceVik/oas-forge/actions)
[![Latest Release](https://img.shields.io/github/v/release/AceVik/oas-forge)](https://github.com/AceVik/oas-forge/releases/latest)

**The zero-runtime OpenAPI 3.1 compiler for Rust.**

`oas-forge` extracts, links, and merges code-first documentation into a standard `openapi.yaml` file at compile time. It eliminates the need for runtime macros that bloat your binary and crash on startup.

### Architecture

```ascii
[Source Code] --> [Scanner] --> [AST Parsing] --> [Registry]    
                                                     |
                                                     v
[Static YAML] --> [Merger] <--- [Monomorphizer] <--- [Fragments]
                       |
                       v
                 [openapi.yaml]
```

---

## Integration Guide

### Method A: build.rs (Fluent API)

The recommended approach.

```rust
use oas_forge::Generator;

fn main() {
    Generator::new()
        .input("src")
        .include("static/security.yaml") // Merge static config
        .output("openapi.yaml")
        .generate()
        .expect("Failed to generate OpenAPI spec");
}
```

### Method B: Cargo.toml Metadata

Configure in your manifest to keep `build.rs` minimal.

**Cargo.toml**:
```toml
[package.metadata.oas-forge]
input = ["src", "lib"]
include = ["static/skeleton.yaml"]
output = "docs/api.yaml"

[build-dependencies]
oas-forge = "0.4"
```

**build.rs**:
```rust
fn main() {
    oas_forge::Generator::default().generate().unwrap();
}
```

### Method C: CLI

Ideal for CI/CD pipelines.

```bash
cargo install oas-forge
# Run generation
oas_forge -i src -I static/openapi.yaml -o openapi.yaml
```

---

## Feature Reference

### ⚡ Route DSL

The Route DSL works purely in doc comments.

#### Inline Path Parameters (The "New Way")
Define parameters, types, and descriptions directly in the path.

```rust
/// @route GET /actress/{id: u32 "The unique ID"}
fn get_actress(id: u32) { ... }
```
*   **Result**: Automatically registers `id` as `in: path`, `required: true`, with `schema: {type: integer, format: int32}`.

#### Legacy Path Parameters (The "Old Way")
You can still split them if you prefer.

```rust
/// @route GET /actress/{id}
/// @path-param id: u32 "The unique ID"
fn get_actress(id: u32) { ... }
```

#### Flexible Parameter Syntax
Attributes are order-independent. Usage: `Name: [Type] [Flags...] [Desc]`.

```rust
/// @route GET /search
/// @query-param filter: Option<String> deprecated example="Alice" "Filter by name"
/// @header-param X-Trace-ID: Uuid required "Tracing ID"
fn search() { ... }
```
*   `deprecated`: Sets `deprecated: true`.
*   `example="Alice"`: Sets `example: "Alice"`.
*   `Option<T>`: Infers `required: false` (unless `required` flag is present).

#### Request Body
Link a Struct as the request body.

```rust
/// @route POST /users
/// @body $CreateRequest application/json
fn create() { ... }
```

#### Smart Responses
The DSL infers schemas and handles generics.

```rust
/// Creates a new user.
/// 
/// @route POST /users
/// @return 201: $User "Created"          <- JSON Ref to User schema
/// @return 200: $Page<User> "List"       <- Monomorphized Generic ($Page<User> -> Page_User)
/// @return 204: "Deleted"                <- Unit Type (No Content body)
/// @return 400: "Invalid Input"          <- String Literal (Description only)
fn create_user() { ... }
```

### 📦 Type Reflection

Document your Rust types to generate JSON Schemas.

#### Structs
```rust
/// @openapi
/// description: A registered user.
struct User {
    /// @openapi example: "123e4567-e89b..."
    id: Uuid,
    /// @openapi description: "Full name"
    name: String,
}
```

#### Enums
Rust Enums are mapped to OpenAPI String Enums.

```rust
/// @openapi
enum Role {
    Admin,
    User,
    Guest
}
// output: type: string, enum: ["Admin", "User", "Guest"]
```

#### Type Aliases & Newtypes
Fully supported.

```rust
/// @openapi
/// format: email
type Email = String;

/// @openapi
struct UserId(pub Uuid);
```

### 🛡️ Validation Safety

`oas-forge` validates your documentation at compile time.

**Example Failure:**
```rust
/// @route GET /users/{id}
/// @path-param slug: String "Slug"
fn get_user() {}
```

**Compiler Error:**
```text
error: Missing definition for path parameter 'id' in route '/users/{id}'
```

---

## License

This project is licensed under the [MIT license](LICENSE).
