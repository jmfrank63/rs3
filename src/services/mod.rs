use crate::config::Config;

use actix_web::{get, web, HttpRequest, HttpResponse, Responder};

pub fn app_service_config(config: &mut web::ServiceConfig) {
    config.service(index);
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

