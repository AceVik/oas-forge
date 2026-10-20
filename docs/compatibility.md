# Compatibility

Schemas describe the JSON that serde writes. This page lists how attributes
of serde, validator and garde and the common Rust types are documented.
Attributes are also read inside `#[cfg_attr(…, serde(…))]`.

## serde

### Containers (structs and enums)

| Attribute | Schema |
| :--- | :--- |
| `rename = "Name"` | the schema name; `rename(serialize = "A", deserialize = "B")` uses the serialize name (`deserialize` alone changes nothing) |
| `rename_all = "…"` | field names (structs) or variant names (enums), exactly as serde converts them; names set with `rename` are kept as written |
| `rename_all_fields = "…"` | field names of struct variants (enums) |
| `tag = "t"` | internally tagged enum: one schema per variant with the tag property, `oneOf` and a `discriminator`; on a struct: a required tag property with the struct's serde name |
| `tag = "t", content = "c"` | adjacently tagged enum |
| `untagged` | `anyOf` of the variants' content |
| `transparent` | the schema of the single field |
| `deny_unknown_fields` | `additionalProperties: false` (`unevaluatedProperties: false` with flattened fields); on an enum also for its struct variants |
| `default` | every field is optional |
| `into = "T"`, `from = "T"`, `try_from = "T"` | the schema of `T` |
| `bound`, `crate`, `remote`, `expecting`, `variant_identifier`, `field_identifier` | no effect on the format |

### Fields

| Attribute | Schema |
| :--- | :--- |
| `rename = "name"` | the property name |
| `default`, `default = "path"` | not required |
| `skip` | left out |
| `skip_serializing` | `writeOnly: true` |
| `skip_deserializing` | `readOnly: true` |
| `skip_serializing_if = "…"` | not required |
| `flatten` | a struct joins with `allOf`; a map becomes `additionalProperties` (`unevaluatedProperties` next to flattened structs); an `Option` may be absent as a whole |
| `with`, `serialize_with` | known modules change the type, see below; others keep the field's type |
| `with`, `deserialize_with` | the field is required, also an `Option` (unless `default`) |
| `alias`, `borrow`, `bound`, `getter` | no effect on the documented format |

Known `with` modules:

| Module | Schema |
| :--- | :--- |
| `chrono::serde::ts_seconds` and the other `ts_*` modules, `…::timestamp` | `integer`, `int64` |
| `time::serde::rfc3339`, `…::iso8601` | `string`, `date-time` |
| `time::serde::rfc2822`, `humantime_serde`, `serde_with::rust::display_fromstr`, `hex` | `string` |
| `base64` modules | `string`, `byte` |

### Variants

| Attribute | Schema |
| :--- | :--- |
| `rename = "name"` | the variant name or tag value |
| `rename_all = "…"` | the field names of this struct variant |
| `skip`, or `skip_serializing` together with `skip_deserializing` | left out |
| `untagged` | an additional `anyOf` alternative |
| `with = "…"` | for newtype variants, as for fields |
| `other`, `alias` | documented like any other variant |

### Enum representations

| Representation | Unit variant | Newtype variant | Tuple variant | Struct variant |
| :--- | :--- | :--- | :--- | :--- |
| external (default) | string in `enum` (with a `oneOf` of `const`s for overrides, `@example`, `#[deprecated]`) | `{Variant: T}` | `{Variant: [..]}` | `{Variant: {..}}` |
| internal (`tag`) | `{tag}` | `{tag, ..fields of T}` for structs and maps; others are reported (`OF1005`) | reported (`OF1005`) | `{tag, ..fields}` |
| adjacent (`tag`, `content`) | `{tag}` | `{tag, content: T}` | `{tag, content: [..]}` | `{tag, content: {..}}` |
| untagged | `null` | `T` | `[..]` | `{..}` |

Tagged variants are separate component schemas named `{Enum}{Variant}`; the
discriminator maps the tag values to them. In a generic enum, the variants
that use a type parameter are instantiated per use (`{Enum}{Variant}_{Args}`).

`#[derive(Serialize_repr)]` of [serde_repr](https://docs.rs/serde_repr)
writes the discriminant: the enum is an integer `enum` of the discriminants
(implicit ones counted like Rust does).

## validator

| Rule | Schema |
| :--- | :--- |
| `email` | `format: email` |
| `url` | `format: uri` |
| `ip` / `ip(v4)` / `ip(v6)` | `anyOf: [ipv4, ipv6]` / `format: ipv4` / `format: ipv6` |
| `length(min, max, equal)` | `minLength`/`maxLength` (strings), `minItems`/`maxItems` (lists), `minProperties`/`maxProperties` (maps) |
| `range(min, max, exclusive_min, exclusive_max, equal)` | `minimum`, `maximum`, `exclusiveMinimum`, `exclusiveMaximum`; floats and negative values are fine |
| `contains(pattern = "x")`, `contains = "x"` | `pattern` matching the literal text |
| `does_not_contain(pattern = "x")` | `not: { pattern }` |
| `regex(path = RE)`, `regex = "RE"` | the `pattern` of the constant `RE` (see [schemas](schemas.md#validation-constraints)) |
| `required`, `required_nested` | required and not nullable |
| `credit_card`, `non_control_character`, `must_match`, `custom`, `nested`, `schema` | not expressible; left out |

## garde

| Rule | Schema |
| :--- | :--- |
| `email`, `url` | `format: email`, `format: uri` |
| `ip`, `ipv4`, `ipv6` | `anyOf: [ipv4, ipv6]`, `format: ipv4`, `format: ipv6` |
| `ascii` | `pattern: '^[\x00-\x7F]*$'` |
| `length(min, max, equal)` | like validator; for strings see the length modes below |
| `length(chars, …)`, `char_count(…)` | exact `minLength`/`maxLength` |
| `length(bytes, …)` (default for strings), `byte_length(…)` | `maxLength` = the byte limit, `minLength` = bytes / 4 (bounds every valid value meets) |
| `length(utf16, …)` | `maxLength` = the limit, `minLength` = units / 2 |
| `length(graphemes, …)` | `minLength` only |
| `range(min, max, equal)` | `minimum`, `maximum` |
| `contains("x")`, `prefix("x")`, `suffix("x")` | `pattern` (`x`, `^x`, `x$`, escaped) |
| `pattern("re")`, `pattern(RE)` | `pattern`, from a literal or a constant |
| `required` | required and not nullable |
| `inner(…)` | the rules apply to the items of a list or the values of a map |
| `alphanumeric`, `credit_card`, `phone_number`, `matches`, `custom`, `dive`, `skip` | not expressible; left out |

Constraint values must be literals; a constant (`length(max = MAX)`) is
reported (`OF1006`). Several rules on one field must all hold: a second
`pattern` goes into `allOf`.

## Types

| Rust | Schema |
| :--- | :--- |
| `bool` | `boolean` |
| `i8`, `i16` | `integer`, `int32`, with `minimum`/`maximum` |
| `i32` | `integer`, `int32` |
| `i64`, `isize` | `integer`, `int64` |
| `u8`, `u16` | `integer`, `int32`, `minimum: 0`, `maximum` |
| `u32` | `integer`, `int64`, `minimum: 0`, `maximum: 4294967295` |
| `u64`, `usize` | `integer`, `int64`, `minimum: 0` |
| `i128`, `u128` | `integer` (unsigned: `minimum: 0`) |
| `NonZeroU32`, `NonZero<u32>`, … | the integer with `minimum: 1` (unsigned) or `not: {enum: [0]}` (signed) |
| `f32`, `f64` | `number`, `float` / `double` |
| `String`, `str`, `Cow<str>` | `string` |
| `char` | `string` with exactly one character |
| `Option<T>` | `T` or `null`; the field is optional |
| `Box`, `Rc`, `Arc`, `Cow`, `RefCell`, `Cell`, `Mutex`, `RwLock`, `Wrapping`, `Saturating`, `Reverse` | the inner type |
| `Vec`, `VecDeque`, `LinkedList`, `BinaryHeap`, `SmallVec`, slices | `array` |
| `[T; N]` | `array` with `minItems` = `maxItems` = N |
| `HashSet`, `BTreeSet`, `IndexSet` | `array` with `uniqueItems: true` |
| `HashMap`, `BTreeMap`, `IndexMap` | `object` with `additionalProperties`; integer and `bool` keys add `propertyNames` (digits, `true`/`false`) |
| tuples `(A, B)` | `array` with `prefixItems` |
| `()` , `PhantomData` | `null` |
| `serde_json::Value` | any value (`{}`) |
| `serde_json::Map`, `Map<String, Value>` | `object` |
| `Range`, `RangeInclusive` | `object` with `start` and `end` |
| `std::time::Duration` | `object` with `secs` and `nanos` |
| `SystemTime` | `object` with `secs_since_epoch` and `nanos_since_epoch` |
| `Uuid` | `string`, `uuid` |
| `Ulid` | `string`, `ulid` |
| `Url`, `Uri` | `string`, `uri` |
| `chrono::DateTime`; `time`: `OffsetDateTime`, `PrimitiveDateTime`, `UtcDateTime`; `jiff::Timestamp` | `string`, `date-time` |
| `NaiveDateTime`, `jiff::civil::DateTime` (no offset) | `string`, `date-time-local` |
| `NaiveDate`, `time::Date`, `jiff::civil::Date` | `string`, `date` |
| `NaiveTime`, `time::Time`, `jiff::civil::Time` (no offset) | `string`, `time-local` |
| `time::Duration` | `[seconds, nanoseconds]` |
| `jiff::SignedDuration`, `jiff::Span` | `string`, `duration` |
| `Decimal`, `BigDecimal` | `string`, `decimal` |
| `ObjectId` | `string`, `objectid` |
| `Bytes`, `BytesMut` | `array` of bytes |
| `IpAddr` | `string` with `anyOf` of `ipv4` and `ipv6` |
| `Ipv4Addr`, `Ipv6Addr` | `string`, `ipv4` / `ipv6` |
| `SocketAddr`, `PathBuf`, `Path` | `string` |
| `Json<T>`, `Form<T>` | `T` |
| a documented type | `$ref: '#/components/schemas/Name'` |
| a generic type (`Page<User>`) | a reference to the instance `Page_User` |

Types that are ambiguous by name (`Date`, `Time`, `Duration`) are mapped by
their path (`time::Date`, `jiff::civil::Date`); use the path or a
[custom mapping](schemas.md#types-from-other-crates) for others. A bare name
of the table (`Duration`, `Value`, `Uuid`, …) that is also the name of a
documented schema refers to that schema
([own types with built-in names](schemas.md#types-from-other-crates)).

## Web frameworks

[Signature inference](route-dsl.md#signature-inference) reads these
extractors:

| axum | actix-web | Documents |
| :--- | :--- | :--- |
| `Path<T>` | `web::Path<T>` | path parameters |
| `Query<T>` | `web::Query<T>` | query parameters |
| `Json<T>` | `web::Json<T>` | `application/json` request body |
| `Form<T>` | `web::Form<T>` | `application/x-www-form-urlencoded` request body |
| `Multipart` | `Multipart` | `multipart/form-data` request body |
| `-> Json<T>`, `-> Result<Json<T>, E>`, `-> (StatusCode, Json<T>)` | the same | the `200` response |

Other parameters (`State`, `Extension`, `HeaderMap`, `web::Data`, …) are
ignored. Handlers of other frameworks work with the DSL alone.

## OpenAPI versions

| Version | Support |
| :--- | :--- |
| 3.2 | full, including `QUERY`, `additionalOperations`, `querystring` parameters and streamed responses (`itemSchema`) |
| 3.1 | full; the default for a generated root (3.1.1) |
| 3.0 | full; schemas use `nullable` and the 3.0 keywords; 3.1 and 3.2 features are left out with a warning |
| Swagger 2.0 | as an export; see [Outputs](outputs.md#swagger-20) |
| AsyncAPI 3.0, 3.1 | the `@asyncapi` document |
| JSON Schema 2020-12 | as a bundle of the component schemas |
