use std::process::ExitCode;

use actix_web::{App, HttpServer};
use rs3::config::Settings;
use rs3::store::Store;

/// The store the settings ask for: durable with `STORAGE.PATH`, else in memory.
fn open_store(settings: &Settings) -> Result<Store, String> {
    let Some(path) = &settings.storage.path else {
        eprintln!("warning: STORAGE.PATH is not set; data is kept in memory only");
        return Ok(Store::new());
    };
    let describe = |e: &dyn std::fmt::Display| format!("{}: {e}", path.display());
    let db = rs3::storage::open(path).map_err(|e| describe(&e))?;
    Store::durable(db).map_err(|e| describe(&e))
}

#[actix_web::main]
async fn main() -> ExitCode {
    let settings = match Settings::load() {
        Ok(s) => s,
        Err(e) => {
            eprintln!("error: invalid configuration: {e}");
            return ExitCode::FAILURE;
        }
    };
    let store = match open_store(&settings) {
        Ok(store) => store,
        Err(e) => {
            eprintln!("error: cannot open storage: {e}");
            return ExitCode::FAILURE;
        }
    };
    // V8 is initialised once on the main thread before any other thread (JS pool, actix workers) spawns.
    rs3::js::init_platform();
    let js = match rs3::js::executor::JsExecutor::start(&rs3::js::JsSettings::default()) {
        Ok(js) => js,
        Err(e) => {
            eprintln!("error: cannot start JS runtime: {e}");
            return ExitCode::FAILURE;
        }
    };
    let host = settings.server.host.clone();
    let port = settings.server.port;
    // One AppState for the whole process; each worker only clones the closure.
    let state = rs3::state::AppState::new(store, settings, js);
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
