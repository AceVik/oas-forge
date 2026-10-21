//! Signature inference: what a handler's parameters and return type say
//! about its operation.
//!
//! Only the extractors of the table below are read; everything else
//! (`State`, `Extension`, headers, receivers) is ignored:
//!
//! | Extractor | Meaning |
//! | :--- | :--- |
//! | `Path<T>`, `web::Path<T>` | path parameters (`T`, a tuple in path order, or a struct) |
//! | `Query<T>`, `web::Query<T>` | query parameters from the fields of struct `T` |
//! | `Json<T>`, `web::Json<T>` | `application/json` body |
//! | `Form<T>`, `web::Form<T>` | `application/x-www-form-urlencoded` body |
//! | `Multipart` | `multipart/form-data` body |
//!
//! A body extractor in `Option` (`Option<Json<T>>`) is an optional body,
//! `Option<Query<T>>` makes every query parameter optional and
//! `Option<Path<T>>` is read like `Path<T>`.
//!
//! A return type of `Json<T>`, `Result<Json<T>, E>` or `(StatusCode, Json<T>)`
//! is the success response.

use crate::dsl::Signature;
use crate::schema::TypeExpr;

/// The signature of a handler, or `None` when it uses no known extractor.
pub(super) fn infer(signature: &syn::Signature) -> Option<Signature> {
    let mut out = Signature::default();
    for input in &signature.inputs {
        let syn::FnArg::Typed(argument) = input else {
            continue;
        };
        let ty = TypeExpr::from_syn(&argument.ty);
        // An extractor in `Option` may find nothing: an optional body or
        // optional query parameters. `Option<Path<T>>` documents the path
        // parameters of the routes that have them (they are always required).
        let (ty, optional) = match (ty.name(), ty.args()) {
            (Some("Option"), [inner])
                if matches!(
                    inner.name(),
                    Some("Json" | "Form" | "Multipart" | "Query" | "Path")
                ) =>
            {
                (inner.clone(), true)
            }
            _ => (ty, false),
        };
        let inner = ty.args().first().cloned();
        match ty.name() {
            Some("Path") => {
                out.path = match inner {
                    Some(TypeExpr::Tuple(elements)) => elements,
                    Some(single) => vec![single],
                    None => Vec::new(),
                };
            }
            Some("Query") => {
                out.query = inner;
                out.optional_query = optional;
            }
            Some("Json") => {
                out.body = inner.map(|t| (t, "application/json".to_string()));
                out.optional_body = optional;
            }
            Some("Form") => {
                out.body = inner.map(|t| (t, "application/x-www-form-urlencoded".to_string()));
                out.optional_body = optional;
            }
            Some("Multipart") => {
                out.body = TypeExpr::parse("serde_json::Map<String, Value>")
                    .map(|t| (t, "multipart/form-data".to_string()));
                out.optional_body = optional;
            }
            _ => {}
        }
    }
    if let syn::ReturnType::Type(_, ty) = &signature.output {
        out.response = response(&TypeExpr::from_syn(ty));
    }
    (out != Signature::default()).then_some(out)
}

/// The body type of a response type.
fn response(ty: &TypeExpr) -> Option<TypeExpr> {
    match ty {
        TypeExpr::Tuple(elements) => elements.last().and_then(response),
        _ => match ty.name()? {
            "Result" => ty.args().first().and_then(response),
            "Json" => ty.args().first().cloned(),
            _ => None,
        },
    }
}
