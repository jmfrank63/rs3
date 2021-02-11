mod config;

use crate::config::RS3Config;

use actix_web::{web, App, HttpRequest, HttpResponse, HttpServer, Responder};
use std::io;

#[actix_rt::main]
pub async fn main() -> io::Result<()> {
    let rs3_conf = RS3Config::from_env().unwrap();

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
    config.route("/", web::get().to(index));
}

async fn index(_req: HttpRequest) -> impl Responder {
    HttpResponse::Ok().json("{'Status' : 'Up'}")
}

#[cfg(test)]
mod tests {
    use super::*;
    use actix_web::{http, test};
    use std::io::Read;

    #[actix_rt::test]
    async fn test_index_ok() {
        let req = test::TestRequest::with_header("content-type", "text/plain").to_http_request();
        let resp = index(req).await;
        assert!(resp.status().is_ok());
    }

    #[actix_rt::test]
    async fn test_index_not_ok() {
        let req = test::TestRequest::default().to_http_request();
        let resp = index(req).await;
        assert!(resp.status().is_err());
    }
}
