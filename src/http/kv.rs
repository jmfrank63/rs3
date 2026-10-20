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
    state.store.insert_all(entries);
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
    async fn insert_truncated_deep_nesting_is_400_json() {
        assert_insert_400("[".repeat(200).into_bytes()).await;
    }

    #[actix_web::test]
    async fn insert_valid_deep_nesting_is_422_json() {
        let state = test_state();
        let app =
            atest::init_service(App::new().configure(crate::http::configure(state.clone()))).await;
        let deep = format!("{}{}", "[".repeat(200), "]".repeat(200));
        let resp = atest::call_service(&app, raw_insert(deep).to_request()).await;
        crate::http::tests::assert_json_error(
            resp,
            StatusCode::UNPROCESSABLE_ENTITY,
            "invalid_input",
        )
        .await;
        assert_eq!(state.store.len(), 0);
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

    fn raw_insert(payload: impl Into<Vec<u8>>) -> atest::TestRequest {
        atest::TestRequest::post()
            .uri("/insert")
            .set_payload(payload.into())
    }

    #[actix_web::test]
    async fn insert_non_object_is_422_json() {
        let state = test_state();
        let app =
            atest::init_service(App::new().configure(crate::http::configure(state.clone()))).await;
        let resp = atest::call_service(&app, raw_insert("[1,2]").to_request()).await;
        crate::http::tests::assert_json_error(
            resp,
            StatusCode::UNPROCESSABLE_ENTITY,
            "invalid_input",
        )
        .await;
        assert!(state.store.is_empty());
    }

    #[actix_web::test]
    async fn insert_non_string_value_is_422_and_inserts_nothing() {
        let state = test_state();
        let app =
            atest::init_service(App::new().configure(crate::http::configure(state.clone()))).await;
        let resp = atest::call_service(&app, raw_insert(r#"{"a":"1","b":2}"#).to_request()).await;
        let body = crate::http::tests::assert_json_error(
            resp,
            StatusCode::UNPROCESSABLE_ENTITY,
            "invalid_input",
        )
        .await;
        let message = body["message"].as_str().expect("string message");
        assert!(message.contains("key b"), "message: {message}");
        assert_eq!(state.store.get("a"), Err(StoreError::NotFound("a".into())));
        assert!(state.store.is_empty());
    }

    #[actix_web::test]
    async fn insert_out_of_range_number_is_422_naming_key() {
        let state = test_state();
        let app =
            atest::init_service(App::new().configure(crate::http::configure(state.clone()))).await;
        let resp = atest::call_service(&app, raw_insert(r#"{"a": 1e400}"#).to_request()).await;
        let body = crate::http::tests::assert_json_error(
            resp,
            StatusCode::UNPROCESSABLE_ENTITY,
            "invalid_input",
        )
        .await;
        let message = body["message"].as_str().expect("string message");
        assert!(message.contains("key a"), "message: {message}");
        assert!(state.store.is_empty());
    }

    #[actix_web::test]
    async fn insert_unpaired_surrogate_escape_is_400_json() {
        let state = test_state();
        let app =
            atest::init_service(App::new().configure(crate::http::configure(state.clone()))).await;
        for payload in [
            r#"{"a":"\ud800"}"#,
            r#"{"\ud800":"v"}"#,
            r#"{"ok":"1","a":"x\udc00y"}"#,
        ] {
            let resp = atest::call_service(&app, raw_insert(payload).to_request()).await;
            let body = crate::http::tests::assert_json_error(
                resp,
                StatusCode::BAD_REQUEST,
                "malformed_json",
            )
            .await;
            let message = body["message"].as_str().expect("string message");
            assert!(
                message.contains("unpaired UTF-16 surrogate"),
                "{payload}: message: {message}"
            );
            assert!(!message.contains("must be a string"), "message: {message}");
            assert!(!message.contains("JSON object"), "message: {message}");
        }
        assert!(state.store.is_empty());
    }

    #[actix_web::test]
    async fn insert_rejected_twice_leaves_store_unchanged() {
        let state = test_state();
        let app =
            atest::init_service(App::new().configure(crate::http::configure(state.clone()))).await;
        for _ in 0..2 {
            let resp = atest::call_service(&app, raw_insert("{bad").to_request()).await;
            crate::http::tests::assert_json_error(resp, StatusCode::BAD_REQUEST, "malformed_json")
                .await;
        }
        for _ in 0..2 {
            let resp =
                atest::call_service(&app, raw_insert(r#"{"a":"1","b":2}"#).to_request()).await;
            crate::http::tests::assert_json_error(
                resp,
                StatusCode::UNPROCESSABLE_ENTITY,
                "invalid_input",
            )
            .await;
        }
        assert_eq!(state.store.len(), 0);
    }

    #[actix_web::test]
    async fn insert_empty_object_is_noop() {
        let state = test_state();
        let app =
            atest::init_service(App::new().configure(crate::http::configure(state.clone()))).await;
        let resp = atest::call_service(&app, raw_insert("{}").to_request()).await;
        assert_eq!(resp.status(), StatusCode::OK);
        assert_eq!(
            atest::read_body(resp).await,
            web::Bytes::from_static(br#""{}""#)
        );
        assert_eq!(state.store.len(), 0);
    }

    #[actix_web::test]
    async fn insert_duplicate_key_last_value_wins() {
        let state = test_state();
        let app =
            atest::init_service(App::new().configure(crate::http::configure(state.clone()))).await;
        let body =
            atest::call_and_read_body(&app, raw_insert(r#"{"a":"1","a":"2"}"#).to_request()).await;
        assert_eq!(body, web::Bytes::from_static(br#""{\"a\":\"2\"}""#));
        assert_eq!(state.store.get("a"), Ok("2".to_owned()));
        assert_eq!(state.store.len(), 1);
    }

    #[actix_web::test]
    async fn insert_overwrites_existing_key() {
        let state = test_state();
        state.store.insert("a".into(), "old".into());
        let app =
            atest::init_service(App::new().configure(crate::http::configure(state.clone()))).await;
        let resp = atest::call_service(&app, raw_insert(r#"{"a":"new"}"#).to_request()).await;
        assert_eq!(resp.status(), StatusCode::OK);
        let got =
            atest::call_and_read_body(&app, atest::TestRequest::get().uri("/key/a").to_request())
                .await;
        assert_eq!(got, web::Bytes::from_static(br#""new""#));
        assert_eq!(state.store.len(), 1);
    }

    #[actix_web::test]
    async fn insert_non_ascii_round_trips() {
        let state = test_state();
        let app =
            atest::init_service(App::new().configure(crate::http::configure(state.clone()))).await;
        let echo = atest::call_and_read_body(&app, raw_insert(r#"{"u":"ü €"}"#).to_request()).await;
        assert_eq!(echo, r#""{\"u\":\"ü €\"}""#.as_bytes());
        let got =
            atest::call_and_read_body(&app, atest::TestRequest::get().uri("/key/u").to_request())
                .await;
        assert_eq!(got, r#""ü €""#.as_bytes());
        assert_eq!(state.store.get("u"), Ok("ü €".to_owned()));
    }

    #[actix_web::test]
    async fn insert_over_256_kib_is_413() {
        let state = test_state();
        let app =
            atest::init_service(App::new().configure(crate::http::configure(state.clone()))).await;
        let resp = atest::call_service(&app, raw_insert(vec![b'a'; 300 * 1024]).to_request()).await;
        assert_eq!(resp.status(), StatusCode::PAYLOAD_TOO_LARGE);
        assert!(state.store.is_empty());
    }

    async fn list_body(state: &crate::state::AppState) -> web::Bytes {
        let app =
            atest::init_service(App::new().configure(crate::http::configure(state.clone()))).await;
        let resp =
            atest::call_service(&app, atest::TestRequest::get().uri("/list").to_request()).await;
        assert_eq!(resp.status(), StatusCode::OK);
        atest::read_body(resp).await
    }

    #[actix_web::test]
    async fn list_escapes_quotes_and_backslashes() {
        let state = test_state();
        let app =
            atest::init_service(App::new().configure(crate::http::configure(state.clone()))).await;
        let resp = atest::call_service(&app, raw_insert(r#"{"q":"a\"b\\c"}"#).to_request()).await;
        assert_eq!(resp.status(), StatusCode::OK);
        let body =
            atest::call_and_read_body(&app, atest::TestRequest::get().uri("/list").to_request())
                .await;
        let inner: String = serde_json::from_slice(&body).expect("JSON string body");
        let parsed: Vec<serde_json::Map<String, serde_json::Value>> =
            serde_json::from_str(&inner).expect("inner text is valid JSON");
        assert_eq!(parsed.len(), 1);
        assert_eq!(parsed[0].len(), 1);
        assert_eq!(parsed[0]["q"], r#"a"b\c"#);
    }

    #[actix_web::test]
    async fn list_golden_sorted() {
        let state = test_state();
        state.store.insert("b".into(), "2".into());
        state.store.insert("a".into(), "1".into());
        state.store.insert("B".into(), "3".into());
        assert_eq!(
            list_body(&state).await,
            web::Bytes::from_static(br#""[{\"B\" : \"3\"},{\"a\" : \"1\"},{\"b\" : \"2\"}]""#)
        );
    }

    #[actix_web::test]
    async fn list_empty_is_brackets_json() {
        let state = test_state();
        assert_eq!(list_body(&state).await, web::Bytes::from_static(br#""[]""#));
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
