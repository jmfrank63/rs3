use actix_web::web::{Data, Path};
use actix_web::{delete, get, post, web, HttpRequest, HttpResponse, Responder};
use flurry::HashMap;

use crate::bindings;
use crate::config::Config;

pub fn app_service_config(config: &mut web::ServiceConfig) {
    config
        .service(index)
        .service(status)
        .service(insert)
        .service(delete)
        .service(get)
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
pub async fn insert(request_body: String, map: Data<HashMap<String, String>>) -> impl Responder {
    let entry = bindings::insert(request_body, map);
    HttpResponse::Ok().json(entry.to_string())
}

#[get("/list")]
pub async fn list(map: Data<HashMap<String, String>>) -> impl Responder {
    let out = bindings::list(map);
    HttpResponse::Ok().json(out.replace(",]", "]"))
}

#[delete("/key/{key}")]
pub async fn delete(path: Path<String>, map: Data<HashMap<String, String>>) -> impl Responder {
    let entry = bindings::delete(path, map);
    HttpResponse::Ok().json(entry)
}

#[get("/key/{key}")]
pub async fn get(path: Path<String>, map: Data<HashMap<String, String>>) -> impl Responder {
    let entry = bindings::get(path, map);
    HttpResponse::Ok().json(entry)
}
