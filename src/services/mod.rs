//! Contains the service handlers
//!
//! A service is a combination of route and correct http method
//! When both match the handler is called
use actix_web::web::Path;
use actix_web::{delete, get, patch, post, web, HttpRequest, HttpResponse, Responder};

use crate::bindings;
use crate::config::Config;

pub fn app_service_config(config: &mut web::ServiceConfig) {
    config
        .service(index)
        .service(status)
        .service(insert)
        .service(delete)
        .service(get)
        .service(patch)
        .service(list);
}

#[get("/")]
pub async fn index(_req: HttpRequest) -> impl Responder {
    HttpResponse::Ok().json("Welcome to rs3")
}

#[get("/ip")]
pub async fn status(_req: HttpRequest) -> impl Responder {
    let rs3_conf = Config::from_env().unwrap();
    HttpResponse::Ok().json("{'IP' : '".to_string() + rs3_conf.server.host.as_str() + "'}")
}

#[post("/insert")]
pub async fn insert(request_body: String) -> impl Responder {
    let entry = bindings::insert(request_body);
    HttpResponse::Ok().json(entry.to_string())
}

#[get("/list")]
pub async fn list() -> impl Responder {
    let out = bindings::list();
    HttpResponse::Ok().json(out.replace(",]", "]"))
}

#[delete("/key/{key}")]
pub async fn delete(path: Path<String>) -> impl Responder {
    let value = bindings::delete(path.into_inner());
    HttpResponse::Ok().json(value)
}

#[get("/key/{key}")]
pub async fn get(path: Path<String>) -> impl Responder {
    let value = bindings::get(path.into_inner());
    HttpResponse::Ok().json(value)
}

#[patch("/key/{key}")]
pub async fn patch(path: Path<String>) -> impl Responder {
    let value = bindings::patch(path.into_inner());
    HttpResponse::Ok().json(value)
}
