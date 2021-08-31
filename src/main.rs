mod config;

use crate::config::Config;

use actix_web::{get, web, App, HttpRequest, HttpResponse, HttpServer, Responder};
use std::io;

#[actix_web::main]
pub async fn main() -> io::Result<()> {
    let rs3_conf = Config::from_env().unwrap();

    println!(
        "Starting Http server at host address: {}, with port: {}!",
        rs3_conf.server.host, rs3_conf.server.port
    );

    HttpServer::new(move || App::new().configure(app_config))
        .bind(format!("{}:{}", rs3_conf.server.host, rs3_conf.server.port))?
        .run()
        .await
}


fn app_config(config: &mut web::ServiceConfig) {
    config.service(index);
}

#[get("/")]
async fn index(_req: HttpRequest) -> impl Responder {
    let rs3_conf = Config::from_env().unwrap();
    HttpResponse::Ok().json("{'IP' : '".to_string() + rs3_conf.server.host.as_str() + "'}")
}

#[cfg(test)]
mod tests {
    use super::*;
    use actix_web::http::StatusCode;
    use actix_web::{test, App};

    #[actix_rt::test]
    async fn test_index_ok() {
        let mut app = test::init_service(App::new().service(index)).await;
        let req = test::TestRequest::default().insert_header(("content-type", "text/plain"))
            .to_request();
        let resp = test::call_service(&mut app, req).await;
        assert_eq!(resp.status(), StatusCode::OK);
    }
}
