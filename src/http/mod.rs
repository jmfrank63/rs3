//! HTTP layer: the route table and its handlers.
use actix_web::{HttpResponse, Responder, get, patch, web};

use crate::state::AppState;
use error::ApiError;

pub mod error;
pub mod kv;

/// Build the app configuration for one shared `AppState`.
///
/// The `web::Data` is created here exactly once; the returned closure only
/// clones it, so every actix worker (and every test app) built from clones of
/// this closure sees the same store and settings.
pub fn configure(state: AppState) -> impl Fn(&mut web::ServiceConfig) + Clone + Send + 'static {
    let data = web::Data::new(state);
    move |cfg: &mut web::ServiceConfig| {
        cfg.app_data(data.clone())
            .service(index)
            .service(status)
            .service(patch_key)
            .configure(kv::configure);
    }
}

/// `GET /` answers with the JSON welcome string.
#[get("/")]
pub async fn index() -> impl Responder {
    HttpResponse::Ok().json("Welcome to rs3")
}

/// `GET /ip` reports the configured host as the legacy JSON string `"{'IP' : '<host>'}"`.
///
/// The host comes from the settings in the injected `web::Data<AppState>`,
/// loaded once at startup; the handler never re-reads configuration.
#[get("/ip")]
pub async fn status(state: web::Data<AppState>) -> impl Responder {
    HttpResponse::Ok().json(format!("{{'IP' : '{}'}}", state.settings.server.host))
}

/// `PATCH /key/{key}` runs the stored value as JavaScript on the dedicated JS
/// pool; the actix worker only awaits the reply.
///
/// Success is 200 with the JSON string of `String(completion value)`. A
/// missing key is a 404 JSON error, answered before anything is queued.
#[patch("/key/{key}")]
pub async fn patch_key(
    state: web::Data<AppState>,
    path: web::Path<String>,
) -> Result<HttpResponse, ApiError> {
    let key = path.into_inner();
    let source = state.store.get(&key)?;
    let out = state.js.execute(state.store.clone(), key, source).await?;
    Ok(HttpResponse::Ok().json(out))
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use crate::state::test_state;
    use actix_web::{App, dev::ServiceResponse, http::StatusCode, test as atest};

    /// Assert a JSON error response: status, `application/json`, the `error`
    /// code and a string `message`. Returns the parsed body.
    pub(crate) async fn assert_json_error(
        resp: ServiceResponse,
        expected: StatusCode,
        code: &str,
    ) -> serde_json::Value {
        assert_eq!(resp.status(), expected);
        assert_eq!(
            resp.headers()
                .get("content-type")
                .map(|v| v.as_bytes().to_vec()),
            Some(b"application/json".to_vec())
        );
        let body: serde_json::Value =
            serde_json::from_slice(&atest::read_body(resp).await).expect("JSON error body");
        assert_eq!(body["error"], code, "unexpected error code: {body}");
        assert!(body["message"].is_string(), "no string message: {body}");
        body
    }

    fn ip_request() -> atest::TestRequest {
        atest::TestRequest::get()
            .uri("/ip")
            .insert_header(("content-type", "text/plain"))
    }

    #[actix_web::test]
    async fn test_index_is_ok() {
        let app = atest::init_service(App::new().configure(configure(test_state()))).await;
        let req = atest::TestRequest::default()
            .insert_header(("content-type", "text/plain"))
            .to_request();
        let resp = atest::call_service(&app, req).await;
        assert_eq!(resp.status(), StatusCode::OK);
    }

    #[actix_web::test]
    async fn test_index_body_is_welcome() {
        let app = atest::init_service(App::new().configure(configure(test_state()))).await;
        let req = atest::TestRequest::default()
            .insert_header(("content-type", "text/plain"))
            .to_request();
        let resp = atest::call_service(&app, req).await;
        assert_eq!(resp.status(), StatusCode::OK);
        assert_eq!(
            atest::read_body(resp).await,
            r#""Welcome to rs3""#.as_bytes()
        );
    }

    #[actix_web::test]
    async fn test_status_is_ok() {
        let app = atest::init_service(App::new().configure(configure(test_state()))).await;
        let resp = atest::call_service(&app, ip_request().to_request()).await;
        assert_eq!(resp.status(), StatusCode::OK);
    }

    #[actix_web::test]
    async fn test_ip_body_is_ip() {
        let app = atest::init_service(App::new().configure(configure(test_state()))).await;
        let resp = atest::call_service(&app, ip_request().to_request()).await;
        assert_eq!(resp.status(), StatusCode::OK);
        assert_eq!(
            atest::read_body(resp).await,
            r#""{'IP' : 'test-host'}""#.as_bytes()
        );
    }

    #[actix_web::test]
    async fn test_ip_without_settings_is_500() {
        // No AppState registered: actix answers its own text/plain 500.
        let app = atest::init_service(App::new().service(status)).await;
        let resp = atest::call_service(&app, ip_request().to_request()).await;
        assert_eq!(resp.status(), StatusCode::INTERNAL_SERVER_ERROR);
    }

    #[actix_web::test]
    async fn test_patch_missing_is_404_json() {
        let app = atest::init_service(App::new().configure(configure(test_state()))).await;
        let req = atest::TestRequest::patch().uri("/key/nope").to_request();
        let resp = atest::call_service(&app, req).await;
        assert_json_error(resp, StatusCode::NOT_FOUND, "not_found").await;
    }

    #[actix_web::test]
    async fn test_patch_existing_runs_script() {
        let state = test_state();
        state.store.insert("p".into(), "1".into());
        let app = atest::init_service(App::new().configure(configure(state.clone()))).await;
        let req = atest::TestRequest::patch().uri("/key/p").to_request();
        let resp = atest::call_service(&app, req).await;
        assert_eq!(resp.status(), StatusCode::OK);
        assert_eq!(
            atest::read_body(resp).await,
            web::Bytes::from_static(br#""1""#)
        );
        assert_eq!(state.store.get("p"), Ok("1".to_owned()));
    }

    #[actix_web::test]
    async fn test_patch_runs_script_200() {
        let state = test_state();
        state.store.insert("m".into(), "6*20".into());
        let app = atest::init_service(App::new().configure(configure(state))).await;
        let req = atest::TestRequest::patch().uri("/key/m").to_request();
        let resp = atest::call_service(&app, req).await;
        assert_eq!(resp.status(), StatusCode::OK);
        assert_eq!(
            atest::read_body(resp).await,
            web::Bytes::from_static(br#""120""#)
        );
    }

    #[actix_web::test]
    async fn insert_on_one_app_visible_on_another() {
        // Two apps from clones of one configure closure stand in for two actix workers.
        let state = test_state();
        let cfg = configure(state.clone());
        let a = atest::init_service(App::new().configure(cfg.clone())).await;
        let b = atest::init_service(App::new().configure(cfg)).await;
        let echo = atest::call_and_read_body(
            &a,
            atest::TestRequest::post()
                .uri("/insert")
                .set_payload(r#"{"k":"v"}"#)
                .to_request(),
        )
        .await;
        assert_eq!(echo, web::Bytes::from_static(br#""{\"k\":\"v\"}""#));
        let resp =
            atest::call_service(&b, atest::TestRequest::get().uri("/key/k").to_request()).await;
        assert_eq!(resp.status(), StatusCode::OK);
        assert_eq!(
            atest::read_body(resp).await,
            web::Bytes::from_static(br#""v""#)
        );
        assert_eq!(state.store.get("k"), Ok("v".to_owned()));
    }

    const LIST_SCRIPT: &str = "Deno.core.ops.op_rs3_list().toString();";

    #[actix_web::test]
    async fn test_patch_is_ok() {
        let state = test_state();
        state.store.insert("code".into(), LIST_SCRIPT.into());
        let app = atest::init_service(App::new().configure(configure(state.clone()))).await;
        let req = atest::TestRequest::patch().uri("/key/code").to_request();
        let resp = atest::call_service(&app, req).await;
        assert_eq!(resp.status(), StatusCode::OK);
    }

    #[actix_web::test]
    async fn test_patch_body_is_listing() {
        let state = test_state();
        state.store.insert("code_body".into(), LIST_SCRIPT.into());
        let app = atest::init_service(App::new().configure(configure(state.clone()))).await;
        let patched = atest::call_and_read_body(
            &app,
            atest::TestRequest::patch()
                .uri("/key/code_body")
                .to_request(),
        )
        .await;
        let listing =
            atest::call_and_read_body(&app, atest::TestRequest::get().uri("/list").to_request())
                .await;
        assert_eq!(patched, listing);
    }
}
