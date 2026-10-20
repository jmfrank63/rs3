use std::process::ExitCode;

use actix_web::{App, HttpServer, web};
use rs3::config::Settings;

#[actix_web::main]
async fn main() -> ExitCode {
    let settings = match Settings::load() {
        Ok(s) => s,
        Err(e) => {
            eprintln!("error: invalid configuration: {e}");
            return ExitCode::FAILURE;
        }
    };
    // Main thread, before actix spawns its workers.
    rs3::js::init_platform();
    let host = settings.server.host.clone();
    let port = settings.server.port;
    println!("Starting Http server at host address: {host}, with port: {port}!");
    // Created outside the factory closure so every worker shares one instance.
    let settings = web::Data::new(settings);
    let server = match HttpServer::new(move || {
        App::new()
            .app_data(settings.clone())
            .configure(rs3::http::configure)
    })
    .bind(&(host.clone(), port))
    {
        Ok(s) => s,
        Err(e) => {
            eprintln!("error: cannot bind {host}:{port}: {e}");
            return ExitCode::FAILURE;
        }
    };
    match server.run().await {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("error: server failed: {e}");
            ExitCode::FAILURE
        }
    }
}
