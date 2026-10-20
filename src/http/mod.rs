//! HTTP layer: the route table and its handlers.
use actix_web::{HttpResponse, Responder, get, patch, web};

use crate::config::Settings;

pub mod kv;

/// Register every rs3 route on an actix `App`.
pub fn configure(cfg: &mut web::ServiceConfig) {
    cfg.service(index)
        .service(status)
        .service(patch_key)
        .configure(kv::configure);
}

/// `GET /` answers with the JSON welcome string.
#[get("/")]
pub async fn index() -> impl Responder {
    HttpResponse::Ok().json("Welcome to rs3")
}

/// `GET /ip` reports the configured host as the legacy JSON string `"{'IP' : '<host>'}"`.
///
/// The settings are injected by `main` (and by tests) as `web::Data<Settings>`;
/// the handler never re-reads configuration.
#[get("/ip")]
pub async fn status(settings: web::Data<Settings>) -> impl Responder {
    HttpResponse::Ok().json(format!("{{'IP' : '{}'}}", settings.server.host))
}

/// `PATCH /key/{key}` answers 501 Not Implemented until the JavaScript runtime returns in Phase 3.
#[patch("/key/{key}")]
pub async fn patch_key(_path: web::Path<String>) -> impl Responder {
    HttpResponse::NotImplemented().json(serde_json::json!({
        "error": "PATCH /key/{key} is not implemented yet; JavaScript execution returns in Phase 3"
    }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use actix_web::{App, http::StatusCode, test as atest};

    fn test_settings() -> web::Data<Settings> {
        let map: ::config::Map<String, String> =
            [("SERVER.HOST", "test-host"), ("SERVER.PORT", "8080")]
                .into_iter()
                .map(|(k, v)| (k.to_owned(), v.to_owned()))
                .collect();
        web::Data::new(Settings::from_map(map).expect("valid test settings"))
    }

    fn ip_request() -> atest::TestRequest {
        atest::TestRequest::get()
            .uri("/ip")
            .insert_header(("content-type", "text/plain"))
    }

    #[actix_web::test]
    async fn test_index_is_ok() {
        let app = atest::init_service(App::new().service(index)).await;
        let req = atest::TestRequest::default()
            .insert_header(("content-type", "text/plain"))
            .to_request();
        let resp = atest::call_service(&app, req).await;
        assert_eq!(resp.status(), StatusCode::OK);
    }

    #[actix_web::test]
    async fn test_index_body_is_welcome() {
        let app = atest::init_service(App::new().service(index)).await;
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
        let app = atest::init_service(App::new().app_data(test_settings()).service(status)).await;
        let resp = atest::call_service(&app, ip_request().to_request()).await;
        assert_eq!(resp.status(), StatusCode::OK);
    }

    #[actix_web::test]
    async fn test_ip_body_is_ip() {
        let app = atest::init_service(App::new().app_data(test_settings()).service(status)).await;
        let resp = atest::call_service(&app, ip_request().to_request()).await;
        assert_eq!(resp.status(), StatusCode::OK);
        assert_eq!(
            atest::read_body(resp).await,
            r#""{'IP' : 'test-host'}""#.as_bytes()
        );
    }

    #[actix_web::test]
    async fn test_ip_without_settings_is_500() {
        let app = atest::init_service(App::new().service(status)).await;
        let resp = atest::call_service(&app, ip_request().to_request()).await;
        assert_eq!(resp.status(), StatusCode::INTERNAL_SERVER_ERROR);
    }

    #[actix_web::test]
    async fn test_patch_is_501() {
        let app = atest::init_service(App::new().configure(configure)).await;
        let req = atest::TestRequest::patch()
            .uri("/key/anything")
            .to_request();
        let resp = atest::call_service(&app, req).await;
        assert_eq!(resp.status(), StatusCode::NOT_IMPLEMENTED);
        let body: serde_json::Value =
            serde_json::from_slice(&atest::read_body(resp).await).expect("JSON error body");
        assert!(body.is_object(), "body is not an object: {body}");
        assert!(body["error"].is_string(), "no string error field: {body}");
    }

    const LIST_SCRIPT: &str = "Deno.core.ops.op_rs3_list().toString();";

    #[actix_web::test]
    #[ignore = "re-enabled in Phase 3 (JS runtime port)"]
    async fn test_patch_is_ok() {
        crate::http::kv::STORE.insert("code".into(), LIST_SCRIPT.into());
        let app = atest::init_service(App::new().configure(configure)).await;
        let req = atest::TestRequest::patch().uri("/key/code").to_request();
        let resp = atest::call_service(&app, req).await;
        assert_eq!(resp.status(), StatusCode::OK);
    }

    #[actix_web::test]
    #[ignore = "re-enabled in Phase 3 (JS runtime port)"]
    async fn test_patch_body_is_listing() {
        crate::http::kv::STORE.insert("code_body".into(), LIST_SCRIPT.into());
        let app = atest::init_service(App::new().configure(configure)).await;
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
