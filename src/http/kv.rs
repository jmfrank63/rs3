//! Store-backed routes: `POST /insert` and `GET /key/{key}`.
use std::sync::LazyLock;

use actix_web::{HttpResponse, Responder, get, post, web};

use crate::store::{Store, legacy};

// TODO(phase-2): inject via web::Data
pub(crate) static STORE: LazyLock<Store> = LazyLock::new(Store::new);

/// Register the store routes.
pub fn configure(cfg: &mut web::ServiceConfig) {
    cfg.service(insert).service(get_key);
}

/// `POST /insert` stores every key/value of a JSON object of strings and echoes it.
///
/// Phase 1 keeps the legacy behaviour of panicking on a bad body; Phase 2 adds 400/422.
#[post("/insert")]
pub async fn insert(body: String) -> impl Responder {
    let entries = legacy::parse_insert_body(&body).unwrap();
    let echo = legacy::format_insert_echo(&entries);
    STORE.insert_many(entries.into_iter().map(Ok)).unwrap();
    HttpResponse::Ok().json(echo)
}

/// `GET /key/{key}` returns the stored value.
///
/// Phase 1 keeps the legacy semantics: `"[]"` while the store is empty, and a
/// panic on a missing key. Phase 2 replaces both with 404.
#[get("/key/{key}")]
pub async fn get_key(path: web::Path<String>) -> impl Responder {
    let key = path.into_inner();
    if STORE.is_empty() {
        HttpResponse::Ok().json("[]")
    } else {
        HttpResponse::Ok().json(STORE.get(&key).unwrap())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use actix_web::{App, http::StatusCode, test as atest, web};

    fn insert_request(payload: &'static str) -> atest::TestRequest {
        atest::TestRequest::post()
            .insert_header(("Content-Type", "application/json"))
            .set_payload(payload)
            .uri("/insert")
    }

    #[actix_web::test]
    async fn test_insert_is_ok() {
        let app = atest::init_service(App::new().service(insert)).await;
        let resp = atest::call_service(
            &app,
            insert_request(r#"{"insert_test":"John Doe"}"#).to_request(),
        )
        .await;
        assert_eq!(resp.status(), StatusCode::OK);
        assert_eq!(
            atest::read_body(resp).await,
            web::Bytes::from_static(br#""{\"insert_test\":\"John Doe\"}""#)
        );
    }

    #[actix_web::test]
    async fn test_insert_map_entry_is_ok() {
        let app = atest::init_service(App::new().service(insert)).await;
        let body = atest::call_and_read_body(
            &app,
            insert_request(r#"{"insert_test":"John Doe"}"#).to_request(),
        )
        .await;
        assert_eq!(
            body,
            web::Bytes::from_static(br#""{\"insert_test\":\"John Doe\"}""#)
        );
    }

    #[actix_web::test]
    async fn test_get_is_ok() {
        STORE.insert("get_test".into(), "John Doe".into());
        let app = atest::init_service(App::new().service(get_key)).await;
        let req = atest::TestRequest::get()
            .uri("/key/get_test")
            .insert_header(("Content-Type", "application/json"))
            .to_request();
        let resp = atest::call_service(&app, req).await;
        assert_eq!(resp.status(), StatusCode::OK);
        assert_eq!(atest::read_body(resp).await, r#""John Doe""#.as_bytes());
    }

    #[actix_web::test]
    async fn test_insert_same_body_twice_is_idempotent() {
        let app = atest::init_service(App::new().service(insert)).await;
        let first =
            atest::call_and_read_body(&app, insert_request(r#"{"idem_test":"v"}"#).to_request())
                .await;
        let second =
            atest::call_and_read_body(&app, insert_request(r#"{"idem_test":"v"}"#).to_request())
                .await;
        assert_eq!(
            first,
            web::Bytes::from_static(br#""{\"idem_test\":\"v\"}""#)
        );
        assert_eq!(first, second);
        assert_eq!(STORE.get("idem_test"), Ok("v".to_owned()));
    }
}
