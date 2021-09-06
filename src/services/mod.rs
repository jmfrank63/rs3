use crate::config::Config;
use flurry::HashMap;

use actix_web::{get, post, web, HttpRequest, HttpResponse, Responder};

pub fn app_service_config(config: &mut web::ServiceConfig) {
    config.service(index).service(status).service(post);
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

#[post("/post")]
pub async fn post(request_body: String, map: web::Data<HashMap<String, String>>) -> impl Responder {
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
