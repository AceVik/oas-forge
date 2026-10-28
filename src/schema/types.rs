//! Rust type expressions and their JSON Schema.

use crate::diagnostics::{Code, Diagnostic};
use crate::extract::names::{NameTable, Resolved};
use crate::template::generics::substitute;
use crate::template::refs::UNRESOLVED_KEY;
use serde_json::{Map, Value, json};
use std::collections::{BTreeMap, BTreeSet};
use std::fmt;
use std::sync::Arc;
use std::sync::OnceLock;

/// A parsed Rust type, independent of the parser library.
///
/// Smart references written with a `$` prefix (`$User`, `$Page<User>`) are
/// [`TypeExpr::Named`] with `smart: true`.
///
/// # Examples
///
/// ```
/// use oas_forge::schema::TypeExpr;
///
/// let ty = TypeExpr::parse("HashMap<String, $User>").ok_or("not a type")?;
/// assert_eq!(ty.name(), Some("HashMap"));
/// assert!(ty.args()[1].is_smart());
/// assert_eq!(ty.to_string(), "HashMap<String, $User>");
/// # Ok::<(), Box<dyn std::error::Error>>(())
/// ```
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

/// Stands in for `$` while `syn` parses a type (`$User` is no Rust);
/// [`TypeExpr::from_syn`] strips it from path segments again and sets
/// `smart`.
const SMART_MARKER: &str = "__oas_forge_smart__";

/// The deepest nesting [`TypeExpr::parse`] accepts.
const MAX_TYPE_NESTING: usize = 32;

/// How deeply `text` nests brackets (`<`, `(`, `[`; the `>` of `->` closes
/// nothing), and the number of `&` and `*`, whichever is larger.
fn nesting(text: &str) -> usize {
    let (mut depth, mut deepest, mut indirections) = (0_usize, 0, 0);
    let mut previous = ' ';
    for c in text.chars() {
        match c {
            '<' | '(' | '[' => {
                depth += 1;
                deepest = deepest.max(depth);
            }
            '>' if previous == '-' => {}
            '>' | ')' | ']' => depth = depth.saturating_sub(1),
            '&' | '*' => indirections += 1,
            _ => {}
        }
        previous = c;
    }
    deepest.max(indirections)
}

impl TypeExpr {
    /// Parses a type written in Rust syntax, optionally with `$` smart markers.
    ///
    /// Returns `None` when the text is not a type, or nests `<`, `(` and `[`
    /// or references and pointers more than 32 levels deep: `syn` recurses
    /// once per level, on the caller's stack.
    ///
    /// # Examples
    ///
    /// ```
    /// use oas_forge::schema::TypeExpr;
    ///
    /// let ty = TypeExpr::parse("[u8; 4]").ok_or("not a type")?;
    /// assert!(matches!(ty, TypeExpr::Array { len: Some(4), .. }));
    /// assert_eq!(TypeExpr::parse("not a type!"), None);
    /// # Ok::<(), Box<dyn std::error::Error>>(())
    /// ```
    pub fn parse(text: &str) -> Option<Self> {
        if nesting(text) > MAX_TYPE_NESTING {
            return None;
        }
        let marked = text.trim().replace('$', SMART_MARKER);
        let ty: syn::Type = syn::parse_str(&marked).ok()?;
        Some(Self::from_syn(&ty))
    }

    /// A smart reference `$name`.
    ///
    /// # Examples
    ///
    /// ```
    /// use oas_forge::schema::TypeExpr;
    ///
    /// let user = TypeExpr::smart("User");
    /// assert_eq!(Some(user.clone()), TypeExpr::parse("$User"));
    /// assert_eq!(user.to_string(), "$User");
    /// ```
    pub fn smart(name: impl Into<String>) -> Self {
        Self::Named {
            path: vec![name.into()],
            args: Vec::new(),
            smart: true,
        }
    }

    /// Converts a type parsed by `syn`. Segments with the [`SMART_MARKER`]
    /// prefix make a smart reference; generic arguments come from the last
    /// segment (lifetimes and constants dropped), parentheses are removed,
    /// and types without an equivalent become [`TypeExpr::Other`].
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
    ///
    /// # Examples
    ///
    /// ```
    /// use oas_forge::schema::TypeExpr;
    ///
    /// let ty = TypeExpr::parse("chrono::DateTime<Utc>").ok_or("not a type")?;
    /// assert_eq!(ty.name(), Some("DateTime"));
    /// assert_eq!(TypeExpr::parse("(u8, u16)").ok_or("not a type")?.name(), None);
    /// # Ok::<(), Box<dyn std::error::Error>>(())
    /// ```
    pub fn name(&self) -> Option<&str> {
        match self {
            Self::Named { path, .. } => path.last().map(String::as_str),
            _ => None,
        }
    }

    /// The generic arguments of a named type.
    ///
    /// # Examples
    ///
    /// ```
    /// use oas_forge::schema::TypeExpr;
    ///
    /// let ty = TypeExpr::parse("Result<User, ApiError>").ok_or("not a type")?;
    /// let names: Vec<Option<&str>> = ty.args().iter().map(TypeExpr::name).collect();
    /// assert_eq!(names, [Some("User"), Some("ApiError")]);
    /// # Ok::<(), Box<dyn std::error::Error>>(())
    /// ```
    pub fn args(&self) -> &[TypeExpr] {
        match self {
            Self::Named { args, .. } => args,
            _ => &[],
        }
    }

    /// Whether the type was written as a `$` smart reference.
    ///
    /// # Examples
    ///
    /// ```
    /// use oas_forge::schema::TypeExpr;
    ///
    /// assert!(TypeExpr::parse("$Page<User>").ok_or("not a type")?.is_smart());
    /// assert!(!TypeExpr::parse("Page<User>").ok_or("not a type")?.is_smart());
    /// # Ok::<(), Box<dyn std::error::Error>>(())
    /// ```
    pub fn is_smart(&self) -> bool {
        matches!(self, Self::Named { smart: true, .. })
    }

    /// The parts of the type that cannot be documented: function pointers,
    /// trait objects, `impl Trait`, macros (their schema is `{}`). The
    /// argument of `PhantomData`, which serializes as `null`, is not read.
    pub(crate) fn undocumentable(&self) -> Vec<String> {
        let mut parts = Vec::new();
        self.collect_undocumentable(&mut parts);
        parts
    }

    /// Adds the parts of [`Self::undocumentable`] to `parts`.
    fn collect_undocumentable(&self, parts: &mut Vec<String>) {
        match self {
            Self::Named { .. } if self.name() == Some("PhantomData") => {}
            Self::Named { args, .. } | Self::Tuple(args) => {
                for arg in args {
                    arg.collect_undocumentable(parts);
                }
            }
            Self::Array { elem, .. } | Self::Slice(elem) | Self::Reference(elem) => {
                elem.collect_undocumentable(parts);
            }
            Self::Other(text) => parts.push(text.clone()),
        }
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

/// The finding for `part` of a type that cannot be documented
/// ([`TypeExpr::undocumentable`]).
pub(crate) fn undocumentable(part: &str) -> Diagnostic {
    Diagnostic::new(
        Code::UndocumentableType,
        format!(
            "`{part}` cannot be documented (a function pointer, trait object, `impl Trait` or macro); its schema is `{{}}`"
        ),
    )
}

/// The token text of `ty` as it is usually written (`fn(&mut u8) -> u8`),
/// kept in [`TypeExpr::Other`].
fn quote_type(ty: &syn::Type) -> String {
    let tokens = quote::ToTokens::to_token_stream(ty).to_string();
    // Tokens are separated by spaces; only those inside paths, generics,
    // parentheses and after `&`, `*` and `!` go.
    let mut out = String::with_capacity(tokens.len());
    let mut chars = tokens.chars().peekable();
    while let Some(c) = chars.next() {
        let tight = c == ' '
            && (out.ends_with(['(', '[', '<', '&', ':', '!', '*'])
                || chars.peek().is_some_and(|next| {
                    [')', ']', '>', ',', '(', ':', '<', '!', ';'].contains(next)
                }));
        if !tight {
            out.push(c);
        }
    }
    out
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
///
/// # Examples
///
/// ```
/// use oas_forge::schema::{TypeExpr, TypeMapper};
/// use serde_json::json;
///
/// let ty = TypeExpr::parse("Option<u32>").ok_or("not a type")?;
/// let mapped = TypeMapper::new().map(&ty);
/// assert_eq!(
///     mapped.schema,
///     json!({ "type": "integer", "format": "int64", "minimum": 0, "maximum": 4_294_967_295_u32 })
/// );
/// assert!(!mapped.required);
/// assert!(mapped.nullable);
/// # Ok::<(), Box<dyn std::error::Error>>(())
/// ```
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
    /// A required, non-nullable type with `schema`.
    fn required(schema: Value) -> Self {
        Self {
            schema,
            required: true,
            nullable: false,
        }
    }

    /// The schema with nullability applied (2020-12 form).
    ///
    /// # Examples
    ///
    /// ```
    /// use oas_forge::schema::{TypeExpr, TypeMapper};
    /// use serde_json::json;
    ///
    /// let ty = TypeExpr::parse("Option<String>").ok_or("not a type")?;
    /// let mapped = TypeMapper::new().map(&ty);
    /// assert_eq!(mapped.schema, json!({ "type": "string" }));
    /// assert_eq!(mapped.into_schema(), json!({ "type": ["string", "null"] }));
    /// # Ok::<(), Box<dyn std::error::Error>>(())
    /// ```
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
///
/// # Examples
///
/// ```
/// use oas_forge::schema::{TypeExpr, TypeMapper};
/// use serde_json::json;
///
/// let types = TypeMapper::new();
/// let schema = |text: &str| TypeExpr::parse(text).map(|ty| types.map(&ty).into_schema());
///
/// assert_eq!(
///     schema("Vec<Uuid>"),
///     Some(json!({ "type": "array", "items": { "type": "string", "format": "uuid" } }))
/// );
/// // Unknown types stay smart references until the references stage.
/// assert_eq!(schema("User"), Some(json!({ "$ref": "$User" })));
/// ```
#[derive(Debug, Clone, Default, PartialEq)]
pub struct TypeMapper {
    /// The schemas of [`TypeMapper::with_type`], by type name or `::` path.
    custom: BTreeMap<String, Value>,
    /// Built-in names written bare that stand for documented schemas.
    shadowed: BTreeSet<String>,
    /// The module paths of the scanned sources and the module the types are
    /// written in (`crate`, `module`, …): names written in Rust resolve to
    /// the component names of the documented types they mean.
    names: Option<(Arc<NameTable>, Vec<String>)>,
    /// The type parameters of the item being reflected (`T` of `Page<T>`):
    /// placeholders, never documented types.
    params: BTreeSet<String>,
}

/// Names of the built-in table that a crate may also give its own types
/// (`Duration`, `Value`, `Uuid`, …). Written bare (one segment, no generic
/// arguments), they may refer to a documented schema of that name, which
/// then wins; see [`TypeMapper::shadowable_names`].
const SHADOWABLE: &[&str] = &[
    "Value",
    "Duration",
    "SystemTime",
    "Range",
    "RangeInclusive",
    "Uuid",
    "Ulid",
    "Url",
    "Uri",
    "DateTime",
    "NaiveDateTime",
    "DateTimeUtc",
    "OffsetDateTime",
    "PrimitiveDateTime",
    "UtcDateTime",
    "NaiveDate",
    "NaiveTime",
    "Decimal",
    "BigDecimal",
    "ObjectId",
    "Bytes",
    "BytesMut",
    "IpAddr",
    "Ipv4Addr",
    "Ipv6Addr",
    "SocketAddr",
    "SocketAddrV4",
    "SocketAddrV6",
    "PathBuf",
    "Path",
];

impl TypeMapper {
    /// A mapper with the built-in type table.
    ///
    /// # Examples
    ///
    /// ```
    /// use oas_forge::schema::{TypeExpr, TypeMapper};
    /// use serde_json::json;
    ///
    /// let ty = TypeExpr::parse("bool").ok_or("not a type")?;
    /// assert_eq!(TypeMapper::new().map(&ty).schema, json!({ "type": "boolean" }));
    /// # Ok::<(), Box<dyn std::error::Error>>(())
    /// ```
    pub fn new() -> Self {
        Self::default()
    }

    /// The shared mapper with only the built-in table.
    ///
    /// # Examples
    ///
    /// ```
    /// use oas_forge::schema::TypeMapper;
    ///
    /// assert_eq!(TypeMapper::builtin(), &TypeMapper::new());
    /// assert_eq!(TypeMapper::builtin().custom_types().count(), 0);
    /// ```
    pub fn builtin() -> &'static Self {
        static BUILTIN: OnceLock<TypeMapper> = OnceLock::new();
        BUILTIN.get_or_init(Self::new)
    }

    /// Maps the type `name` to `schema`. `name` is a type name (`UserId`) or a
    /// path (`my_crate::UserId`); a path only matches types written with the
    /// same path. Custom mappings take precedence over the built-in table.
    ///
    /// # Examples
    ///
    /// ```
    /// use oas_forge::schema::{TypeExpr, TypeMapper};
    /// use serde_json::json;
    ///
    /// let types = TypeMapper::new()
    ///     .with_type("UserId", json!({ "type": "string", "format": "uuid" }))
    ///     .with_type("money::Amount", json!({ "type": "string", "format": "decimal" }));
    /// let schema = |text: &str| TypeExpr::parse(text).map(|ty| types.map(&ty).schema);
    ///
    /// // A name matches any path that ends with it.
    /// assert_eq!(schema("models::UserId"), Some(json!({ "type": "string", "format": "uuid" })));
    /// // A path matches only the same path.
    /// assert_eq!(schema("money::Amount"), Some(json!({ "type": "string", "format": "decimal" })));
    /// assert_eq!(schema("Amount"), Some(json!({ "$ref": "$Amount" })));
    /// ```
    pub fn with_type(mut self, name: impl Into<String>, schema: Value) -> Self {
        self.custom.insert(name.into(), schema);
        self
    }

    /// The custom mappings, by name.
    ///
    /// # Examples
    ///
    /// ```
    /// use oas_forge::schema::TypeMapper;
    /// use serde_json::json;
    ///
    /// let types = TypeMapper::new()
    ///     .with_type("UserId", json!({ "type": "string" }))
    ///     .with_type("Amount", json!({ "type": "string" }));
    /// let names: Vec<&String> = types.custom_types().map(|(name, _)| name).collect();
    /// assert_eq!(names, ["Amount", "UserId"]);
    /// ```
    pub fn custom_types(&self) -> impl Iterator<Item = (&String, &Value)> {
        self.custom.iter()
    }

    /// The built-in names in `ty` that a documented schema of the same name
    /// would replace: written bare (`Duration`, not `std::time::Duration`),
    /// without generic arguments and without a custom mapping.
    pub(crate) fn shadowable_names(&self, ty: &TypeExpr) -> BTreeSet<String> {
        let mut names = BTreeSet::new();
        self.collect_shadowable(ty, &mut names);
        names
    }

    /// Adds the shadowable names of `ty`, its generic arguments, element
    /// types and tuple members to `names`.
    fn collect_shadowable(&self, ty: &TypeExpr, names: &mut BTreeSet<String>) {
        match ty {
            TypeExpr::Named { path, args, .. } => {
                if let [name] = path.as_slice()
                    && args.is_empty()
                    && SHADOWABLE.contains(&name.as_str())
                    && self.custom_schema(path).is_none()
                {
                    names.insert(name.clone());
                }
                for arg in args {
                    self.collect_shadowable(arg, names);
                }
            }
            TypeExpr::Array { elem, .. } | TypeExpr::Slice(elem) | TypeExpr::Reference(elem) => {
                self.collect_shadowable(elem, names);
            }
            TypeExpr::Tuple(elems) => {
                for elem in elems {
                    self.collect_shadowable(elem, names);
                }
            }
            TypeExpr::Other(_) => {}
        }
    }

    /// This mapper for types written in the module `scope` of the sources
    /// of `names`.
    pub(crate) fn scoped(&self, names: &Arc<NameTable>, scope: Vec<String>) -> Self {
        let mut mapper = self.clone();
        mapper.names = Some((Arc::clone(names), scope));
        mapper
    }

    /// The component name of the documented type `name` of the module this
    /// mapper is scoped to, when it is qualified because its name collides.
    pub(crate) fn qualified(&self, name: &str) -> Option<String> {
        let (names, scope) = self.names.as_ref()?;
        names.qualified(scope, name).map(str::to_string)
    }

    /// This mapper for an item with the type parameters `params`.
    pub(crate) fn with_params(&self, params: BTreeSet<String>) -> Self {
        let mut mapper = self.clone();
        mapper.params = params;
        mapper
    }

    /// What the Rust type written as `path` means in the module of this
    /// mapper (unknown without a name table, and for a type parameter);
    /// `generic` when it is written with type arguments.
    fn resolve(&self, path: &[String], generic: bool) -> Resolved {
        match (&self.names, path) {
            (_, [name]) if self.params.contains(name) => Resolved::Unknown,
            (Some((names, scope)), _) => names.resolve(scope, path, generic),
            (None, _) => Resolved::Unknown,
        }
    }

    /// `{"$ref": "$Name"}` for the type written as `path`, `Name` the
    /// component name of the documented type it means (`resolve`, for types
    /// written in Rust). A name that is ambiguous or names an undocumented
    /// type of its module, while documented types have it, carries the
    /// finding under [`UNRESOLVED_KEY`] for the references stage.
    fn reference(&self, path: &[String], resolve: bool) -> Value {
        let name = path.last().map(String::as_str).unwrap_or_default();
        let resolved = if resolve {
            self.resolve(path, false)
        } else {
            Resolved::Unknown
        };
        if let Resolved::Component(component) = &resolved {
            return json!({ "$ref": format!("${component}") });
        }
        let mut reference = Map::new();
        reference.insert("$ref".into(), json!(format!("${name}")));
        if let Some(finding) = finding(path, &resolved) {
            reference.insert(UNRESOLVED_KEY.into(), json!(finding));
        }
        Value::Object(reference)
    }

    /// A generic reference without the `$` (`Page<$User>`) for the generic
    /// type written as `path` with `args`: the template and the named
    /// arguments by the component names of the documented types they mean
    /// (`resolve`). A name that does not say which documented type it means
    /// adds its finding to `findings`.
    fn generic_text(
        &self,
        path: &[String],
        args: &[TypeExpr],
        resolve: bool,
        findings: &mut Vec<String>,
    ) -> String {
        let written = path.last().map(String::as_str).unwrap_or_default();
        let template = match resolve.then(|| self.resolve(path, true)) {
            Some(Resolved::Component(component)) => component,
            resolved => {
                findings.extend(resolved.and_then(|resolved| finding(path, &resolved)));
                written.to_string()
            }
        };
        let args: Vec<String> = args
            .iter()
            .map(|arg| self.generic_arg(arg, findings))
            .collect();
        format!("{template}<{}>", args.join(", "))
    }

    /// `ty` as a generic argument of a raw reference (`$User`,
    /// `$Vec<$T>`): what [`Self::generic_arg`] makes of it, without
    /// findings (where the type is used, its schema reports them).
    pub(crate) fn type_text(&self, ty: &TypeExpr) -> String {
        self.generic_arg(ty, &mut Vec::new())
    }

    /// A generic argument of a raw reference. Named types carry a `$`
    /// (`$Page<$T>`, `$Page<$Vec<$T>>`), so that the generics stage
    /// substitutes the type parameters of a blueprint also inside nested
    /// arguments; a documented type is named by its component name, and a
    /// reference is its target (`&str` is `$str`). A name that does not say
    /// which documented type it means adds its finding to `findings`.
    fn generic_arg(&self, ty: &TypeExpr, findings: &mut Vec<String>) -> String {
        match ty {
            TypeExpr::Reference(inner) => self.generic_arg(inner, findings),
            TypeExpr::Named { path, args, .. } if args.is_empty() => {
                let resolved = self.resolve(path, false);
                if let Resolved::Component(component) = resolved {
                    return format!("${component}");
                }
                findings.extend(finding(path, &resolved));
                format!("${}", path.join("::"))
            }
            TypeExpr::Named { path, args, .. } => {
                format!("${}", self.generic_text(path, args, true, findings))
            }
            other => other.to_string(),
        }
    }

    /// This mapper with the bare built-in `names` standing for documented
    /// schemas (`{"$ref": "$Duration"}`).
    pub(crate) fn shadowing(&self, names: &BTreeSet<String>) -> Self {
        let mut mapper = self.clone();
        mapper.shadowed.extend(names.iter().cloned());
        mapper
    }

    /// The schema of `ty`.
    ///
    /// # Examples
    ///
    /// ```
    /// use oas_forge::schema::{TypeExpr, TypeMapper};
    /// use serde_json::json;
    ///
    /// let types = TypeMapper::new();
    /// let schema = |text: &str| TypeExpr::parse(text).map(|ty| types.map(&ty).schema);
    ///
    /// assert_eq!(
    ///     schema("HashMap<u32, String>"),
    ///     Some(json!({
    ///         "type": "object",
    ///         "additionalProperties": { "type": "string" },
    ///         "propertyNames": { "pattern": "^[0-9]+$" }
    ///     }))
    /// );
    /// // Generic types stay raw references for the generics stage.
    /// assert_eq!(schema("Page<User>"), Some(json!({ "$ref": "$Page<$User>" })));
    /// assert_eq!(schema("$Page<User>"), Some(json!({ "$ref": "$Page<User>" })));
    /// ```
    pub fn map(&self, ty: &TypeExpr) -> MappedType {
        match ty {
            TypeExpr::Named { path, args, smart } => {
                if let Some(schema) = self.custom_schema(path) {
                    return MappedType::required(schema);
                }
                if let [name] = path.as_slice()
                    && args.is_empty()
                    && self.shadowed.contains(name)
                {
                    return MappedType::required(json!({ "$ref": format!("${name}") }));
                }
                let name = ty.name().unwrap_or_default();
                if *smart && !args.is_empty() && !is_container(name) {
                    // `$Page<User>`: kept for generic instantiation.
                    return MappedType::required(if ty.undocumentable().is_empty() {
                        json!({ "$ref": ty.to_string() })
                    } else {
                        json!({})
                    });
                }
                // `$User` is a schema name; only types written in Rust are
                // resolved by their module.
                self.map_named(path, args, !*smart)
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

    /// The custom schema of the type at `path`: a mapping of the whole path
    /// (`money::Amount`) first, then one of its last segment (`Amount`).
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

    /// The schema of a named type without custom or shadowed mapping:
    /// integers, std containers and wrappers, the library types of the
    /// built-in table, and else a smart reference (`$Name`, or `$Name<$Arg>`
    /// for a generic type); `resolve` resolves the name of a type written in
    /// Rust by its module.
    fn map_named(&self, path: &[String], args: &[TypeExpr], resolve: bool) -> MappedType {
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
            // `NonZeroU32`, … (not every type whose name starts with `NonZero`).
            _ if name
                .strip_prefix("NonZero")
                .is_some_and(|inner| integer(&inner.to_lowercase()).is_some()) =>
            {
                non_zero(
                    &name
                        .strip_prefix("NonZero")
                        .unwrap_or_default()
                        .to_lowercase(),
                )
            }
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
            // `serde_json::Map`, also imported (`Map<String, Value>`).
            "Map" if qualified("serde_json") || is_json_map(path, args) => {
                json!({ "type": "object" })
            }
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
            // Date-times without offset (`2024-01-02T03:04:05`) are not RFC 3339
            // `date-time`s; the OpenAPI format registry calls them
            // `date-time-local` (and times without offset `time-local`).
            "NaiveDateTime" => json!({ "type": "string", "format": "date-time-local" }),
            "DateTime" if qualified("jiff") => {
                json!({ "type": "string", "format": "date-time-local" })
            }
            "DateTime" | "DateTimeUtc" | "OffsetDateTime" | "PrimitiveDateTime" | "UtcDateTime" => {
                json!({ "type": "string", "format": "date-time" })
            }
            "NaiveDate" => json!({ "type": "string", "format": "date" }),
            "NaiveTime" => json!({ "type": "string", "format": "time-local" }),
            "Date" if qualified("time") || qualified("jiff") => {
                json!({ "type": "string", "format": "date" })
            }
            "Time" if qualified("time") || qualified("jiff") => {
                json!({ "type": "string", "format": "time-local" })
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
            // A function pointer or trait object as an argument has no name.
            _ if args.iter().any(|arg| !arg.undocumentable().is_empty()) => json!({}),
            _ => return self.declared(path, args, resolve),
        };
        MappedType::required(schema)
    }

    /// The schema of a type that is not built in: a type alias of an
    /// `Option` is optional like an `Option`; `Page<User>`, a generic type,
    /// is instantiated from its blueprint; other names are references. A
    /// template or argument that does not say which documented type it means
    /// is reported, and nothing is instantiated.
    fn declared(&self, path: &[String], args: &[TypeExpr], resolve: bool) -> MappedType {
        if resolve && let Some(optional) = self.optional_alias(path, args) {
            return optional;
        }
        if args.is_empty() {
            return MappedType::required(self.reference(path, resolve));
        }
        let mut findings = Vec::new();
        let text = self.generic_text(path, args, resolve, &mut findings);
        MappedType::required(match findings.into_iter().next() {
            Some(finding) => json!({ UNRESOLVED_KEY: finding }),
            None => json!({ "$ref": format!("${text}") }),
        })
    }

    /// `Option<X>` for a type alias of an `Option` (`type MaybeId =
    /// Option<u64>;`, also through aliases of aliases): `X` is read in the
    /// module of the alias, its type parameters bound to `args`.
    fn optional_alias(&self, path: &[String], args: &[TypeExpr]) -> Option<MappedType> {
        let (names, scope) = self.names.as_ref()?;
        if matches!(path, [name] if self.params.contains(name)) {
            return None;
        }
        let alias = names.optional_alias(scope, path)?;
        if alias.params.len() != args.len() {
            return None;
        }
        let params = alias.params.iter().cloned().collect();
        let mapper = self.scoped(names, alias.scope).with_params(params);
        let mut schema = mapper.map(&alias.inner).into_schema();
        if !args.is_empty() {
            // The arguments are written here, so they are read here.
            let texts: Vec<String> = args.iter().map(|arg| self.type_text(arg)).collect();
            let bindings: BTreeMap<&str, &str> = alias
                .params
                .iter()
                .map(String::as_str)
                .zip(
                    texts
                        .iter()
                        .map(|text| text.strip_prefix('$').unwrap_or(text)),
                )
                .collect();
            schema = substitute(&schema, &bindings);
        }
        Some(MappedType {
            schema,
            required: false,
            nullable: true,
        })
    }
}

/// The finding for the type written as `path` when `resolved` does not
/// identify one documented type: several documented types, or an
/// undocumented type of its module while documented types have its name.
fn finding(path: &[String], resolved: &Resolved) -> Option<String> {
    let name = path.last().map(String::as_str).unwrap_or_default();
    match resolved {
        Resolved::Undocumented(defined) => Some(format!(
            "`{name}` is `{defined}` here, which is not documented; document it with `@openapi`"
        )),
        Resolved::Ambiguous(paths) => Some(format!(
            "`{}` may be any of the documented types {}; write its path or import it with `use`",
            path.join("::"),
            paths
                .iter()
                .map(|p| format!("`{p}`"))
                .collect::<Vec<_>>()
                .join(", ")
        )),
        Resolved::Component(_) | Resolved::Unknown => None,
    }
}

/// `Map<String, Value>`: `serde_json::Map` used by its imported name.
fn is_json_map(path: &[String], args: &[TypeExpr]) -> bool {
    path.len() == 1
        && matches!(args, [key, value] if key.name() == Some("String") && value.name() == Some("Value"))
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
