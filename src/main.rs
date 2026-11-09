use std::process::ExitCode;
use std::sync::Arc;

use actix_web::{App, HttpServer, web};
use rs3::config::Settings;
use rs3::goals::Goals;
use rs3::store::Store;

/// The store, its goals and, in a cluster, the database the Raft node shares.
type Storage = (Store, Option<Goals>, Option<Arc<redb::Database>>);

/// The store and goals the settings ask for: durable with `STORAGE.PATH`, else
/// an in-memory store and no goals. Both share one database handle. A cluster
/// builds goals owned by this node and hands back the database for the Raft node.
fn open_storage(settings: &Settings) -> Result<Storage, String> {
    let Some(path) = &settings.storage.path else {
        eprintln!("warning: STORAGE.PATH is not set; data is kept in memory only");
        return Ok((Store::new(), None, None));
    };
    let describe = |e: &dyn std::fmt::Display| format!("{}: {e}", path.display());
    let db = rs3::storage::open(path).map_err(|e| describe(&e))?;
    let store = Store::durable(db.clone()).map_err(|e| describe(&e))?;
    let rate = settings.model.tokens_per_second;
    match &settings.cluster {
        Some(cluster) => {
            let goals = Goals::open_in_cluster(db.clone(), rate, cluster.node_id, &store)
                .map_err(|e| describe(&e))?;
            Ok((store, Some(goals), Some(db)))
        }
        None => {
            let goals = Goals::open(db, rate).map_err(|e| describe(&e))?;
            Ok((store, Some(goals), None))
        }
    }
}

/// The Raft node when the settings ask for a cluster; announces membership on stderr.
async fn start_cluster(
    settings: &Settings,
    db: Option<Arc<redb::Database>>,
    store: &Store,
) -> Result<Option<Arc<rs3::raft::Node>>, String> {
    let (Some(cluster), Some(db)) = (&settings.cluster, db) else {
        return Ok(None);
    };
    eprintln!(
        "cluster node {} of {} peers",
        cluster.node_id,
        cluster.peers.len()
    );
    let members = cluster.peers.iter().cloned().collect();
    rs3::raft::Node::start(
        cluster.node_id,
        &members,
        db,
        store.clone(),
        cluster.snapshot_every,
    )
    .await
    .map(|node| Some(Arc::new(node)))
    .map_err(|e| format!("cannot start cluster node: {e}"))
}

/// Start everything and serve until shutdown. The error is the startup or
/// server failure to report, without the `error: ` prefix.
async fn run() -> Result<(), String> {
    let settings = Settings::load().map_err(|e| format!("invalid configuration: {e}"))?;
    let (store, goals, cluster_db) =
        open_storage(&settings).map_err(|e| format!("cannot open storage: {e}"))?;
    // V8 is initialised once on the main thread before any other thread (JS pool, actix workers) spawns.
    rs3::js::init_platform();
    let js = rs3::js::executor::JsExecutor::start(&rs3::js::JsSettings::default())
        .map_err(|e| format!("cannot start JS runtime: {e}"))?;
    if let Some(goals) = &goals {
        goals.enable_scripts(store.clone(), js.clone());
    }
    let cluster = start_cluster(&settings, cluster_db, &store).await?;
    let fragments = settings
        .storage
        .path
        .as_deref()
        .map(rs3::raft::seal::fragment_dir)
        .unwrap_or_default();
    if let (Some(goals), Some(node), Some(config)) = (&goals, &cluster, &settings.cluster) {
        goals.connect(node.clone(), fragments.clone());
        rs3::raft::assign::spawn(
            node.clone(),
            goals.clone(),
            config.node_id,
            Some(fragments.clone()),
            config.shrink_after,
        );
    }
    if let Some(goals) = &goals {
        goals.start_workers(settings.scheduler.workers);
    }
    let host = settings.server.host.clone();
    let port = settings.server.port;
    // One AppState for the whole process; each worker only clones the closure.
    let cfg = rs3::http::configure(rs3::state::AppState {
        goals,
        cluster: cluster.clone(),
        ..rs3::state::AppState::new(store, settings, js)
    });
    let raft = cluster.map(|node| rs3::http::raft::configure(web::Data::from(node), fragments));
    let server = HttpServer::new(move || {
        let app = App::new().configure(cfg.clone());
        match &raft {
            Some(raft) => app.configure(raft.clone()),
            None => app,
        }
    })
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
    let args: Vec<String> = std::env::args().skip(1).collect();
    let result = match args.split_first() {
        Some((command, urls)) if command == "acp" => rs3::acp::run(urls).await,
        _ => run().await,
    };
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("error: {e}");
            ExitCode::FAILURE
        }
    }
}
