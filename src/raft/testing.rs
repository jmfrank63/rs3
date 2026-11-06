//! A cluster of real nodes on loopback ports, for tests that drive it over HTTP.
use std::collections::{BTreeMap, BTreeSet};
use std::net::TcpListener;
use std::sync::Arc;
use std::time::Duration;

use actix_web::dev::ServerHandle;
use actix_web::{App, HttpResponse, HttpServer, post, web};
use openraft::EntryPayload;
use openraft::error::RaftError;
use openraft::raft::{AppendEntriesRequest, AppendEntriesResponse};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};

use super::{Node, TypeConfig};
use crate::goals::Goals;
use crate::state::{AppState, test_state};
use crate::storage::TempDir;
use crate::store::Command;
use crate::store::Store;

/// Tokens per second the goals of every node emit.
pub const RATE: std::num::NonZeroU32 = std::num::NonZeroU32::new(200).unwrap();

pub const WAIT: Option<Duration> = Some(Duration::from_secs(15));

/// While set, a node takes the log entries that carry a command but answers the leader that only
/// what came before them matched. The leader keeps hearing its followers, so it keeps its quorum, yet never commits:
/// the window between the quorum check and commit.
#[derive(Clone, Default)]
pub struct Cut(pub Arc<AtomicBool>);

/// While `armed`, the next forwarded append is committed by the leader but answered with an
/// error, and from then on `lag` holds back what followers are told is committed, so they apply
/// nothing new until the test clears it.
#[derive(Clone, Default)]
pub struct Lose {
    pub armed: Arc<AtomicBool>,
    pub lag: Arc<AtomicBool>,
}

/// Node `node` (when non-zero) answers every append only after `millis` ms.
#[derive(Clone, Default)]
pub struct Slow {
    pub node: Arc<AtomicU64>,
    pub millis: Arc<AtomicU64>,
}

#[post("/raft/forward")]
async fn forward(
    node: web::Data<Node>,
    lose: web::Data<Lose>,
    cmd: web::Json<Command>,
) -> HttpResponse {
    let cmd = cmd.into_inner();
    let drop_answer =
        matches!(cmd, Command::Append { .. }) && lose.armed.swap(false, Ordering::SeqCst);
    if drop_answer {
        lose.lag.store(true, Ordering::SeqCst);
    }
    let written = node.write(cmd).await;
    if drop_answer {
        return HttpResponse::InternalServerError().finish();
    }
    crate::http::raft::answer(written)
}

#[post("/raft/append")]
async fn append(
    node: web::Data<Node>,
    cut: web::Data<Cut>,
    lose: web::Data<Lose>,
    slow: web::Data<Slow>,
    req: web::Json<AppendEntriesRequest<TypeConfig>>,
) -> HttpResponse {
    let mut req = req.into_inner();
    if lose.lag.load(Ordering::SeqCst) {
        req.leader_commit = None;
    }
    let carries_command = req
        .entries
        .iter()
        .any(|e| matches!(e.payload, EntryPayload::Normal(_)));
    let prev = req.prev_log_id;
    let answer = node.raft().append_entries(req).await;
    if slow.node.load(Ordering::SeqCst) == node.raft().metrics().borrow().id {
        let millis = slow.millis.load(Ordering::SeqCst);
        tokio::time::sleep(Duration::from_millis(millis)).await;
    }
    if carries_command && cut.0.load(Ordering::SeqCst) {
        let behind: Result<_, RaftError<u64>> =
            Ok(AppendEntriesResponse::<u64>::PartialSuccess(prev));
        return HttpResponse::Ok().json(behind);
    }
    HttpResponse::Ok().json(answer)
}

/// `members` nodes are configured; only the first `running` are started, the rest refuse connections.
pub struct Cluster {
    pub addrs: Vec<String>,
    pub cut: Cut,
    pub lose: Lose,
    pub slow: Slow,
    pub nodes: Vec<Arc<Node>>,
    pub stores: Vec<Store>,
    /// The goals of each running node; idle until `work`.
    pub goals: Vec<Goals>,
    config: BTreeMap<u64, String>,
    pub dbs: Vec<Arc<redb::Database>>,
    servers: Vec<ServerHandle>,
    down: BTreeSet<usize>,
    _dir: TempDir,
}

impl Cluster {
    pub async fn start(members: u64, running: u64) -> Self {
        let dir = TempDir::new();
        let listeners: Vec<TcpListener> = (0..members)
            .map(|_| TcpListener::bind("127.0.0.1:0").unwrap())
            .collect();
        let addrs: Vec<String> = listeners
            .iter()
            .map(|l| l.local_addr().unwrap().to_string())
            .collect();
        let config: BTreeMap<u64, String> = (1..).zip(addrs.iter().cloned()).collect();
        let mut cluster = Self {
            addrs,
            cut: Cut::default(),
            lose: Lose::default(),
            slow: Slow::default(),
            nodes: Vec::new(),
            stores: Vec::new(),
            goals: Vec::new(),
            config,
            dbs: Vec::new(),
            servers: Vec::new(),
            down: BTreeSet::new(),
            _dir: dir,
        };
        for (listener, id) in listeners.into_iter().zip(1..=running) {
            let db = crate::storage::open(&cluster._dir.path().join(format!("db{id}"))).unwrap();
            let store = Store::durable(db.clone()).unwrap();
            let (node, server, goals) = cluster
                .launch(id, listener, db.clone(), store.clone())
                .await;
            cluster.goals.push(goals);
            cluster.dbs.push(db);
            cluster.nodes.push(node);
            cluster.stores.push(store);
            cluster.servers.push(server);
        }
        cluster
    }

    async fn launch(
        &self,
        id: u64,
        listener: TcpListener,
        db: Arc<redb::Database>,
        store: Store,
    ) -> (Arc<Node>, ServerHandle, Goals) {
        let node = Arc::new(
            Node::start(id, &self.config, db.clone(), store.clone())
                .await
                .unwrap(),
        );
        let goals = Goals::open_in_cluster(db, RATE, id, &store).unwrap();
        goals.enable_scripts(store.clone(), crate::js::test_executor());
        goals.connect(node.clone());
        let state = AppState {
            store,
            goals: Some(goals.clone()),
            cluster: Some(node.clone()),
            ..test_state()
        };
        let (app, raft) = (
            crate::http::configure(state),
            crate::http::raft::configure(web::Data::from(node.clone())),
        );
        let (cut, lose, slow, data) = (
            self.cut.clone(),
            self.lose.clone(),
            self.slow.clone(),
            web::Data::from(node.clone()),
        );
        let server = HttpServer::new(move || {
            // Registered first, so it answers /raft/append in place of the real route.
            App::new()
                .app_data(web::Data::new(cut.clone()))
                .app_data(web::Data::new(lose.clone()))
                .app_data(web::Data::new(slow.clone()))
                .app_data(data.clone())
                .service(append)
                .service(forward)
                .configure(app.clone())
                .configure(raft.clone())
        })
        .workers(1)
        .listen(listener)
        .unwrap()
        .run();
        let handle = server.handle();
        actix_web::rt::spawn(server);
        (node, handle, goals)
    }

    /// Give every node one worker, so assigned goals run.
    pub fn work(&self) {
        for goals in &self.goals {
            goals.start_workers(std::num::NonZeroUsize::MIN);
        }
    }

    /// Run the assignment loop on every node, so created goals get an owner.
    pub fn assign(&self) {
        let members: Vec<u64> = (1..=self.addrs.len() as u64).collect();
        for ((node, goals), id) in self.nodes.iter().zip(&self.goals).zip(1..) {
            super::assign::spawn(node.clone(), goals.clone(), id, members.clone());
        }
    }

    /// Stop node `i`: its HTTP server and its Raft. Its data stays on disk for `start_node`.
    pub async fn stop_node(&mut self, i: usize) {
        self.down.insert(i);
        self.servers[i].stop(false).await;
        self.nodes[i].raft().shutdown().await.unwrap();
    }

    /// Start node `i` again, on the same address and over the same data as before `stop_node`.
    pub async fn start_node(&mut self, i: usize) {
        let listener = TcpListener::bind(&self.addrs[i]).unwrap();
        let (db, store) = (self.dbs[i].clone(), self.stores[i].clone());
        let (node, server, goals) = self.launch(i as u64 + 1, listener, db, store).await;
        self.down.remove(&i);
        self.goals[i] = goals;
        self.nodes[i] = node;
        self.servers[i] = server;
    }

    /// Wait until the nodes that are up agree on one leader, and return its index.
    pub async fn leader(&self) -> usize {
        let mut leaders = Vec::new();
        for (i, node) in self
            .nodes
            .iter()
            .enumerate()
            .filter(|(i, _)| !self.down.contains(i))
        {
            let metrics = node
                .raft()
                .wait(WAIT)
                .metrics(|m| m.current_leader.is_some(), "a leader")
                .await
                .unwrap();
            leaders.push((i, metrics.current_leader));
        }
        let leader = leaders[0].1;
        for (i, _) in leaders {
            self.nodes[i]
                .raft()
                .wait(WAIT)
                .metrics(|m| m.current_leader == leader, "the same leader")
                .await
                .unwrap();
        }
        leader.unwrap() as usize - 1
    }

    /// Wait for an agreed leader, and return the index of a node that is not it.
    pub async fn follower(&self) -> usize {
        usize::from(self.leader().await == 0)
    }

    pub async fn stop(self) {
        for server in self.servers {
            server.stop(false).await;
        }
    }
}
