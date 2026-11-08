//! A cluster of real nodes on loopback ports, for tests that drive it over HTTP.
use std::collections::{BTreeMap, BTreeSet};
use std::net::TcpListener;
use std::sync::Arc;
use std::time::Duration;

use actix_web::dev::ServerHandle;
use actix_web::{App, HttpResponse, HttpServer, get, post, web};
use openraft::EntryPayload;
use openraft::error::RaftError;
use openraft::raft::{AppendEntriesRequest, AppendEntriesResponse};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};

use super::{Node, TypeConfig};

/// A small snapshot interval, so tests reach a snapshot and a log purge with few writes.
pub const SNAPSHOT_EVERY: u64 = 50;
use crate::goals::Goals;
use crate::state::{AppState, test_state};
use crate::storage::TempDir;
use crate::store::Command;
use crate::store::Store;

/// Tokens per second the goals of every node emit.
pub const RATE: std::num::NonZeroU32 = std::num::NonZeroU32::new(200).unwrap();

/// Short, so a test sees a shrink without a long wait.
pub const SHRINK_AFTER: std::time::Duration = std::time::Duration::from_secs(5);

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

/// The number of next `read_index` requests a node refuses as a node that has just stopped leading
/// would, which a follower still pointing at it meets while leadership moves.
#[derive(Clone, Default)]
pub struct Refuse(pub Arc<AtomicU64>);

#[post("/raft/read_index")]
async fn read_index(node: web::Data<Node>, refuse: web::Data<Refuse>) -> HttpResponse {
    let refused = refuse
        .0
        .try_update(Ordering::SeqCst, Ordering::SeqCst, |n| n.checked_sub(1))
        .is_ok();
    if refused {
        return crate::http::raft::answer::<()>(Err(super::ClusterError::NotLeader(None)));
    }
    crate::http::raft::answer(node.read_index().await)
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

/// Like the real route, but node `slow.node` answers only after `slow.millis` ms.
#[get("/raft/fragment/{stream}")]
async fn fragment(
    node: web::Data<Node>,
    slow: web::Data<Slow>,
    dir: web::Data<crate::http::raft::Fragments>,
    stream: web::Path<u64>,
) -> HttpResponse {
    if slow.node.load(Ordering::SeqCst) == node.raft().metrics().borrow().id {
        let millis = slow.millis.load(Ordering::SeqCst);
        tokio::time::sleep(Duration::from_millis(millis)).await;
    }
    crate::http::raft::read_fragment(dir.0.clone(), *stream).await
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
    pub refuse: Refuse,
    pub nodes: Vec<Arc<Node>>,
    pub stores: Vec<Store>,
    /// The goals of each running node; idle until `work`.
    pub goals: Vec<Goals>,
    config: BTreeMap<u64, String>,
    pub dbs: Vec<Arc<redb::Database>>,
    servers: Vec<ServerHandle>,
    down: BTreeSet<usize>,
    assigning: AtomicBool,
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
            refuse: Refuse::default(),
            nodes: Vec::new(),
            stores: Vec::new(),
            goals: Vec::new(),
            config,
            dbs: Vec::new(),
            servers: Vec::new(),
            down: BTreeSet::new(),
            assigning: AtomicBool::new(false),
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
            Node::start(id, &self.config, db.clone(), store.clone(), SNAPSHOT_EVERY)
                .await
                .unwrap(),
        );
        let goals = Goals::open_in_cluster(db, RATE, id, &store).unwrap();
        goals.enable_scripts(store.clone(), crate::js::test_executor());
        let state = AppState {
            store,
            goals: Some(goals.clone()),
            cluster: Some(node.clone()),
            ..test_state()
        };
        let dir = super::seal::fragment_dir(&self._dir.path().join(format!("db{id}")));
        goals.connect(node.clone(), dir.clone());
        let (app, raft) = (
            crate::http::configure(state),
            crate::http::raft::configure(web::Data::from(node.clone()), dir.clone()),
        );
        let (cut, lose, slow, refuse, data, dir) = (
            self.cut.clone(),
            self.lose.clone(),
            self.slow.clone(),
            self.refuse.clone(),
            web::Data::from(node.clone()),
            dir.clone(),
        );
        let server = HttpServer::new(move || {
            // Registered first, so it answers /raft/append in place of the real route.
            App::new()
                .app_data(web::Data::new(cut.clone()))
                .app_data(web::Data::new(lose.clone()))
                .app_data(web::Data::new(slow.clone()))
                .app_data(web::Data::new(refuse.clone()))
                .app_data(data.clone())
                .app_data(web::Data::new(crate::http::raft::Fragments(dir.clone())))
                .service(append)
                .service(fragment)
                .service(forward)
                .service(read_index)
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
        self.assigning.store(true, Ordering::SeqCst);
        for i in 0..self.nodes.len() {
            self.assign_node(i);
        }
    }

    fn assign_node(&self, i: usize) {
        let (node, goals) = (self.nodes[i].clone(), self.goals[i].clone());
        super::assign::spawn(
            node,
            goals,
            i as u64 + 1,
            Some(self.fragments(i)),
            SHRINK_AFTER,
        );
    }

    /// Where node `i` keeps its fragment files.
    pub fn fragments(&self, i: usize) -> std::path::PathBuf {
        super::seal::fragment_dir(&self._dir.path().join(format!("db{}", i + 1)))
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
        if self.assigning.load(Ordering::SeqCst) {
            self.assign_node(i);
        }
    }

    /// Wait until the nodes that are up agree on one leader, and return its index. An election
    /// that is lost under load is waited out, not failed.
    pub async fn leader(&self) -> usize {
        let deadline = tokio::time::Instant::now() + 4 * WAIT.unwrap();
        loop {
            let leaders: Vec<_> = self
                .nodes
                .iter()
                .enumerate()
                .filter(|(i, _)| !self.down.contains(i))
                .map(|(_, node)| node.raft().metrics().borrow().current_leader)
                .collect();
            if let Some(Some(leader)) = leaders.first()
                && leaders.iter().all(|l| l == &Some(*leader))
            {
                return *leader as usize - 1;
            }
            assert!(
                tokio::time::Instant::now() < deadline,
                "no agreed leader: {leaders:?}"
            );
            tokio::time::sleep(std::time::Duration::from_millis(50)).await;
        }
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
