mod bindings;
mod config;
mod services;

use crate::config::Config;
use crate::services::app_service_config;
use flurry::HashMap;
use lazy_static::lazy_static;

use actix_web::{App, HttpServer};
use std::io;

lazy_static! {
    pub static ref MAP: HashMap<String, String> = HashMap::new();
}

#[actix_web::main]
pub async fn main() -> io::Result<()> {
    let rs3_conf = Config::from_env().unwrap();

    println!(
        "Starting Http server at host address: {}, with port: {}!",
        rs3_conf.server.host, rs3_conf.server.port
    );

    let app = HttpServer::new(move || {
        let app = App::new().configure(app_service_config);
        app
    })
    .bind(format!("{}:{}", rs3_conf.server.host, rs3_conf.server.port))?
    .run();
    app.await
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::services::{delete, get, index, insert, list, patch, status};
    use actix_web::http::StatusCode;
    use actix_web::{body::Body, test, App};
    use std::{thread, time};

    #[actix_rt::test]
    async fn test_index_is_ok() {
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
        assert_eq!(body, Body::from(r#""Welcome to rs3""#));
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
    async fn test_ip_body_is_ip() {
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

    #[actix_rt::test]
    async fn test_insert_is_ok() {
        let mut app = test::init_service(App::new().service(insert)).await;
        let payload = r#"{"insert_test":"John Doe"}"#;
        let req = test::TestRequest::post()
            .insert_header(("Content-Type", "application/json"))
            .set_payload(payload)
            .uri("/insert")
            .to_request();
        let resp = test::call_service(&mut app, req).await;
        assert_eq!(resp.status(), StatusCode::OK);
    }

    #[actix_rt::test]
    async fn test_insert_map_entry_is_ok() {
        let mut app = test::init_service(App::new().service(insert)).await;
        let payload = r#"{"insert_test":"John Doe"}"#;
        let req = test::TestRequest::post()
            .insert_header(("Content-Type", "application/json"))
            .set_payload(payload)
            .uri("/insert")
            .to_request();
        let resp = test::call_service(&mut app, req).await;
        let body = resp.into_body();
        assert_eq!(body, Body::from(r#""{\"insert_test\":\"John Doe\"}""#));
    }

    #[actix_rt::test]
    async fn test_list_is_ok() {
        let mut app = test::init_service(App::new().service(list)).await;
        let req = test::TestRequest::get().uri("/list").to_request();
        let resp = test::call_service(&mut app, req).await;
        assert_eq!(resp.status(), StatusCode::OK);
    }

    #[actix_rt::test]
    async fn test_delete_is_ok() {
        let guard = MAP.guard();
        MAP.insert("delete_test".to_string(), "John Doe".to_string(), &guard);
        let mut app = test::init_service(App::new().service(delete)).await;
        let req = test::TestRequest::delete()
            .uri("/key/delete_test")
            .insert_header(("Content-Type", "application/json"))
            .to_request();
        let resp = test::call_service(&mut app, req).await;
        assert!(!MAP.contains_key("delete_test", &guard));
        assert_eq!(resp.status(), StatusCode::OK);
    }

    #[actix_rt::test]
    async fn test_get_is_ok() {
        let guard = MAP.guard();
        MAP.insert("get_test".to_string(), "John Doe".to_string(), &guard);
        let mut app = test::init_service(App::new().service(get)).await;
        let req = test::TestRequest::get()
            .uri("/key/get_test")
            .insert_header(("Content-Type", "application/json"))
            .to_request();
        let resp = test::call_service(&mut app, req).await;
        assert_eq!(resp.status(), StatusCode::OK);
    }

    #[actix_rt::test]
    async fn test_patch_is_ok() {
        let guard = MAP.guard();
        MAP.insert(
            "code".to_string(),
            r#"Deno.core.ops();Deno.core.opSync("rs3_list", "").toString();"#.to_string(),
            &guard,
        );
        let mut app = test::init_service(App::new().service(patch)).await;
        let req = test::TestRequest::patch()
            .uri("/key/code")
            .insert_header(("Content-Type", "application/json"))
            .to_request();
        let resp = test::call_service(&mut app, req).await;
        assert_eq!(resp.status(), StatusCode::OK);
    }

    #[actix_rt::test]
    async fn test_patch_body_is_listing() {
        let guard = MAP.guard();
        thread::sleep(time::Duration::from_millis(1000));
        MAP.clear(&guard);
        thread::sleep(time::Duration::from_millis(1000));
        MAP.insert(
            "code_body".to_string(),
            r#"Deno.core.ops();Deno.core.opSync("rs3_list", "").toString();"#.to_string(),
            &guard,
        );
        let mut app = test::init_service(App::new().service(patch).service(list)).await;
        let patch_req = test::TestRequest::patch()
            .uri("/key/code_body")
            .insert_header(("Content-Type", "application/json"))
            .to_request();
        let patch_resp = test::call_service(&mut app, patch_req).await;
        let patch_body = patch_resp.into_body();

        let list_req = test::TestRequest::get()
            .uri("/list")
            .insert_header(("Content-Type", "application/json"))
            .to_request();
        let list_resp = test::call_service(&mut app, list_req).await;
        let list_body = list_resp.into_body();
        assert_eq!(patch_body, list_body);
    }
}
