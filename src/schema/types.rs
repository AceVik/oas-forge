//! Rust type expressions and their JSON Schema.

use serde_json::{Map, Value, json};
use std::collections::BTreeMap;
use std::fmt;
use std::sync::OnceLock;

/// A parsed Rust type, independent of the parser library.
///
/// Smart references written with a `$` prefix (`$User`, `$Page<User>`) are
/// [`TypeExpr::Named`] with `smart: true`.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum TypeExpr {
    /// A path such as `Vec<User>` or `chrono::DateTime<Utc>`.
    Named {
        /// The path segments, e.g. `["chrono", "DateTime"]`.
        path: Vec<String>,
        /// Generic type arguments (lifetimes and constants are dropped).
        args: Vec<TypeExpr>,
        /// Written with a `$` prefix.
        smart: bool,
    },
    /// `[T; N]` (`len` when `N` is an integer literal).
    Array {
        /// The element type.
        elem: Box<TypeExpr>,
        /// The length, if known.
        len: Option<usize>,
    },
    /// `[T]`
    Slice(Box<TypeExpr>),
    /// `(A, B)`; `()` is the empty tuple.
    Tuple(Vec<TypeExpr>),
    /// `&T` / `&mut T`
    Reference(Box<TypeExpr>),
    /// Anything else (function pointers, trait objects, `impl Trait`, macros).
    Other(String),
}

const SMART_MARKER: &str = "__oas_forge_smart__";

impl TypeExpr {
    /// Parses a type written in Rust syntax, optionally with `$` smart markers.
    ///
    /// Returns `None` when the text is not a type.
    pub fn parse(text: &str) -> Option<Self> {
        let marked = text.trim().replace('$', SMART_MARKER);
        let ty: syn::Type = syn::parse_str(&marked).ok()?;
        Some(Self::from_syn(&ty))
    }

    /// A smart reference `$name`.
    pub fn smart(name: impl Into<String>) -> Self {
        Self::Named {
            path: vec![name.into()],
            args: Vec::new(),
            smart: true,
        }
    }

    pub(crate) fn from_syn(ty: &syn::Type) -> Self {
        match ty {
            syn::Type::Path(p) => {
                let mut smart = false;
                let path: Vec<String> = p
                    .path
                    .segments
                    .iter()
                    .map(|seg| {
                        let ident = seg.ident.to_string();
                        match ident.strip_prefix(SMART_MARKER) {
                            Some(stripped) => {
                                smart = true;
                                stripped.to_string()
                            }
                            None => ident,
                        }
                    })
                    .collect();
                let args = match p.path.segments.last().map(|s| &s.arguments) {
                    Some(syn::PathArguments::AngleBracketed(args)) => args
                        .args
                        .iter()
                        .filter_map(|arg| match arg {
                            syn::GenericArgument::Type(t) => Some(Self::from_syn(t)),
                            _ => None,
                        })
                        .collect(),
                    _ => Vec::new(),
                };
                if path.is_empty() {
                    return Self::Other(String::new());
                }
                Self::Named { path, args, smart }
            }
            syn::Type::Array(a) => Self::Array {
                elem: Box::new(Self::from_syn(&a.elem)),
                len: match &a.len {
                    syn::Expr::Lit(syn::ExprLit {
                        lit: syn::Lit::Int(n),
                        ..
                    }) => n.base10_parse().ok(),
                    _ => None,
                },
            },
            syn::Type::Slice(s) => Self::Slice(Box::new(Self::from_syn(&s.elem))),
            syn::Type::Reference(r) => Self::Reference(Box::new(Self::from_syn(&r.elem))),
            syn::Type::Paren(p) => Self::from_syn(&p.elem),
            syn::Type::Tuple(t) => Self::Tuple(t.elems.iter().map(Self::from_syn).collect()),
            other => Self::Other(quote_type(other)),
        }
    }

    /// The last path segment of a named type (`DateTime` for `chrono::DateTime<Utc>`).
    pub fn name(&self) -> Option<&str> {
        match self {
            Self::Named { path, .. } => path.last().map(String::as_str),
            _ => None,
        }
    }

    /// The generic arguments of a named type.
    pub fn args(&self) -> &[TypeExpr] {
        match self {
            Self::Named { args, .. } => args,
            _ => &[],
        }
    }

    /// Whether the type was written as a `$` smart reference.
    pub fn is_smart(&self) -> bool {
        matches!(self, Self::Named { smart: true, .. })
    }
}

impl fmt::Display for TypeExpr {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Named { path, args, smart } => {
                if *smart {
                    f.write_str("$")?;
                }
                f.write_str(&path.join("::"))?;
                if !args.is_empty() {
                    f.write_str("<")?;
                    for (i, arg) in args.iter().enumerate() {
                        if i > 0 {
                            f.write_str(", ")?;
                        }
                        write!(f, "{arg}")?;
                    }
                    f.write_str(">")?;
                }
                Ok(())
            }
            Self::Array { elem, len } => match len {
                Some(len) => write!(f, "[{elem}; {len}]"),
                None => write!(f, "[{elem}; _]"),
            },
            Self::Slice(elem) => write!(f, "[{elem}]"),
            Self::Tuple(elems) => {
                f.write_str("(")?;
                for (i, e) in elems.iter().enumerate() {
                    if i > 0 {
                        f.write_str(", ")?;
                    }
                    write!(f, "{e}")?;
                }
                f.write_str(")")
            }
            Self::Reference(inner) => write!(f, "&{inner}"),
            Self::Other(text) => f.write_str(text),
        }
    }
}

fn quote_type(ty: &syn::Type) -> String {
    quote::ToTokens::to_token_stream(ty).to_string()
}

/// Std containers and wrappers that are mapped structurally rather than
/// referenced (`Vec`, `Option`, `Box`, maps, sets…).
pub(crate) fn is_container(name: &str) -> bool {
    matches!(
        name,
        "Vec"
            | "VecDeque"
            | "LinkedList"
            | "BinaryHeap"
            | "SmallVec"
            | "Option"
            | "Box"
            | "Arc"
            | "Rc"
            | "Cow"
            | "RefCell"
            | "Cell"
            | "Mutex"
            | "RwLock"
            | "Wrapping"
            | "Saturating"
            | "Reverse"
            | "Json"
            | "Form"
            | "HashMap"
            | "BTreeMap"
            | "IndexMap"
            | "HashSet"
            | "BTreeSet"
            | "IndexSet"
            | "NonZero"
            | "Range"
            | "RangeInclusive"
    )
}

/// The schema of a type and how a field of that type behaves.
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub struct MappedType {
    /// The JSON Schema of the type (for `Option<T>` the schema of `T`).
    pub schema: Value,
    /// `false` for `Option<T>`: the field may be missing.
    pub required: bool,
    /// `true` for `Option<T>`: the value may be `null`. Parameters ignore it;
    /// properties and bodies render it with [`nullable`](super::nullable).
    pub nullable: bool,
}

impl MappedType {
    fn required(schema: Value) -> Self {
        Self {
            schema,
            required: true,
            nullable: false,
        }
    }

    /// The schema with nullability applied (2020-12 form).
    pub fn into_schema(self) -> Value {
        if self.nullable {
            super::nullable(self.schema)
        } else {
            self.schema
        }
    }
}

/// Maps Rust types to JSON Schema, following the serde data model.
///
/// Types the table does not know become smart references
/// (`{"$ref": "$Name"}`) that are resolved against the documented schemas.
/// [`TypeMapper::with_type`] adds or replaces mappings.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct TypeMapper {
    custom: BTreeMap<String, Value>,
}

impl TypeMapper {
    /// A mapper with the built-in type table.
    pub fn new() -> Self {
        Self::default()
    }

    /// The shared mapper with only the built-in table.
    pub fn builtin() -> &'static Self {
        static BUILTIN: OnceLock<TypeMapper> = OnceLock::new();
        BUILTIN.get_or_init(Self::new)
    }

    /// Maps the type `name` to `schema`. `name` is a type name (`UserId`) or a
    /// path (`my_crate::UserId`); a path only matches types written with the
    /// same path. Custom mappings take precedence over the built-in table.
    pub fn with_type(mut self, name: impl Into<String>, schema: Value) -> Self {
        self.custom.insert(name.into(), schema);
        self
    }

    /// The custom mappings, by name.
    pub fn custom_types(&self) -> impl Iterator<Item = (&String, &Value)> {
        self.custom.iter()
    }

    /// The schema of `ty`.
    pub fn map(&self, ty: &TypeExpr) -> MappedType {
        match ty {
            TypeExpr::Named { path, args, smart } => {
                if let Some(schema) = self.custom_schema(path) {
                    return MappedType::required(schema);
                }
                let name = ty.name().unwrap_or_default();
                if *smart && !args.is_empty() && !is_container(name) {
                    // `$Page<User>`: kept for generic instantiation.
                    return MappedType::required(json!({ "$ref": ty.to_string() }));
                }
                self.map_named(path, args)
            }
            TypeExpr::Array { elem, len } => {
                let items = self.schema(elem);
                MappedType::required(match len {
                    Some(len) => json!({
                        "type": "array",
                        "items": items,
                        "minItems": len,
                        "maxItems": len,
                    }),
                    None => json!({ "type": "array", "items": items }),
                })
            }
            TypeExpr::Slice(elem) => {
                MappedType::required(json!({ "type": "array", "items": self.schema(elem) }))
            }
            TypeExpr::Reference(inner) => self.map(inner),
            TypeExpr::Tuple(elems) if elems.is_empty() => {
                MappedType::required(json!({ "type": "null" }))
            }
            TypeExpr::Tuple(elems) => MappedType::required(self.tuple(elems)),
            TypeExpr::Other(_) => MappedType::required(json!({})),
        }
    }

    /// The schema of a type in a nested position (`Vec<Option<T>>` items are
    /// nullable).
    fn schema(&self, ty: &TypeExpr) -> Value {
        self.map(ty).into_schema()
    }

    /// `(A, B)`: a fixed-length array.
    fn tuple(&self, elems: &[TypeExpr]) -> Value {
        let items: Vec<Value> = elems.iter().map(|e| self.schema(e)).collect();
        json!({
            "type": "array",
            "prefixItems": items,
            "items": false,
            "minItems": elems.len(),
            "maxItems": elems.len(),
        })
    }

    fn custom_schema(&self, path: &[String]) -> Option<Value> {
        if self.custom.is_empty() {
            return None;
        }
        let full = path.join("::");
        self.custom
            .get(&full)
            .or_else(|| path.last().and_then(|name| self.custom.get(name)))
            .cloned()
    }

    fn map_named(&self, path: &[String], args: &[TypeExpr]) -> MappedType {
        let name = path.last().map(String::as_str).unwrap_or_default();
        let qualified = |prefix: &str| matches!(path, [first, _, ..] if first == prefix);
        let first = args.first();
        let element = || first.map_or_else(|| json!({}), |inner| self.schema(inner));
        let array = |extra: Value| {
            let mut schema = Map::new();
            schema.insert("type".into(), json!("array"));
            if let Some(inner) = first {
                schema.insert("items".into(), self.schema(inner));
            }
            if let Value::Object(extra) = extra {
                schema.extend(extra);
            }
            Value::Object(schema)
        };
        if let Some(schema) = integer(name) {
            return MappedType::required(schema);
        }

        let schema = match name {
            "Option" => {
                let inner = first.map(|inner| self.map(inner));
                return MappedType {
                    schema: inner.map_or_else(|| json!({}), MappedType::into_schema),
                    required: false,
                    nullable: true,
                };
            }
            // `Json`/`Form` are the web framework body wrappers (`Json<T>` in a
            // `@return` or handler signature is `T`).
            "Box" | "Arc" | "Rc" | "Cow" | "RefCell" | "Cell" | "Mutex" | "RwLock" | "Wrapping"
            | "Saturating" | "Reverse" | "Json" | "Form" => match first {
                Some(inner) => return self.map(inner),
                None => json!({}),
            },
            "bool" => json!({ "type": "boolean" }),
            "String" | "str" => json!({ "type": "string" }),
            "char" => json!({ "type": "string", "minLength": 1, "maxLength": 1 }),
            "f32" => json!({ "type": "number", "format": "float" }),
            "f64" => json!({ "type": "number", "format": "double" }),
            "NonZero" => match first.and_then(TypeExpr::name) {
                Some(inner) => non_zero(inner),
                None => json!({ "type": "integer" }),
            },
            _ if name.starts_with("NonZero") => non_zero(
                &name
                    .strip_prefix("NonZero")
                    .unwrap_or_default()
                    .to_lowercase(),
            ),
            "Vec" | "VecDeque" | "LinkedList" | "BinaryHeap" => array(json!({})),
            "SmallVec" => match first {
                Some(TypeExpr::Array { elem, .. }) => {
                    json!({ "type": "array", "items": self.schema(elem) })
                }
                _ => array(json!({})),
            },
            "HashSet" | "BTreeSet" | "IndexSet" => array(json!({ "uniqueItems": true })),
            "HashMap" | "BTreeMap" | "IndexMap" => {
                let mut schema = Map::new();
                schema.insert("type".into(), json!("object"));
                if let Some(value) = args.get(1) {
                    schema.insert("additionalProperties".into(), self.schema(value));
                }
                if let Some(names) = first.and_then(property_names) {
                    schema.insert("propertyNames".into(), names);
                }
                Value::Object(schema)
            }
            "Map" if qualified("serde_json") => json!({ "type": "object" }),
            "Value" => json!({}),
            "Range" | "RangeInclusive" => json!({
                "type": "object",
                "properties": { "start": element(), "end": element() },
                "required": ["start", "end"],
            }),
            "PhantomData" => json!({ "type": "null" }),
            "Uuid" => json!({ "type": "string", "format": "uuid" }),
            "Ulid" => json!({ "type": "string", "format": "ulid" }),
            "Url" | "Uri" => json!({ "type": "string", "format": "uri" }),
            "DateTime" | "NaiveDateTime" | "DateTimeUtc" | "OffsetDateTime"
            | "PrimitiveDateTime" | "UtcDateTime" => {
                json!({ "type": "string", "format": "date-time" })
            }
            "NaiveDate" => json!({ "type": "string", "format": "date" }),
            "NaiveTime" => json!({ "type": "string", "format": "time" }),
            "Date" if qualified("time") || qualified("jiff") => {
                json!({ "type": "string", "format": "date" })
            }
            "Time" if qualified("time") || qualified("jiff") => {
                json!({ "type": "string", "format": "time" })
            }
            "Timestamp" if qualified("jiff") => json!({ "type": "string", "format": "date-time" }),
            "SignedDuration" | "Span" if qualified("jiff") => {
                json!({ "type": "string", "format": "duration" })
            }
            "Duration" if qualified("time") => json!({
                "type": "array",
                "prefixItems": [
                    { "type": "integer", "format": "int64" },
                    { "type": "integer", "format": "int32" },
                ],
                "items": false,
                "minItems": 2,
                "maxItems": 2,
            }),
            "Duration" => json!({
                "type": "object",
                "properties": {
                    "secs": { "type": "integer", "format": "int64", "minimum": 0 },
                    "nanos": { "type": "integer", "format": "int64", "minimum": 0, "maximum": 999_999_999 },
                },
                "required": ["secs", "nanos"],
            }),
            "SystemTime" => json!({
                "type": "object",
                "properties": {
                    "secs_since_epoch": { "type": "integer", "format": "int64", "minimum": 0 },
                    "nanos_since_epoch": { "type": "integer", "format": "int64", "minimum": 0, "maximum": 999_999_999 },
                },
                "required": ["secs_since_epoch", "nanos_since_epoch"],
            }),
            "Decimal" | "BigDecimal" => json!({ "type": "string", "format": "decimal" }),
            "ObjectId" => json!({ "type": "string", "format": "objectid" }),
            "Bytes" | "BytesMut" => json!({
                "type": "array",
                "items": { "type": "integer", "format": "int32", "minimum": 0, "maximum": 255 },
            }),
            "IpAddr" => json!({
                "type": "string",
                "anyOf": [{ "format": "ipv4" }, { "format": "ipv6" }],
            }),
            "Ipv4Addr" => json!({ "type": "string", "format": "ipv4" }),
            "Ipv6Addr" => json!({ "type": "string", "format": "ipv6" }),
            "SocketAddr" | "SocketAddrV4" | "SocketAddrV6" | "PathBuf" | "Path" => {
                json!({ "type": "string" })
            }
            // `Page<User>`: a generic type, instantiated from its blueprint.
            _ if !args.is_empty() => {
                let args: Vec<String> = args.iter().map(ToString::to_string).collect();
                json!({ "$ref": format!("${name}<{}>", args.join(", ")) })
            }
            _ => json!({ "$ref": format!("${name}") }),
        };
        MappedType::required(schema)
    }
}

/// The schema of a primitive integer type.
fn integer(name: &str) -> Option<Value> {
    Some(match name {
        "i8" => {
            json!({ "type": "integer", "format": "int32", "minimum": i8::MIN, "maximum": i8::MAX })
        }
        "i16" => {
            json!({ "type": "integer", "format": "int32", "minimum": i16::MIN, "maximum": i16::MAX })
        }
        "i32" => json!({ "type": "integer", "format": "int32" }),
        "i64" | "isize" => json!({ "type": "integer", "format": "int64" }),
        "i128" => json!({ "type": "integer" }),
        "u8" => json!({ "type": "integer", "format": "int32", "minimum": 0, "maximum": u8::MAX }),
        "u16" => json!({ "type": "integer", "format": "int32", "minimum": 0, "maximum": u16::MAX }),
        "u32" => json!({ "type": "integer", "format": "int64", "minimum": 0, "maximum": u32::MAX }),
        "u64" | "usize" => json!({ "type": "integer", "format": "int64", "minimum": 0 }),
        "u128" => json!({ "type": "integer", "minimum": 0 }),
        _ => return None,
    })
}

/// `NonZeroU32` / `NonZero<u32>`: the integer without zero.
fn non_zero(inner: &str) -> Value {
    let Some(Value::Object(mut schema)) = integer(inner) else {
        return json!({ "type": "integer" });
    };
    if inner.starts_with('u') {
        schema.insert("minimum".into(), json!(1));
    } else {
        schema.insert("not".into(), json!({ "enum": [0] }));
    }
    Value::Object(schema)
}

/// Map keys are always strings in JSON; integer and boolean keys are
/// restricted to their textual form.
fn property_names(key: &TypeExpr) -> Option<Value> {
    let name = key.name()?;
    if name == "bool" {
        return Some(json!({ "enum": ["true", "false"] }));
    }
    integer(name)?;
    Some(if name.starts_with('u') {
        json!({ "pattern": "^[0-9]+$" })
    } else {
        json!({ "pattern": "^-?[0-9]+$" })
    })
}
