//! Rust type expressions and their JSON Schema.

use serde_json::{Value, json};
use std::fmt;

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
            syn::Type::Group(g) => Self::from_syn(&g.elem),
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
            | "Option"
            | "Box"
            | "Arc"
            | "Rc"
            | "Cow"
            | "HashMap"
            | "BTreeMap"
            | "HashSet"
            | "LinkedList"
    )
}

/// The schema of a type and whether a field of that type is required.
#[derive(Debug, Clone, PartialEq)]
pub struct MappedType {
    /// The JSON Schema of the type.
    pub schema: Value,
    /// `false` for `Option<T>`: the field may be missing.
    pub required: bool,
}

impl MappedType {
    fn required(schema: Value) -> Self {
        Self {
            schema,
            required: true,
        }
    }
}

/// Maps Rust types to JSON Schema.
#[derive(Debug, Clone, Default)]
#[non_exhaustive]
pub struct TypeMapper {}

impl TypeMapper {
    /// A mapper with the built-in type table.
    pub fn new() -> Self {
        Self::default()
    }

    /// The schema of `ty`. Unknown named types become smart references
    /// (`{"$ref": "$Name"}`) that are resolved later.
    pub fn map(&self, ty: &TypeExpr) -> MappedType {
        match ty {
            TypeExpr::Named { args, smart, .. } => {
                let name = ty.name().unwrap_or_default();
                if *smart && !args.is_empty() && !is_container(name) {
                    // `$Page<User>`: kept for generic instantiation.
                    return MappedType::required(json!({ "$ref": ty.to_string() }));
                }
                self.map_named(name, args)
            }
            TypeExpr::Array { elem, .. } | TypeExpr::Slice(elem) => {
                MappedType::required(json!({ "type": "array", "items": self.map(elem).schema }))
            }
            TypeExpr::Reference(inner) => self.map(inner),
            TypeExpr::Tuple(_) | TypeExpr::Other(_) => {
                MappedType::required(json!({ "type": "object" }))
            }
        }
    }

    fn map_named(&self, name: &str, args: &[TypeExpr]) -> MappedType {
        let first = args.first();
        if matches!(name, "Box" | "Arc" | "Rc" | "Cow") {
            if let Some(inner) = first {
                return self.map(inner);
            }
        }
        let schema = match name {
            "bool" => json!({ "type": "boolean" }),
            "String" | "str" | "char" => json!({ "type": "string" }),
            "i8" | "i16" | "i32" | "u8" | "u16" | "u32" => {
                json!({ "type": "integer", "format": "int32" })
            }
            "i64" | "u64" | "isize" | "usize" => json!({ "type": "integer", "format": "int64" }),
            "f32" => json!({ "type": "number", "format": "float" }),
            "f64" => json!({ "type": "number", "format": "double" }),
            "Uuid" => json!({ "type": "string", "format": "uuid" }),
            "NaiveDate" => json!({ "type": "string", "format": "date" }),
            "DateTime" | "NaiveDateTime" | "DateTimeUtc" => {
                json!({ "type": "string", "format": "date-time" })
            }
            "NaiveTime" => json!({ "type": "string", "format": "time" }),
            "Url" | "Uri" => json!({ "type": "string", "format": "uri" }),
            "Decimal" | "BigDecimal" => json!({ "type": "string", "format": "decimal" }),
            "ObjectId" => json!({ "type": "string", "format": "objectid" }),
            "Value" => json!({}),
            "Option" => {
                return MappedType {
                    schema: first.map_or_else(|| json!({}), |inner| self.map(inner).schema),
                    required: false,
                };
            }
            "Vec" | "LinkedList" | "HashSet" => match first {
                Some(inner) => json!({ "type": "array", "items": self.map(inner).schema }),
                None => json!({ "type": "array" }),
            },
            "HashMap" | "BTreeMap" => match args.get(1) {
                Some(value) => {
                    json!({ "type": "object", "additionalProperties": self.map(value).schema })
                }
                None => json!({ "type": "object" }),
            },
            _ => json!({ "$ref": format!("${name}") }),
        };
        MappedType::required(schema)
    }
}
