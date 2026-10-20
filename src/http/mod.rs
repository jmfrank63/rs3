//! HTTP layer: the route table and its handlers.
use actix_web::{HttpResponse, Responder, get, web};

use crate::config::Settings;

pub mod kv;

/// Register every rs3 route on an actix `App`.
pub fn configure(cfg: &mut web::ServiceConfig) {
    cfg.service(index).service(status).configure(kv::configure);
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
}
