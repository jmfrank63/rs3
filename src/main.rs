use std::process::ExitCode;

use actix_web::{App, HttpServer};
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
    rs3::js::init_platform();
    let host = settings.server.host.clone();
    let port = settings.server.port;
    // One AppState for the whole process; each worker only clones the closure.
    let state = rs3::state::AppState::new(settings);
    let cfg = rs3::http::configure(state);
    let server = match HttpServer::new(move || App::new().configure(cfg.clone()))
        .bind((host.as_str(), port))
    {
        Ok(s) => s,
        Err(e) => {
            eprintln!("error: cannot bind {host}:{port}: {e}");
            return ExitCode::FAILURE;
        }
    };
    println!("Starting Http server at host address: {host}, with port: {port}!");
    match server.run().await {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("error: server failed: {e}");
            ExitCode::FAILURE
        }
    }
}
