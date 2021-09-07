mod config;
mod services;

use crate::config::Config;
use crate::services::app_service_config;
use flurry::HashMap;

use actix_web::web::Data;
use actix_web::{App, HttpServer};
use std::io;

#[actix_web::main]
pub async fn main() -> io::Result<()> {
    let rs3_conf = Config::from_env().unwrap();

    println!(
        "Starting Http server at host address: {}, with port: {}!",
        rs3_conf.server.host, rs3_conf.server.port
    );
    let map: Data<HashMap<String, String>> = Data::new(HashMap::new());

    HttpServer::new(move || {
        App::new()
            .app_data(map.clone())
            .configure(app_service_config)
    })
    .bind(format!("{}:{}", rs3_conf.server.host, rs3_conf.server.port))?
    .run()
    .await
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::services::{index, status};
    use actix_web::http::StatusCode;
    use actix_web::{body::Body, test, App};

    #[actix_rt::test]
    async fn test_index_ok() {
        let mut app = test::init_service(App::new().service(index)).await;
        let req = test::TestRequest::default()
            .insert_header(("content-type", "text/plain"))
            .to_request();
        let resp = test::call_service(&mut app, req).await;
        assert_eq!(resp.status(), StatusCode::OK);
    }

    #[actix_rt::test]
    async fn test_index_body_is_welcome() {
        let mut app = test::init_service(App::new().service(index)).await;
        let req = test::TestRequest::default()
            .insert_header(("content-type", "text/plain"))
            .to_request();
        let resp = test::call_service(&mut app, req).await;
        let body = resp.into_body();
        assert_eq!(body, Body::from("\"Welcome to rs3\""));
    }

    #[actix_rt::test]
    async fn test_status_is_ok() {
        let mut app = test::init_service(App::new().service(status)).await;
        let req = test::TestRequest::default()
            .insert_header(("content-type", "text/plain"))
            .uri("/ip")
            .to_request();
        let resp = test::call_service(&mut app, req).await;
        assert_eq!(resp.status(), StatusCode::OK);
    }

    #[actix_rt::test]
    async fn test_status_body_is_ip() {
        let mut app = test::init_service(App::new().service(status)).await;
        let req = test::TestRequest::default()
            .insert_header(("content-type", "text/plain"))
            .uri("/ip")
            .to_request();
        let resp = test::call_service(&mut app, req).await;
        let body = resp.into_body();
        let rs3_conf = Config::from_env().unwrap();
        assert_eq!(
            body,
            Body::from("\"{'IP' : '".to_string() + rs3_conf.server.host.as_str() + "'}\"")
        );
    }
}
