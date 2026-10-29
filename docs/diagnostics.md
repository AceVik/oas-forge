# Diagnostics

Every problem is reported as a diagnostic with a stable code, a severity
and, where possible, the file and line of the annotation that caused it.
Nothing is dropped silently.

- **Errors** fail `generate()` and the CLI. `build()` still returns the
  `Spec` with them, so tools can show every finding at once.
- **Warnings** are printed (as `cargo:warning` in build scripts with
  `emit_cargo_directives(true)`) and fail the run only with
  `deny_warnings(true)` / `--deny-warnings`.
- **Notes** are informational and never fail a run.

Problems that make a result impossible (unreadable files, invalid Rust,
invalid YAML in a document, two roots) are not diagnostics but errors of the
run (`oas_forge::Error`), with the location and the offending lines.

The codes are stable: a code keeps its meaning across releases
([ADR 0006](adr/0006-diagnostics.md)).

## Inputs (OF0xxx)

| Code | Severity | Cause | Fix |
| :--- | :--- | :--- | :--- |
| `OF0001` | warning | An `include` file does not exist | Fix the path or remove the include |
| `OF0002` | error | An input file exceeds the size limit (16 MiB) | Exclude it or raise `max_file_size` |
| `OF0003` | note | A configured output inside an input directory was not read | Nothing to do |
| `OF0004` | note | No `openapi`/`info` root in the sources; it was generated from the package | Write a root to choose the values yourself, or call `without_package()` |
| `OF0005` | warning | A symbolic link in an input directory is broken or forms a loop; it was skipped | Fix or remove the link |
| `OF0006` | warning | A `.proto` file cannot be indexed (an unclosed brace, comment or string, a second `package`, a malformed `message` line) and was skipped, or declares a message another file declares (the first is used) | Fix the file, or rename the message |
| `OF0007` | warning | The `Cargo.toml` next to a scanned `src` directory cannot be read or has no `[package] name`; the crate's types are qualified with the directory's name instead (`crate.module.Name`) | Fix the manifest |
| `OF0008` | warning | A symbolic link in an input directory leads outside every input directory; it was skipped | Add its target as an input or include, or remove the link |
| `OF0009` | error | A source writes a key of the reserved prefix `x-oas-forge-`, the generator's own markers ([reserved keys](templates.md#reserved-keys)): in an `@openapi` block or override, an `@example`, a fragment, the YAML of a route or channel block, or a `.yaml`/`.json` input outside the template wire format in `components`; the key is removed. Or a source chooses a name that would become such a key (a `@security` scheme, a `@return-header` name, a media type, the serialized name of a field, variant or type, a `#[serde(tag)]` or `#[serde(content)]`, an `@openapi-type` name, a channel id, a placeholder of its address, a message, `@ws-query` or `@ws-header` name); what it names is left out. Also reported for a key of the prefix left in a finished document (written where no check reads it, such as an `@extend` marker whose value is no fragment name, or a marker of an imported template that no stage resolved) | Remove the key, or rename the item (a directive whose inline schema or flag value writes one is ignored with `OF2004`) |

## Extraction (OF1xxx)

| Code | Severity | Cause | Fix |
| :--- | :--- | :--- | :--- |
| `OF1001` | warning | The YAML after `@openapi` on a type, field or variant is invalid or not a mapping (`/// @openapi The id`) | Fix the YAML (descriptions go before the `@openapi` line); the override is ignored meanwhile |
| `OF1002` | warning | `@openapi<T>` on something without a name | Put it on a struct or enum |
| `OF1003` | note | A separator line (`//! ---`) closed a file-level block | Nothing to do; text after it is not part of the block |
| `OF1004` | warning | Two items produce the same schema name: documented Rust types of different modules are named by their module path (`crate.module.Name`), other definitions (a YAML schema, the same type read twice) were merged | Rename one (`#[serde(rename)]` or `@openapi rename`) |
| `OF1005` | warning | A variant shape serde cannot write in the enum's tagging mode (a tuple or primitive in an internally tagged enum); only the tag is documented | Use a struct variant or another representation |
| `OF1006` | warning | A validation value is not a literal (`length(max = MAX)`) or outside the 64-bit range; the constraint is left out | Use a literal, or add the keyword with an `@openapi` override |
| `OF1007` | warning | A `regex(path = …)` or `pattern(…)` constant is not defined in the scanned code | Scan the file that defines it, or define it with a literal pattern |
| `OF1008` | warning | An `@openapi` directive has an invalid argument: an unknown `rename-all` rule, `rename` without a name, text after `skip` | Fix the directive; the argument is ignored meanwhile |
| `OF1009` | warning | A variant of a `serde_repr` enum has a discriminant that is not an integer literal; the values are not documented | Use literals, or describe the enum with an `@openapi` override |
| `OF1010` | warning | A documented type uses a type that cannot be documented: a function pointer, `dyn Trait`, `impl Trait` or a macro, also through a type alias; its schema is `{}` | Skip the field (`#[serde(skip)]`), describe it with an `@openapi` override, or map the type with `map_type` |
| `OF1011` | warning | A `regex(path = …)` or `pattern(…)` constant matches several constants with different patterns (the same name in several modules); the pattern is left out | Qualify the path with the module (`regex(path = *crate::a::RE)`), or rename a constant |

## Route DSL (OF2xxx)

| Code | Severity | Cause | Fix |
| :--- | :--- | :--- | :--- |
| `OF2001` | error | A `@route` without method or path, an invalid method, two `@route`s in one block, a `@channel` without address | Fix the line |
| `OF2002` | error | A path placeholder without parameter definition | Declare it inline (`{id: u64}`), with `@path-param` or in YAML |
| `OF2003` | error | A path parameter that is not in the path | Remove it or add the placeholder |
| `OF2004` | warning | A directive could not be parsed (e.g. `@return` without code) | Fix the line; it is ignored meanwhile |
| `OF2005` | warning | Embedded YAML in a route is invalid or not a mapping | Fix the YAML |
| `OF2006` | warning | Unknown directive | Fix the spelling; see the [directive table](route-dsl.md#directives) |
| `OF2007` | warning | A handler's `Path` extractor does not fit the placeholders | Align the extractor with the path, or use `@no-infer` |
| `OF2008` | warning | `@query-params $Struct` (or an inferred `Query<T>`) names no documented struct | Add `@openapi` to the struct, or declare the parameters |
| `OF2009` | warning | `proto(package.Message)` names no top-level message of the `.proto` inputs; the message is left out | Qualify the name with the package, and add the `.proto` file to an input directory or as an include |

## Templates and references (OF3xxx)

| Code | Severity | Cause | Fix |
| :--- | :--- | :--- | :--- |
| `OF3001` | warning | `@insert`/`@extend` names an unknown fragment, or fragments are nested more than 8 deep | Define the fragment, or check its name |
| `OF3002` | warning | A fragment's body is not valid YAML when it is extended | Fix the fragment |
| `OF3003` | warning | A fragment got fewer arguments than it has parameters | Pass every argument |
| `OF3004` | warning | A generic reference names an unknown template | Document the generic type, or include the crate's fragments |
| `OF3005` | warning | A template got the wrong number of arguments | Match the type's parameters |
| `OF3006` | error | Generic instantiation nested more than 16 levels; reported once per generic type, at the first instance that reaches the limit | Look for a recursive generic type |
| `OF3007` | warning | `$Name` does not name a known schema | Document the type, define it with `@openapi-type`, map it with `map_type`, or escape a literal `\$` |
| `OF3008` | warning | Included templates use an unknown format version | Regenerate the fragments with this version |
| `OF3009` | warning | `{{env:NAME}}` names an environment variable that is not allowed or not set; the placeholder is kept | Allow it (`Generator::allow_env`, `allow_env`, `--allow-env`) and set it for the build |
| `OF3010` | warning | A fragment inserts or extends itself, directly or through other fragments; the repeated `@insert`/`@extend` is skipped | Remove the cycle |
| `OF3011` | warning | A fragment got more arguments than it has parameters; the extra ones are ignored | Remove the extra arguments, or quote an argument that contains a comma |
| `OF3012` | warning | A type alias name used without `@openapi` names aliases with different targets in several files; the reference is not resolved | Rename one of the aliases, or document the type the name should mean |
| `OF3013` | warning | A type alias refers to itself through other aliases; the reference is not resolved | Remove the cycle |
| `OF3014` | warning | A Rust type name in a field, type argument, signature or route means several documented types (none of them defined or imported in its module), or names an undocumented type of its module while other documented types have the name; the schema has no reference | Import the type with `use`, write its path, or document the local type |
| `OF3015` | error | A generic `$ref` is malformed or contains more than 128 nested generic lists | Fix the reference or reduce its nesting |
| `OF3016` | error | Fragment expansion or variable substitution exceeds 16 MiB of processed text or 65,536 visited lines or extension resolutions, or the pasted text of a run (inserted fragment lines, `@extend` bodies, copies of route defaults, of a block and its defaults for each further `@route` line and of `.proto` files; 256 bytes per line besides the text) exceeds 32 MiB, or the parameters of struct expansion in a document exceed 16 MiB (or 4,194,304 visited schemas and fields), or the `formData` parameters of the form bodies of a Swagger 2.0 export exceed 16 MiB (from the first form body that does not fit on, form bodies are left out) | Reduce repeated expansion or the size of arguments and variables |
| `OF3017` | warning | A type alias is reached through a chain of more than 64 type aliases; the reference is not resolved | Shorten the chain |
| `OF3018` | error | The concrete schemas of generic instantiation, or the targets of replaced type aliases, exceed 16 MiB in all (names, strings and keys, plus 32 bytes per value, 256 per instance and 64 per replaced alias; the inline schemas of std containers such as `$Vec<T>` and the references wrapped into `{ $ref: … }` while an instance or alias target is built count the same way); later instances and aliases are not created, and a reference whose inline schema does not fit keeps its text | Look for a generic type whose instances refer to growing instances of itself, or type aliases whose target names the next alias several times |

## Merging (OF4xxx)

| Code | Severity | Cause | Fix |
| :--- | :--- | :--- | :--- |
| `OF4001` | warning | Two documents set the same value differently; the later one wins. Or a document adds entries to a `security` or `servers` list another document set (the lists are joined), or adds such a list to an operation or path another document defined (it replaces the list that applied there), or adds to an operation or path whose `security` or `servers` another document set, or adds a key to a security scheme another document defined | Remove one of them, or set the list in one place |
| `OF4002` | warning | The root of the sources has a `@scope` that does not include the scope of a document, which gets no root fields from it (no global `security`, `servers`, …) | Give that scope a root of its own, or add the scope to the root |

## Lints (OF5xxx)

Lints check the merged document. A finding points to the annotation or file
that first defined the part it concerns: the operation, the component, or
the top-level field such as `security`. For a duplicate `operationId` that
is the second operation using it.

| Code | Severity | Cause | Fix |
| :--- | :--- | :--- | :--- |
| `OF5001` | warning | A `$ref` points to a component that does not exist | Define the component or fix the reference |
| `OF5002` | warning | Two operations have the same `operationId` | Rename a handler or use `@operation-id` |
| `OF5003` | warning (strict) | A component is not reachable from outside `components` | Remove it, or ignore the finding for public schemas |
| `OF5004` | warning (strict) | An operation has no summary or description, or a schema no description | Add a doc comment |
| `OF5005` | warning | A security requirement names a scheme not in `components.securitySchemes` | Define the scheme |
| `OF5006` | warning | A path placeholder has no path parameter in an operation, or a path parameter no placeholder; likewise a placeholder of an AsyncAPI channel address and the channel's `parameters` | Align the parameters with the path or address |

## Versions and exports (OF6xxx)

| Code | Severity | Cause | Fix |
| :--- | :--- | :--- | :--- |
| `OF6001` | note | The target (OpenAPI 3.0, Swagger 2.0, AsyncAPI) has no exact equivalent; the construct was approximated | Nothing to do, or avoid the construct |
| `OF6002` | warning | The construct needs a newer OpenAPI version than the target and was removed (`QUERY`, streams, webhooks, `info.summary`, …) | Raise the root's `openapi` version, or accept the loss in the export |

## Machine-readable output

`Diagnostic::to_json()` and `--message-format json` give

```json
{"severity":"warning","code":"OF3007","message":"`$Usr` does not name a known schema","file":"src/api.rs","line":14}
```

with `column` where known. `Code::ALL` lists every code;
`Code::default_severity()` gives its severity.

## Troubleshooting

**A type is missing from the document.** Only types with a line starting with
`@openapi` are exported, and generic types only where they are used with
concrete arguments. Check that the file is inside an `input` directory.

**`$ref: '#/components/schemas/Foo'` leads nowhere (`OF3007`, `OF5001`).**
`Foo` is a type from another crate or not annotated. See
[types from other crates](schemas.md#types-from-other-crates).

**The route is missing.** Look for `OF2001`–`OF2003`: a rejected route
leaves no trace in the document, and the run fails.

**Two roots.** Exactly one snippet may have both `openapi` and `info`. A YAML
file next to the sources that is a complete document counts as a root; move
it out of the input directory or remove its root keys.

**The document changes between machines.** It should not; please report it
with both outputs. Environment variables (`{{env:NAME}}`) and the package
version are the only inputs besides the sources.

**Build scripts are slow.** Compile them with optimizations, see
[faster builds](getting-started.md#faster-builds).
