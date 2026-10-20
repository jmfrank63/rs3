//! Store-backed routes: `POST /insert`, `GET /key/{key}`, `DELETE /key/{key}` and `GET /list`.
use actix_web::{HttpResponse, Responder, delete, get, post, web};

use super::error::ApiError;
use crate::state::AppState;
use crate::store::legacy;

/// Register the store routes. The shared `AppState` is registered by `http::configure`.
pub fn configure(cfg: &mut web::ServiceConfig) {
    cfg.service(insert)
        .service(get_key)
        .service(delete_key)
        .service(list);
}

/// `POST /insert` stores every key/value of a JSON object of strings and echoes it.
///
/// The body is fully parsed and validated before anything is written; a bad
/// body becomes a JSON error response instead of a panic.
#[post("/insert")]
pub async fn insert(state: web::Data<AppState>, body: String) -> Result<HttpResponse, ApiError> {
    let entries = legacy::parse_insert_body(&body)?;
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
}
