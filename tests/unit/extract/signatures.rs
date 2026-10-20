//! Signature inference from handler parameters and return types.

use super::routes;
use crate::common::extract;
use oas_forge::dsl::Signature;
use oas_forge::schema::TypeExpr;

fn signature(code: &str) -> Option<Signature> {
    routes(&extract(code))[0].signature.clone()
}

fn ty(text: &str) -> TypeExpr {
    TypeExpr::parse(text).unwrap()
}

#[test]
fn axum_extractors() {
    let found = signature(
        "/// @route GET /x/{id}\nasync fn h(State(s): State<Db>, Path(id): Path<u64>, Query(q): Query<Filter>, Json(b): Json<New>) -> Result<Json<User>, E> { todo!() }",
    )
    .unwrap();
    assert_eq!(found.path, [ty("u64")]);
    assert_eq!(found.query, Some(ty("Filter")));
    assert_eq!(found.body, Some((ty("New"), "application/json".into())));
    assert_eq!(found.response, Some(ty("User")));
}

#[test]
fn actix_extractors_and_tuples() {
    let found = signature(
        "/// @route POST /x/{a}/{b}\nasync fn h(path: web::Path<(String, u8)>, form: web::Form<Login>) -> actix_web::Result<web::Json<Vec<User>>> { todo!() }",
    )
    .unwrap();
    assert_eq!(found.path, [ty("String"), ty("u8")]);
    assert_eq!(
        found.body,
        Some((ty("Login"), "application/x-www-form-urlencoded".into()))
    );
    assert_eq!(found.response, Some(ty("Vec<User>")));
}

#[test]
fn multipart_and_status_tuples() {
    let found = signature(
        "/// @route POST /u\nasync fn h(m: Multipart) -> (StatusCode, Json<Done>) { todo!() }",
    )
    .unwrap();
    assert_eq!(found.body.unwrap().1, "multipart/form-data");
    assert_eq!(found.response, Some(ty("Done")));
}

#[test]
fn handlers_without_known_extractors_have_no_signature() {
    for code in [
        "/// @route GET /x\nfn h() {}",
        "/// @route GET /x\nasync fn h(State(s): State<Db>, e: Extension<A>) -> impl IntoResponse { todo!() }",
        "/// @route GET /x\nfn h(p: &Path) -> HttpResponse { todo!() }",
        "/// @route GET /x\nfn h(p: Path, q: Query) -> Result<StatusCode, E> { todo!() }",
        "/// @route GET /x\nfn h() -> Json { todo!() }",
    ] {
        assert_eq!(signature(code), None, "{code}");
    }
}

#[test]
fn methods_are_inferred_too() {
    let items = extract(
        "struct A;\nimpl A {\n/// @route GET /x\nasync fn h(&self, Json(b): Json<T>) {}\n}",
    );
    assert!(routes(&items)[0].signature.as_ref().unwrap().body.is_some());
}
