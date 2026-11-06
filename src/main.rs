use std::process::ExitCode;

use actix_web::{App, HttpServer};
use rs3::config::Settings;
use rs3::goals::Goals;
use rs3::store::Store;

/// The store and goals the settings ask for: durable with `STORAGE.PATH`, else
/// an in-memory store and no goals. Both share one database handle.
fn open_storage(settings: &Settings) -> Result<(Store, Option<Goals>), String> {
    let Some(path) = &settings.storage.path else {
        eprintln!("warning: STORAGE.PATH is not set; data is kept in memory only");
        return Ok((Store::new(), None));
    };
    let describe = |e: &dyn std::fmt::Display| format!("{}: {e}", path.display());
    let db = rs3::storage::open(path).map_err(|e| describe(&e))?;
    let store = Store::durable(db.clone()).map_err(|e| describe(&e))?;
    let goals = Goals::open(db, settings.model.tokens_per_second).map_err(|e| describe(&e))?;
    Ok((store, Some(goals)))
}

/// Start everything and serve until shutdown. The error is the startup or
/// server failure to report, without the `error: ` prefix.
async fn run() -> Result<(), String> {
    let settings = Settings::load().map_err(|e| format!("invalid configuration: {e}"))?;
    let (store, goals) =
        open_storage(&settings).map_err(|e| format!("cannot open storage: {e}"))?;
    // V8 is initialised once on the main thread before any other thread (JS pool, actix workers) spawns.
    rs3::js::init_platform();
    let js = rs3::js::executor::JsExecutor::start(&rs3::js::JsSettings::default())
        .map_err(|e| format!("cannot start JS runtime: {e}"))?;
    if let Some(goals) = &goals {
        goals.enable_scripts(store.clone(), js.clone());
        goals.start_workers(settings.scheduler.workers);
    }
    let host = settings.server.host.clone();
    let port = settings.server.port;
    // One AppState for the whole process; each worker only clones the closure.
    let cfg = rs3::http::configure(rs3::state::AppState {
        goals,
        ..rs3::state::AppState::new(store, settings, js)
    });
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
