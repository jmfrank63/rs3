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

/// Start everything and serve until shutdown. The error is the startup or
/// server failure to report, without the `error: ` prefix.
async fn run() -> Result<(), String> {
    let settings = Settings::load().map_err(|e| format!("invalid configuration: {e}"))?;
    let store = open_store(&settings).map_err(|e| format!("cannot open storage: {e}"))?;
    // V8 is initialised once on the main thread before any other thread (JS pool, actix workers) spawns.
    rs3::js::init_platform();
    let js = rs3::js::executor::JsExecutor::start(&rs3::js::JsSettings::default())
        .map_err(|e| format!("cannot start JS runtime: {e}"))?;
    let host = settings.server.host.clone();
    let port = settings.server.port;
    // One AppState for the whole process; each worker only clones the closure.
    let cfg = rs3::http::configure(rs3::state::AppState::new(store, settings, js));
    let server = HttpServer::new(move || App::new().configure(cfg.clone()))
        .bind((host.as_str(), port))
        .map_err(|e| format!("cannot bind {host}:{port}: {e}"))?;
    println!("Starting Http server at host address: {host}, with port: {port}!");
    server
        .run()
        .await
        .map_err(|e| format!("server failed: {e}"))
}

#[actix_web::main]
async fn main() -> ExitCode {
    match run().await {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("error: {e}");
            ExitCode::FAILURE
        }
    }
}
