mod config;

use crate::config::Config;

use actix_web::{web, App, HttpRequest, HttpResponse, HttpServer};
use std::io;

#[actix_web::main]
pub async fn main() -> io::Result<()> {
    let rs3_conf = Config::from_env().unwrap();

    println!(
        "Starting Http server at host address: {}, with port: {}!",
        rs3_conf.rs3_server.host, rs3_conf.rs3_server.port
    );

    HttpServer::new(move || App::new().configure(app_config))
        .bind(format!(
            "{}:{}",
            rs3_conf.rs3_server.host, rs3_conf.rs3_server.port
        ))?
        .run()
        .await
}

fn app_config(config: &mut web::ServiceConfig) {
    config.route("/", web::get().to(index));
}

async fn index(_req: HttpRequest) -> HttpResponse {
    HttpResponse::Ok().json("{'Status' : 'Up'}")
}

#[cfg(test)]
mod tests {
    use super::*;
    use actix_web::http::StatusCode;
    use actix_web::test;

    #[actix_rt::test]
    async fn test_index_ok() {
        let req = test::TestRequest::default()
            .insert_header(("content-type", "text/plain"))
            .to_http_request();
        let resp = index(req).await;
        assert_eq!(resp.status(), StatusCode::OK);
    }
}
