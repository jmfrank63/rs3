//! HTTP layer: the route table and its handlers.
use actix_web::{HttpResponse, Responder, get, web};

/// Register every rs3 route on an actix `App`.
pub fn configure(cfg: &mut web::ServiceConfig) {
    cfg.service(index);
}

/// `GET /` answers with the JSON welcome string.
#[get("/")]
pub async fn index() -> impl Responder {
    HttpResponse::Ok().json("Welcome to rs3")
}

#[cfg(test)]
mod tests {
    use super::*;
    use actix_web::{App, http::StatusCode, test as atest};

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
}
