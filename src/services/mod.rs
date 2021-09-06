use crate::config::Config;
use flurry::HashMap;

use actix_web::web::Data;
use actix_web::{delete, get, post, web, HttpRequest, HttpResponse, Responder};

pub fn app_service_config(config: &mut web::ServiceConfig) {
    config
        .service(index)
        .service(status)
        .service(insert)
        .service(delete)
        .service(list);
}

#[get("/")]
pub async fn index(_req: HttpRequest) -> impl Responder {
    let rs3_conf = Config::from_env().unwrap();
    HttpResponse::Ok().json("{'IP' : '".to_string() + rs3_conf.server.host.as_str() + "'}")
}

#[get("/ip")]
pub async fn status(_req: HttpRequest) -> impl Responder {
    let rs3_conf = Config::from_env().unwrap();
    HttpResponse::Ok().json("{'IP' : '".to_string() + rs3_conf.server.host.as_str() + "'}")
}

#[post("/insert")]
pub async fn insert(request_body: String, map: Data<HashMap<String, String>>) -> impl Responder {
    let guard = map.guard();
    let entry: serde_json::Value = serde_json::from_str(request_body.as_str()).unwrap();
    let obj = entry.as_object().unwrap();
    for (k, v) in obj.iter() {
        map.insert(k.to_string(), v.to_string(), &guard);
    }
    for (k, v) in map.iter(&guard) {
        println!("{} {}", k, v);
    }
    HttpResponse::Ok().json(entry.to_string())
}

#[get("/list")]
pub async fn list(map: Data<HashMap<String, String>>) -> impl Responder {
    let guard = map.guard();
    let mut out = String::new();
    out.push_str("[");
    for (k, v) in map.iter(&guard) {
        out.push_str(format!("{{\"{}\" : {}}}", k, v).as_str());
        out.push_str(",");
    }
    out.push_str("]");
    HttpResponse::Ok().json(out.replace(",]", "]"))
}

#[delete("/delete")]
pub async fn delete(request_body: String, map: Data<HashMap<String, String>>) -> impl Responder {
    let guard = map.guard();
    let name = request_body.as_str();
    let entry = map.remove(name, &guard).unwrap();
    HttpResponse::Ok().json(entry)
}
