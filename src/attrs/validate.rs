//! Validation attributes of the `validator` and `garde` crates as JSON
//! Schema keywords.
//!
//! Rules are collected per field and applied to the field's schema with
//! [`Constraints::apply`], which picks the keywords by the schema's type:
//! `length` becomes `minLength`/`maxLength` on strings, `minItems`/`maxItems`
//! on arrays and `minProperties`/`maxProperties` on maps.

use super::{metas, string_literal};
use crate::diagnostics::{Code, Diagnostic, Diagnostics, Location};
use crate::extract::PatternItem;
use serde_json::{Map, Number, Value, json};
use std::collections::BTreeMap;
use std::path::Path;
use syn::punctuated::Punctuated;
use syn::{Attribute, Expr, ExprLit, ExprUnary, Lit, Meta, UnOp};

/// How a string length is counted.
///
/// JSON Schema counts characters, so other units become the character bounds
/// every valid value satisfies.
///
/// # Examples
///
/// garde's `length(min = 8, max = 64)` counts UTF-8 bytes: 8 bytes are at
/// least 2 characters of up to 4 bytes each.
///
/// ```
/// use oas_forge::attrs::{Constraints, LengthUnit, Rule};
/// use serde_json::json;
///
/// let length = |unit| Rule::Length { min: Some(8), max: Some(64), unit };
///
/// let mut bytes = json!({ "type": "string" });
/// Constraints::new(vec![length(LengthUnit::Bytes)]).apply(&mut bytes);
/// assert_eq!(bytes, json!({ "type": "string", "minLength": 2, "maxLength": 64 }));
///
/// let mut chars = json!({ "type": "string" });
/// Constraints::new(vec![length(LengthUnit::Chars)]).apply(&mut chars);
/// assert_eq!(chars, json!({ "type": "string", "minLength": 8, "maxLength": 64 }));
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum LengthUnit {
    /// Unicode scalar values, like JSON Schema (`validator`, garde `chars`).
    Chars,
    /// UTF-8 bytes (garde's default for strings and `bytes`).
    Bytes,
    /// UTF-16 code units (garde `utf16`).
    Utf16,
    /// Grapheme clusters (garde `graphemes`).
    Graphemes,
}

/// One validation rule.
///
/// # Examples
///
/// ```
/// use oas_forge::attrs::{Constraints, Rule};
/// use serde_json::json;
///
/// let mut percent = json!({ "type": "integer" });
/// Constraints::new(vec![Rule::Range {
///     min: Some(0.into()),
///     max: None,
///     exclusive_min: None,
///     exclusive_max: Some(100.into()),
/// }])
/// .apply(&mut percent);
/// assert_eq!(
///     percent,
///     json!({ "type": "integer", "minimum": 0, "exclusiveMaximum": 100 })
/// );
///
/// // garde's `inner(email)` applies to the items of a list.
/// let mut emails = json!({ "type": "array", "items": { "type": "string" } });
/// Constraints::new(vec![Rule::Inner(vec![Rule::Format("email".into())])]).apply(&mut emails);
/// assert_eq!(
///     emails,
///     json!({ "type": "array", "items": { "type": "string", "format": "email" } })
/// );
/// ```
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub enum Rule {
    /// `length(min, max, equal)`
    Length {
        /// Lower bound.
        min: Option<u64>,
        /// Upper bound.
        max: Option<u64>,
        /// How string lengths are counted.
        unit: LengthUnit,
    },
    /// `range(min, max, exclusive_min, exclusive_max)`
    Range {
        /// Inclusive lower bound.
        min: Option<Number>,
        /// Inclusive upper bound.
        max: Option<Number>,
        /// Exclusive lower bound.
        exclusive_min: Option<Number>,
        /// Exclusive upper bound.
        exclusive_max: Option<Number>,
    },
    /// A string format such as `email`, `uri`, `ipv4`.
    Format(String),
    /// An IPv4 or IPv6 address.
    Ip,
    /// The value matches a regular expression.
    Pattern(String),
    /// The value does not match a regular expression.
    NotPattern(String),
    /// The value matches the regular expression in the constant at this
    /// path, as written (`RE` for `regex(path = *RE)`, `crate::re::RE`),
    /// resolved after all files are read.
    PatternConstant(String),
    /// The value must be present (`Option` fields become required).
    Required,
    /// Rules for the items of a collection or the value of an `Option`
    /// (garde `inner`).
    Inner(Vec<Rule>),
}

/// The validation rules of a field.
///
/// # Examples
///
/// The rules are read from `#[validate(...)]` and `#[garde(...)]` during
/// extraction:
///
/// ```
/// use oas_forge::diagnostics::Diagnostics;
/// use oas_forge::extract::{self, Item, Options};
/// use serde_json::json;
/// use std::path::Path;
///
/// let source = r#"
/// /// @openapi
/// pub struct SignUp {
///     #[validate(email, length(max = 255))]
///     pub email: String,
///     #[garde(range(min = 18, max = 150))]
///     pub age: u8,
///     #[validate(required)]
///     pub nick: Option<String>,
/// }
/// "#;
/// let items = extract::extract_str(
///     Path::new("src/lib.rs"),
///     source,
///     Options::new(),
///     &mut Diagnostics::new(),
/// )?;
/// let Some(Item::Schema(sign_up)) = items.first() else {
///     return Err("no schema".into());
/// };
/// assert_eq!(
///     sign_up.schema,
///     json!({
///         "type": "object",
///         "properties": {
///             "email": { "type": "string", "format": "email", "maxLength": 255 },
///             "age": { "type": "integer", "format": "int32", "minimum": 18, "maximum": 150 },
///             // `required`: the `Option` must be present and is not nullable.
///             "nick": { "type": "string" },
///         },
///         "required": ["email", "age", "nick"],
///     })
/// );
/// # Ok::<(), Box<dyn std::error::Error>>(())
/// ```
#[derive(Debug, Clone, Default, PartialEq)]
#[non_exhaustive]
pub struct Constraints {
    /// The rules in declaration order.
    pub rules: Vec<Rule>,
}

/// The marker key a [`Rule::PatternConstant`] leaves in the schema until the
/// constant is resolved.
pub(crate) const PATTERN_KEY: &str = "x-oas-forge-pattern";

impl Constraints {
    /// Constraints from rules.
    ///
    /// # Examples
    ///
    /// ```
    /// use oas_forge::attrs::{Constraints, LengthUnit, Rule};
    /// use serde_json::json;
    ///
    /// let constraints = Constraints::new(vec![
    ///     Rule::Format("email".into()),
    ///     Rule::Length { min: None, max: Some(255), unit: LengthUnit::Chars },
    /// ]);
    /// let mut schema = json!({ "type": "string" });
    /// constraints.apply(&mut schema);
    /// assert_eq!(schema, json!({ "type": "string", "format": "email", "maxLength": 255 }));
    /// ```
    pub fn new(rules: Vec<Rule>) -> Self {
        Self { rules }
    }

    /// Whether there is no rule.
    ///
    /// # Examples
    ///
    /// ```
    /// use oas_forge::attrs::{Constraints, Rule};
    ///
    /// assert!(Constraints::default().is_empty());
    /// assert!(!Constraints::new(vec![Rule::Required]).is_empty());
    /// ```
    pub fn is_empty(&self) -> bool {
        self.rules.is_empty()
    }

    /// Whether a `required` rule forces an `Option` field to be present.
    ///
    /// # Examples
    ///
    /// ```
    /// use oas_forge::attrs::{Constraints, Rule};
    ///
    /// assert!(Constraints::new(vec![Rule::Required]).requires_value());
    /// assert!(!Constraints::new(vec![Rule::Format("email".into())]).requires_value());
    /// ```
    pub fn requires_value(&self) -> bool {
        self.rules.contains(&Rule::Required)
    }

    /// Adds the keywords of every rule to `schema`, chosen by its `type`.
    ///
    /// # Examples
    ///
    /// ```
    /// use oas_forge::attrs::{Constraints, LengthUnit, Rule};
    /// use serde_json::json;
    ///
    /// let constraints = Constraints::new(vec![Rule::Length {
    ///     min: Some(1),
    ///     max: Some(10),
    ///     unit: LengthUnit::Chars,
    /// }]);
    /// let mut name = json!({ "type": "string" });
    /// let mut tags = json!({ "type": "array", "items": { "type": "string" } });
    /// let mut labels = json!({ "type": "object", "additionalProperties": { "type": "string" } });
    /// for schema in [&mut name, &mut tags, &mut labels] {
    ///     constraints.apply(schema);
    /// }
    /// assert_eq!(name, json!({ "type": "string", "minLength": 1, "maxLength": 10 }));
    /// assert_eq!(
    ///     tags,
    ///     json!({ "type": "array", "items": { "type": "string" }, "minItems": 1, "maxItems": 10 })
    /// );
    /// assert_eq!(
    ///     labels,
    ///     json!({
    ///         "type": "object",
    ///         "additionalProperties": { "type": "string" },
    ///         "minProperties": 1,
    ///         "maxProperties": 10,
    ///     })
    /// );
    /// ```
    pub fn apply(&self, schema: &mut Value) {
        for rule in &self.rules {
            apply_rule(rule, schema);
        }
    }

    /// Collects `#[validate(...)]` and `#[garde(...)]` rules. Values that are
    /// not usable literals are returned as problems, each a sentence such as
    /// "`length(max = MAX)` is not a literal" (their rule is left out).
    pub(crate) fn parse(attrs: &[Attribute]) -> (Self, Vec<String>) {
        let mut parser = Parser::default();
        for meta in metas(attrs, "validate") {
            parser.validator(&meta);
        }
        for meta in metas(attrs, "garde") {
            if let Some(rule) = parser.garde(&meta) {
                parser.rules.push(rule);
            }
        }
        (Self::new(parser.rules), parser.problems)
    }
}

/// Collects the rules of a field's validation attributes.
#[derive(Default)]
struct Parser {
    /// The rules in declaration order (`validator` before `garde`).
    rules: Vec<Rule>,
    /// Why values were left out, one sentence each ("`length(max = MAX)` is
    /// not a literal").
    problems: Vec<String>,
}

impl Parser {
    /// One item of `#[validate(...)]`.
    fn validator(&mut self, meta: &Meta) {
        let Some(name) = meta.path().get_ident().map(ToString::to_string) else {
            return;
        };
        let rule = match (name.as_str(), meta) {
            ("email", _) => Rule::Format("email".into()),
            ("url", _) => Rule::Format("uri".into()),
            ("ip", Meta::List(list)) => {
                let args = args(list);
                let flag = |n: &str| args.iter().any(|a| a.path().is_ident(n) && is_true(a));
                match (flag("v4"), flag("v6")) {
                    (true, false) => Rule::Format("ipv4".into()),
                    (false, true) => Rule::Format("ipv6".into()),
                    _ => Rule::Ip,
                }
            }
            ("ip", _) => Rule::Ip,
            ("required" | "required_nested", _) => Rule::Required,
            ("length", Meta::List(list)) => {
                let Some((min, max)) = self.bounds(&args(list)) else {
                    return;
                };
                Rule::Length {
                    min,
                    max,
                    unit: LengthUnit::Chars,
                }
            }
            ("range", Meta::List(list)) => match self.range(&args(list)) {
                Some(rule) => rule,
                None => return,
            },
            // `contains(pattern = "x")` (0.16+) and `contains = "x"` (older).
            ("contains", _) => match self.literal_arg(meta, "pattern") {
                Some(text) => Rule::Pattern(escape(&text)),
                None => return,
            },
            ("does_not_contain", _) => match self.literal_arg(meta, "pattern") {
                Some(text) => Rule::NotPattern(escape(&text)),
                None => return,
            },
            // `regex(path = *RE)`, `regex(path = "RE")`, `regex = "RE"`.
            ("regex", _) => match path_arg(meta, "path") {
                Some(name) => Rule::PatternConstant(name),
                None => return,
            },
            // 0.1 accepted a literal pattern.
            ("pattern", Meta::NameValue(nv)) => match string_literal(&nv.value) {
                Some(pattern) => Rule::Pattern(pattern),
                None => return,
            },
            _ => return,
        };
        self.rules.push(rule);
    }

    /// One item of `#[garde(...)]` (also inside `inner(...)`).
    fn garde(&mut self, meta: &Meta) -> Option<Rule> {
        let name = meta.path().get_ident()?.to_string();
        Some(match (name.as_str(), meta) {
            ("email", _) => Rule::Format("email".into()),
            ("url", _) => Rule::Format("uri".into()),
            ("ip", _) => Rule::Ip,
            ("ipv4", _) => Rule::Format("ipv4".into()),
            ("ipv6", _) => Rule::Format("ipv6".into()),
            ("ascii", _) => Rule::Pattern("^[\\x00-\\x7F]*$".into()),
            ("required", _) => Rule::Required,
            ("length" | "byte_length" | "char_count", Meta::List(list)) => {
                let args = args(list);
                let mut unit = match name.as_str() {
                    "char_count" => LengthUnit::Chars,
                    _ => LengthUnit::Bytes,
                };
                for arg in &args {
                    if let Meta::Path(path) = arg {
                        unit = match path.get_ident()?.to_string().as_str() {
                            "chars" => LengthUnit::Chars,
                            "utf16" => LengthUnit::Utf16,
                            "graphemes" => LengthUnit::Graphemes,
                            _ => LengthUnit::Bytes,
                        };
                    }
                }
                let (min, max) = self.bounds(&args)?;
                Rule::Length { min, max, unit }
            }
            ("range", Meta::List(list)) => self.range(&args(list))?,
            ("contains" | "prefix" | "suffix" | "pattern", Meta::List(list)) => {
                let expr: Expr = list.parse_args().ok()?;
                let constant = || expr_path(&expr).filter(|_| name == "pattern");
                match (name.as_str(), string_literal(&expr), constant()) {
                    ("pattern", Some(pattern), _) => Rule::Pattern(pattern),
                    (_, None, Some(constant)) => Rule::PatternConstant(constant),
                    ("contains", Some(text), _) => Rule::Pattern(escape(&text)),
                    ("prefix", Some(text), _) => Rule::Pattern(format!("^{}", escape(&text))),
                    ("suffix", Some(text), _) => Rule::Pattern(format!("{}$", escape(&text))),
                    _ => {
                        self.problems.push(format!(
                            "`{name}` argument `{}` is not a literal",
                            quote_expr(&expr)
                        ));
                        return None;
                    }
                }
            }
            ("inner", Meta::List(list)) => {
                let rules: Vec<Rule> = args(list).iter().filter_map(|m| self.garde(m)).collect();
                if rules.is_empty() {
                    return None;
                }
                Rule::Inner(rules)
            }
            _ => return None,
        })
    }

    /// `min`, `max` and `equal` of a `length` rule.
    fn bounds(&mut self, args: &[Meta]) -> Option<(Option<u64>, Option<u64>)> {
        let mut min = None;
        let mut max = None;
        for arg in args {
            let Meta::NameValue(nv) = arg else { continue };
            let Some(key) = nv.path.get_ident().map(ToString::to_string) else {
                continue;
            };
            if !matches!(key.as_str(), "min" | "max" | "equal") {
                continue;
            }
            let Some(value) = number(&nv.value).and_then(|n| n.as_u64()) else {
                self.problems.push(format!(
                    "`length({key} = {})` {}",
                    quote_expr(&nv.value),
                    unusable(&nv.value)
                ));
                continue;
            };
            match key.as_str() {
                "min" => min = Some(value),
                "max" => max = Some(value),
                _ => {
                    min = Some(value);
                    max = Some(value);
                }
            }
        }
        (min.is_some() || max.is_some()).then_some((min, max))
    }

    /// `min`, `max`, `exclusive_min`, `exclusive_max` and `equal` of a
    /// `range` rule; `None` without a usable bound.
    fn range(&mut self, args: &[Meta]) -> Option<Rule> {
        let mut min = None;
        let mut max = None;
        let mut exclusive_min = None;
        let mut exclusive_max = None;
        for arg in args {
            let Meta::NameValue(nv) = arg else { continue };
            let Some(key) = nv.path.get_ident().map(ToString::to_string) else {
                continue;
            };
            if !matches!(
                key.as_str(),
                "min" | "max" | "exclusive_min" | "exclusive_max" | "equal"
            ) {
                continue;
            }
            let Some(value) = number(&nv.value) else {
                self.problems.push(format!(
                    "`range({key} = {})` {}",
                    quote_expr(&nv.value),
                    unusable(&nv.value)
                ));
                continue;
            };
            match key.as_str() {
                "min" => min = Some(value),
                "max" => max = Some(value),
                "exclusive_min" => exclusive_min = Some(value),
                "exclusive_max" => exclusive_max = Some(value),
                _ => {
                    min = Some(value.clone());
                    max = Some(value);
                }
            }
        }
        if min.is_none() && max.is_none() && exclusive_min.is_none() && exclusive_max.is_none() {
            return None;
        }
        Some(Rule::Range {
            min,
            max,
            exclusive_min,
            exclusive_max,
        })
    }

    /// A string argument given as `name(key = "x")`, `name = "x"` or `name("x")`.
    fn literal_arg(&mut self, meta: &Meta, key: &str) -> Option<String> {
        match meta {
            Meta::NameValue(nv) => string_literal(&nv.value),
            Meta::List(list) => {
                if let Ok(expr) = list.parse_args::<Expr>()
                    && let Some(text) = string_literal(&expr)
                {
                    return Some(text);
                }
                args(list).iter().find_map(|arg| match arg {
                    Meta::NameValue(nv) if nv.path.is_ident(key) => string_literal(&nv.value),
                    _ => None,
                })
            }
            Meta::Path(_) => None,
        }
    }
}

/// The comma-separated items of a nested list.
fn args(list: &syn::MetaList) -> Vec<Meta> {
    list.parse_args_with(Punctuated::<Meta, syn::Token![,]>::parse_terminated)
        .map(|args| args.into_iter().collect())
        .unwrap_or_default()
}

/// `v4` or `v4 = true`.
fn is_true(meta: &Meta) -> bool {
    match meta {
        Meta::Path(_) => true,
        Meta::NameValue(nv) => matches!(
            &nv.value,
            Expr::Lit(ExprLit { lit: Lit::Bool(b), .. }) if b.value
        ),
        Meta::List(_) => false,
    }
}

/// The path of the constant named by `regex(path = *RE)` /
/// `regex(path = "RE")` / `regex = "RE"`, segments joined with `::`.
fn path_arg(meta: &Meta, key: &str) -> Option<String> {
    let expr = match meta {
        Meta::NameValue(nv) => nv.value.clone(),
        Meta::List(list) => args(list).into_iter().find_map(|arg| match arg {
            Meta::NameValue(nv) if nv.path.is_ident(key) => Some(nv.value),
            _ => None,
        })?,
        Meta::Path(_) => return None,
    };
    match string_literal(&expr) {
        Some(text) => Some(
            text.split("::")
                .map(str::trim)
                .collect::<Vec<_>>()
                .join("::"),
        ),
        None => expr_path(&expr),
    }
}

/// The path of `RE`, `*RE`, `&RE` or `crate::re::RE`, segments joined with
/// `::`.
fn expr_path(expr: &Expr) -> Option<String> {
    match expr {
        Expr::Path(p) => Some(
            p.path
                .segments
                .iter()
                .map(|s| s.ident.to_string())
                .collect::<Vec<_>>()
                .join("::"),
        ),
        Expr::Unary(ExprUnary { expr, .. }) | Expr::Reference(syn::ExprReference { expr, .. }) => {
            expr_path(expr)
        }
        Expr::Paren(p) => expr_path(&p.expr),
        _ => None,
    }
}

/// A numeric literal, possibly negative.
fn number(expr: &Expr) -> Option<Number> {
    match expr {
        Expr::Lit(ExprLit {
            lit: Lit::Int(i), ..
        }) => i.base10_parse::<u64>().ok().map(Number::from),
        Expr::Lit(ExprLit {
            lit: Lit::Float(f), ..
        }) => f.base10_parse::<f64>().ok().and_then(Number::from_f64),
        Expr::Unary(ExprUnary {
            op: UnOp::Neg(_),
            expr,
            ..
        }) => {
            let n = number(expr)?;
            if let Some(value) = n.as_u64() {
                // Down to `i64::MIN`, whose magnitude is not an `i64`.
                i64::try_from(-i128::from(value)).ok().map(Number::from)
            } else if let Some(value) = n.as_i64() {
                value.checked_neg().map(Number::from)
            } else {
                Number::from_f64(-n.as_f64()?)
            }
        }
        Expr::Paren(p) => number(&p.expr),
        _ => None,
    }
}

/// Why a bound is left out: a literal outside the 64-bit range, or not a
/// literal at all.
fn unusable(expr: &Expr) -> &'static str {
    match expr {
        Expr::Lit(ExprLit {
            lit: Lit::Int(_) | Lit::Float(_),
            ..
        }) => "is out of range",
        Expr::Unary(ExprUnary {
            op: UnOp::Neg(_),
            expr,
            ..
        }) => unusable(expr),
        Expr::Paren(p) => unusable(&p.expr),
        _ => "is not a literal",
    }
}

/// The source text of `expr`, for messages.
fn quote_expr(expr: &Expr) -> String {
    quote::ToTokens::to_token_stream(expr).to_string()
}

/// `text` as a regular expression that matches it literally.
fn escape(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for c in text.chars() {
        if "\\^$.|?*+()[]{}".contains(c) {
            out.push('\\');
        }
        out.push(c);
    }
    out
}

/// The kind of value a schema describes, by its `type`: rules pick other
/// keywords for arrays and objects than for everything else.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Kind {
    /// `array`
    Array,
    /// `object`
    Object,
    /// Any other or no `type`: a string, a number, a `$ref`, ...
    Other,
}

/// The kind of `schema`; of a list of types (`["string", "null"]`), the
/// first that is not `null`.
fn kind(schema: &Value) -> Kind {
    let type_name = match schema.get("type") {
        Some(Value::String(t)) => t.as_str(),
        Some(Value::Array(types)) => types
            .iter()
            .filter_map(Value::as_str)
            .find(|t| *t != "null")
            .unwrap_or_default(),
        _ => "",
    };
    match type_name {
        "array" => Kind::Array,
        "object" => Kind::Object,
        _ => Kind::Other,
    }
}

/// Adds the keywords of `rule` to `schema`, chosen by its [`Kind`]; a
/// schema that is not an object is left as it is.
fn apply_rule(rule: &Rule, schema: &mut Value) {
    let kind = kind(schema);
    let Value::Object(map) = schema else { return };
    match rule {
        Rule::Length { min, max, unit } => {
            let (min_key, max_key) = match kind {
                Kind::Array => ("minItems", "maxItems"),
                Kind::Object => ("minProperties", "maxProperties"),
                _ => ("minLength", "maxLength"),
            };
            let strings = !matches!(kind, Kind::Array | Kind::Object);
            // JSON Schema counts characters; byte and UTF-16 limits are turned
            // into the character bounds every valid value satisfies.
            let (min, max) = match (strings, unit) {
                (false, _) | (true, LengthUnit::Chars) => (*min, *max),
                (true, LengthUnit::Bytes) => (min.map(|n| n.div_ceil(4)), *max),
                (true, LengthUnit::Utf16) => (min.map(|n| n.div_ceil(2)), *max),
                (true, LengthUnit::Graphemes) => (*min, None),
            };
            if let Some(min) = min {
                map.insert(min_key.into(), json!(min));
            }
            if let Some(max) = max {
                map.insert(max_key.into(), json!(max));
            }
        }
        Rule::Range {
            min,
            max,
            exclusive_min,
            exclusive_max,
        } => {
            for (key, value) in [
                ("minimum", min),
                ("maximum", max),
                ("exclusiveMinimum", exclusive_min),
                ("exclusiveMaximum", exclusive_max),
            ] {
                if let Some(value) = value {
                    map.insert(key.into(), Value::Number(value.clone()));
                }
            }
        }
        Rule::Format(format) => {
            map.insert("format".into(), json!(format));
        }
        Rule::Ip => {
            map.remove("format");
            add_subschema(
                map,
                "anyOf",
                json!([{ "format": "ipv4" }, { "format": "ipv6" }]),
            );
        }
        Rule::Pattern(pattern) => add_subschema(map, "pattern", json!(pattern)),
        Rule::NotPattern(pattern) => {
            add_subschema(map, "not", json!({ "pattern": pattern }));
        }
        Rule::PatternConstant(name) => {
            map.insert(PATTERN_KEY.into(), json!(name));
        }
        Rule::Required => {}
        Rule::Inner(rules) => {
            let target = match kind {
                Kind::Array => map.get_mut("items"),
                Kind::Object => map.get_mut("additionalProperties"),
                _ => None,
            };
            match target {
                Some(inner) => rules.iter().for_each(|r| apply_rule(r, inner)),
                None => rules.iter().for_each(|r| apply_rule(r, schema)),
            }
        }
    }
}

/// Adds `key: value`, or a subschema `{key: value}` to `allOf` when `key`
/// is taken already (both must hold).
fn add_subschema(map: &mut Map<String, Value>, key: &str, value: Value) {
    if !map.contains_key(key) {
        map.insert(key.to_string(), value);
        return;
    }
    let mut sub = Map::new();
    sub.insert(key.to_string(), value);
    match map.get_mut("allOf") {
        Some(Value::Array(all)) => all.push(Value::Object(sub)),
        _ => {
            map.insert("allOf".into(), json!([sub]));
        }
    }
}

/// The regular expression constants of a run, for resolving the paths of
/// [`Rule::PatternConstant`]s.
///
/// Constants are found by name. A path with modules (`crate::a::RE`,
/// `a::RE`) selects the constants whose file stands for a module path
/// ending with those modules (`src/a.rs`, `src/x/a/mod.rs`); a bare name
/// prefers the constant in the referencing file, as Rust resolves it in
/// scope. Constants in inline `mod` blocks count as part of their file's
/// module.
#[derive(Debug, Default)]
pub(crate) struct Patterns {
    /// The definitions of each constant name, in the order they were found.
    by_name: BTreeMap<String, Vec<PatternItem>>,
}

/// What the path of a pattern constant resolves to.
enum Resolution<'a> {
    /// The pattern of the constant (or of all candidates, which agree).
    Pattern(&'a str),
    /// No constant of that name is defined.
    Unknown,
    /// Constants with different patterns match; where they are defined.
    Ambiguous(Vec<&'a Location>),
}

impl Patterns {
    /// Adds a constant definition.
    pub(crate) fn add(&mut self, item: PatternItem) {
        self.by_name
            .entry(item.name.clone())
            .or_default()
            .push(item);
    }

    /// The constant `path` (as written in the attribute) refers to from the
    /// file `from`.
    fn resolve(&self, path: &str, from: &Path) -> Resolution<'_> {
        let segments: Vec<&str> = path.split("::").collect();
        let Some((name, modules)) = segments.split_last() else {
            return Resolution::Unknown;
        };
        let Some(definitions) = self.by_name.get(*name) else {
            return Resolution::Unknown;
        };
        // `crate::`, `self::` and `super::` say nothing about the module
        // names that follow.
        let modules: Vec<&str> = modules
            .iter()
            .copied()
            .skip_while(|s| matches!(*s, "crate" | "self" | "super"))
            .collect();
        let preferred: Vec<&PatternItem> = definitions
            .iter()
            .filter(|d| match modules.as_slice() {
                [] => d.location.file() == from,
                modules => in_modules(d.location.file(), modules),
            })
            .collect();
        let candidates = if preferred.is_empty() {
            definitions.iter().collect()
        } else {
            preferred
        };
        match candidates.as_slice() {
            [first, rest @ ..] if rest.iter().all(|d| d.pattern == first.pattern) => {
                Resolution::Pattern(&first.pattern)
            }
            _ => Resolution::Ambiguous(candidates.iter().map(|d| &d.location).collect()),
        }
    }
}

/// Whether the module path of `file` ([`file_modules`]) ends with
/// `modules`.
fn in_modules(file: &Path, modules: &[&str]) -> bool {
    let path = file_modules(file);
    modules.len() <= path.len()
        && path
            .iter()
            .rev()
            .zip(modules.iter().rev())
            .all(|(a, b)| a == b)
}

/// The module path a source file stands for: its path without the
/// extension, where `lib.rs`, `main.rs` and `mod.rs` stand for their
/// directory (`src/a/mod.rs` and `src/a.rs` are both `src::a`).
fn file_modules(file: &Path) -> Vec<String> {
    let mut modules: Vec<String> = file
        .with_extension("")
        .components()
        .filter_map(|c| match c {
            std::path::Component::Normal(name) => Some(name.to_string_lossy().into_owned()),
            _ => None,
        })
        .collect();
    if modules
        .last()
        .is_some_and(|last| matches!(last.as_str(), "lib" | "main" | "mod"))
    {
        modules.pop();
    }
    modules
}

/// Replaces the [`PATTERN_KEY`] markers in `value` with the `pattern` of the
/// constant they name ([`Patterns`]); unknown and ambiguous constants are
/// reported and dropped.
pub(crate) fn resolve_patterns(
    value: &mut Value,
    patterns: &Patterns,
    location: &Location,
    diagnostics: &mut Diagnostics,
) {
    match value {
        Value::Object(map) => {
            if let Some(Value::String(path)) = map.remove(PATTERN_KEY) {
                match patterns.resolve(&path, location.file()) {
                    Resolution::Pattern(pattern) => {
                        let mut schema = Value::Object(std::mem::take(map));
                        apply_rule(&Rule::Pattern(pattern.to_string()), &mut schema);
                        if let Value::Object(restored) = schema {
                            *map = restored;
                        }
                    }
                    Resolution::Unknown => diagnostics.push(
                        Diagnostic::new(
                            Code::UnresolvedPattern,
                            format!("regular expression constant `{path}` is not defined in the scanned code; the pattern is not documented"),
                        )
                        .at(location.clone()),
                    ),
                    Resolution::Ambiguous(mut definitions) => diagnostics.push({
                        definitions.sort();
                        Diagnostic::new(
                            Code::AmbiguousPattern,
                            format!(
                                "regular expression constant `{path}` matches constants with different patterns ({}); the pattern is not documented",
                                definitions
                                    .iter()
                                    .map(ToString::to_string)
                                    .collect::<Vec<_>>()
                                    .join(", ")
                            ),
                        )
                        .at(location.clone())
                    }),
                }
            }
            map.values_mut()
                .for_each(|v| resolve_patterns(v, patterns, location, diagnostics));
        }
        Value::Array(items) => items
            .iter_mut()
            .for_each(|v| resolve_patterns(v, patterns, location, diagnostics)),
        _ => {}
    }
}
