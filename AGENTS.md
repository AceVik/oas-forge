# AGENTS.md - Developer Guide for AI Assistants

> **SYSTEM PROMPT:** If you are an AI assistant (Cursor, Copilot, Windsurf) working on this codebase, READ THIS FILE FIRST.

## 1. Project Context
`oas-forge` is a **compile-time** OpenAPI 3.1 generator for Rust. It parses source code ASTs (using `syn`) to generate a YAML spec.
* **Core Philosophy:** Zero runtime overhead. No macros in the final binary.
* **Stack:** Rust (2024 edition), `syn` (parsing), `serde_json/yaml` (output), `regex` (DSL parsing).

## 2. Architecture Data Flow

1.  **`src/scanner.rs`**: Walks file system. Handles text-based macros (`@insert`).
2.  **`src/visitor.rs`**: **THE CORE.** Parses Rust AST. Implements the **Route DSL** logic.
3.  **`src/index.rs`**: Stores Schemas, Blueprints (`$Page<T>`), and Fragments.
4.  **`src/generics.rs`**: **Monomorphizer.** Resolves `$Page<User>` refs to concrete schemas *after* parsing.
5.  **`src/merger.rs`**: Deep-merges generated JSON with static YAML files.

## 3. The Route DSL Specification
When generating doc comments for handlers, adhere to this Strict Grammar:

### A. Routes & Path Params
* **Preferred (Inline):** `/// @route METHOD /path/{name: Type "Description"}`
* **Legacy:** `/// @path-param name: Type "Description"`
* **Normalization:** The parser automatically strips type/desc to output `/path/{name}`.

### B. Parameters (Query, Header, Cookie)
* **Syntax:** `/// @query-param name: [Type] [Flags...] [Description]`
* **Rules:**
    * `Type`: Optional. Defaults to `String` if omitted (or if keyword detected).
    * `Flags` (Order Independent):
        * `required`: Forces `required: true`.
        * `deprecated`: Sets `deprecated: true`.
        * `example="Val"`: Sets example value.
    * `Description`: Quoted string `"..."`.

### C. Return Types & Body
* **Ref Syntax:** Use `$` prefix for internal schemas (e.g., `$User`, `$Page<User>`).
* **Unit/204:** `/// @return 204: "Deleted"` (Implies Unit/No Body).
* **Generics:** `/// @return 200: $Page<User>` (Must preserve generic syntax for Monomorphizer).

## 4. CRITICAL Implementation Constraints (Do Not Break)

### 🔴 Trap 1: Dynamic JSON Keys
**NEVER** use the `json!` macro for dynamic object keys in `visitor.rs`.
* **WRONG:** `json!({ path_var: ... })` -> Creates literal key `"path_var"`.
* **RIGHT:** Use `serde_json::Map` to insert `path_var` as a key.

### 🔴 Trap 2: Generics Resolution
**NEVER** wrap a type containing `<` (brackets) in `#/components/schemas/` inside `visitor.rs`.
* **WRONG:** `json!({ "$ref": "#/components/schemas/$Page<User>" })`
* **RIGHT:** `json!({ "$ref": "$Page<User>" })`
* **Reason:** The **Monomorphizer** (Pass 3) scans for raw `$Name<Args>` patterns. If you wrap it early, the scanner fails, and the concrete schema is never generated.

### 🔴 Trap 3: Unit / 204 Responses
If a return type is inferred as `()` or explicit `"No Content"`:
* **Constraint:** Do NOT generate a `content` block in the response object.
* **Reason:** HTTP 204 responses cannot have a body.

### 🔴 Trap 4: Strict Validation
Every change to `visit_item_fn` **MUST** be accompanied by a regression test in `src/visitor.rs` covering the specific syntax case.
