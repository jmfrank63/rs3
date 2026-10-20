//! Store-backed routes: `POST /insert`, `GET /key/{key}`, `DELETE /key/{key}` and `GET /list`.
use actix_web::{HttpResponse, Responder, delete, get, post, web};

use super::error::ApiError;
use crate::state::AppState;
use crate::store::{StoreError, legacy};

/// Register the store routes. The shared `AppState` is registered by `http::configure`.
pub fn configure(cfg: &mut web::ServiceConfig) {
    cfg.service(insert)
        .service(get_key)
        .service(delete_key)
        .service(list);
}

/// `POST /insert` stores every key/value of a JSON object of strings and echoes it.
///
/// The body is read as raw bytes, whatever its content type or charset
/// parameter, and decoded as strict UTF-8. It is fully parsed and validated
/// before anything is written. Invalid UTF-8 or malformed JSON answers a 400
/// `malformed_json` JSON error; valid JSON of the wrong shape answers a 422
/// `invalid_input` JSON error naming the offending key.
#[post("/insert")]
pub async fn insert(
    state: web::Data<AppState>,
    body: web::Bytes,
) -> Result<HttpResponse, ApiError> {
    let text = std::str::from_utf8(&body)
        .map_err(|e| StoreError::MalformedJson(format!("body is not valid UTF-8: {e}")))?;
    let entries = legacy::parse_insert_body(text)?;
    let echo = legacy::format_insert_echo(&entries);
    state.store.insert_many(entries.into_iter().map(Ok))?;
    Ok(HttpResponse::Ok().json(echo))
}

/// `GET /key/{key}` returns the stored value, or a 404 JSON error for a missing key.
#[get("/key/{key}")]
pub async fn get_key(
    state: web::Data<AppState>,
    path: web::Path<String>,
) -> Result<HttpResponse, ApiError> {
    Ok(HttpResponse::Ok().json(state.store.get(&path.into_inner())?))
}

/// `DELETE /key/{key}` removes the key and returns the value it held, or a 404 JSON error.
#[delete("/key/{key}")]
pub async fn delete_key(
    state: web::Data<AppState>,
    path: web::Path<String>,
) -> Result<HttpResponse, ApiError> {
    Ok(HttpResponse::Ok().json(state.store.remove(&path.into_inner())?))
}

/// `GET /list` returns every stored pair, sorted by key, in the legacy list shape.
#[get("/list")]
pub async fn list(state: web::Data<AppState>) -> impl Responder {
    HttpResponse::Ok().json(legacy::format_list(&state.store.list()))
}

#[cfg(test)]
mod tests {
    use crate::state::test_state;
    use crate::store::StoreError;
    use actix_web::{App, http::StatusCode, test as atest, web};

    fn insert_request(payload: &'static str) -> atest::TestRequest {
        atest::TestRequest::post()
            .insert_header(("Content-Type", "application/json"))
            .set_payload(payload)
            .uri("/insert")
    }

    #[actix_web::test]
    async fn test_insert_is_ok() {
        let state = test_state();
        let app =
            atest::init_service(App::new().configure(crate::http::configure(state.clone()))).await;
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
        let state = test_state();
        let app =
            atest::init_service(App::new().configure(crate::http::configure(state.clone()))).await;
        let body = atest::call_and_read_body(
            &app,
            insert_request(r#"{"insert_test":"John Doe"}"#).to_request(),
        )
        .await;
        assert_eq!(
            body,
            web::Bytes::from_static(br#""{\"insert_test\":\"John Doe\"}""#)
        );
        assert_eq!(state.store.get("insert_test"), Ok("John Doe".to_owned()));
    }

    #[actix_web::test]
    async fn test_get_is_ok() {
        let state = test_state();
        state.store.insert("get_test".into(), "John Doe".into());
        let app =
            atest::init_service(App::new().configure(crate::http::configure(state.clone()))).await;
        let req = atest::TestRequest::get()
            .uri("/key/get_test")
            .insert_header(("Content-Type", "application/json"))
            .to_request();
        let resp = atest::call_service(&app, req).await;
        assert_eq!(resp.status(), StatusCode::OK);
        assert_eq!(atest::read_body(resp).await, r#""John Doe""#.as_bytes());
    }

    #[actix_web::test]
    async fn get_missing_is_404_json() {
        let state = test_state();
        state.store.insert("other".into(), "v".into());
        let app =
            atest::init_service(App::new().configure(crate::http::configure(state.clone()))).await;
        let resp = atest::call_service(
            &app,
            atest::TestRequest::get().uri("/key/nope").to_request(),
        )
        .await;
        crate::http::tests::assert_json_error(resp, StatusCode::NOT_FOUND, "not_found").await;
    }

    #[actix_web::test]
    async fn test_delete_is_ok() {
        let state = test_state();
        state.store.insert("delete_test".into(), "John Doe".into());
        let app =
            atest::init_service(App::new().configure(crate::http::configure(state.clone()))).await;
        let req = atest::TestRequest::delete()
            .uri("/key/delete_test")
            .to_request();
        let resp = atest::call_service(&app, req).await;
        assert_eq!(resp.status(), StatusCode::OK);
        assert_eq!(
            atest::read_body(resp).await,
            web::Bytes::from_static(br#""John Doe""#)
        );
        assert_eq!(
            state.store.get("delete_test"),
            Err(StoreError::NotFound("delete_test".into()))
        );
    }

    #[actix_web::test]
    async fn test_list_is_ok() {
        let state = test_state();
        state.store.insert("list_test".into(), "John Doe".into());
        let app =
            atest::init_service(App::new().configure(crate::http::configure(state.clone()))).await;
        let req = atest::TestRequest::get().uri("/list").to_request();
        let resp = atest::call_service(&app, req).await;
        assert_eq!(resp.status(), StatusCode::OK);
        let body = atest::read_body(resp).await;
        assert!(body.starts_with(br#""["#), "body: {body:?}");
        assert!(body.ends_with(br#"]""#), "body: {body:?}");
    }

    #[actix_web::test]
    async fn test_insert_same_body_twice_is_idempotent() {
        let state = test_state();
        let app =
            atest::init_service(App::new().configure(crate::http::configure(state.clone()))).await;
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
        assert_eq!(state.store.get("idem_test"), Ok("v".to_owned()));
        assert_eq!(state.store.len(), 1);
    }

    #[actix_web::test]
    async fn get_on_empty_store_is_404_json() {
        let state = test_state();
        let app =
            atest::init_service(App::new().configure(crate::http::configure(state.clone()))).await;
        let resp =
            atest::call_service(&app, atest::TestRequest::get().uri("/key/any").to_request()).await;
        let body =
            crate::http::tests::assert_json_error(resp, StatusCode::NOT_FOUND, "not_found").await;
        assert_ne!(body, serde_json::Value::from("[]"));
    }

    #[actix_web::test]
    async fn get_missing_twice_is_404_and_creates_nothing() {
        let state = test_state();
        let app =
            atest::init_service(App::new().configure(crate::http::configure(state.clone()))).await;
        for _ in 0..2 {
            let resp = atest::call_service(
                &app,
                atest::TestRequest::get().uri("/key/ghost").to_request(),
            )
            .await;
            crate::http::tests::assert_json_error(resp, StatusCode::NOT_FOUND, "not_found").await;
        }
        assert_eq!(state.store.len(), 0);
    }

    #[actix_web::test]
    async fn delete_missing_is_404_json() {
        let state = test_state();
        let app =
            atest::init_service(App::new().configure(crate::http::configure(state.clone()))).await;
        let resp = atest::call_service(
            &app,
            atest::TestRequest::delete().uri("/key/nope").to_request(),
        )
        .await;
        crate::http::tests::assert_json_error(resp, StatusCode::NOT_FOUND, "not_found").await;
    }

    #[actix_web::test]
    async fn delete_twice_is_200_then_404() {
        let state = test_state();
        state.store.insert("d".into(), "v".into());
        let app =
            atest::init_service(App::new().configure(crate::http::configure(state.clone()))).await;
        let first = atest::call_service(
            &app,
            atest::TestRequest::delete().uri("/key/d").to_request(),
        )
        .await;
        assert_eq!(first.status(), StatusCode::OK);
        assert_eq!(
            atest::read_body(first).await,
            web::Bytes::from_static(br#""v""#)
        );
        let second = atest::call_service(
            &app,
            atest::TestRequest::delete().uri("/key/d").to_request(),
        )
        .await;
        crate::http::tests::assert_json_error(second, StatusCode::NOT_FOUND, "not_found").await;
    }

    async fn assert_insert_400(payload: Vec<u8>) {
        let state = test_state();
        let app =
            atest::init_service(App::new().configure(crate::http::configure(state.clone()))).await;
        let resp = atest::call_service(
            &app,
            atest::TestRequest::post()
                .uri("/insert")
                .insert_header(("Content-Type", "application/json"))
                .set_payload(payload)
                .to_request(),
        )
        .await;
        crate::http::tests::assert_json_error(resp, StatusCode::BAD_REQUEST, "malformed_json")
            .await;
        assert_eq!(state.store.len(), 0);
    }

    #[actix_web::test]
    async fn insert_malformed_is_400_json() {
        assert_insert_400(b"{bad".to_vec()).await;
    }

    #[actix_web::test]
    async fn insert_empty_body_is_400_json() {
        assert_insert_400(Vec::new()).await;
    }

    #[actix_web::test]
    async fn insert_invalid_utf8_is_400_json() {
        assert_insert_400(vec![0xFF, 0xFE]).await;
    }

    #[actix_web::test]
    async fn insert_deeply_nested_is_400_json() {
        assert_insert_400("[".repeat(200).into_bytes()).await;
    }

    #[actix_web::test]
    async fn insert_without_content_type_is_ok() {
        let state = test_state();
        let app =
            atest::init_service(App::new().configure(crate::http::configure(state.clone()))).await;
        let req = atest::TestRequest::post()
            .uri("/insert")
            .set_payload(r#"{"n":"1"}"#)
            .to_request();
        assert!(req.head().headers.get("content-type").is_none());
        let resp = atest::call_service(&app, req).await;
        assert_eq!(resp.status(), StatusCode::OK);
        assert_eq!(
            atest::read_body(resp).await,
            web::Bytes::from_static(br#""{\"n\":\"1\"}""#)
        );
        assert_eq!(state.store.get("n"), Ok("1".to_owned()));
    }

    #[actix_web::test]
    async fn insert_with_bogus_charset_is_ok() {
        let state = test_state();
        let app =
            atest::init_service(App::new().configure(crate::http::configure(state.clone()))).await;
        let req = atest::TestRequest::post()
            .uri("/insert")
            .insert_header(("Content-Type", "text/plain; charset=bogus"))
            .set_payload(r#"{"c":"d"}"#)
            .to_request();
        let resp = atest::call_service(&app, req).await;
        assert_eq!(resp.status(), StatusCode::OK);
        assert_eq!(
            atest::read_body(resp).await,
            web::Bytes::from_static(br#""{\"c\":\"d\"}""#)
        );
        assert_eq!(state.store.get("c"), Ok("d".to_owned()));
    }

    #[actix_web::test]
    async fn test_insert_echo_preserves_body_order() {
        let state = test_state();
        let app =
            atest::init_service(App::new().configure(crate::http::configure(state.clone()))).await;
        let body =
            atest::call_and_read_body(&app, insert_request(r#"{"b":"2","a":"1"}"#).to_request())
                .await;
        assert_eq!(
            body,
            web::Bytes::from_static(br#""{\"b\":\"2\",\"a\":\"1\"}""#)
        );
    }
}
