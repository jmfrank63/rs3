//! Raft consensus over the node's redb file: the log and vote live in `log`,
//! the replicated state machine in `state_machine`.
use std::collections::BTreeMap;
use std::io::Cursor;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, PoisonError};
use std::time::{Duration, Instant};

use openraft::error::{CheckIsLeaderError, ClientWriteError, InitializeError, RaftError};
use openraft::{AnyError, BasicNode, Config, ErrorSubject, ErrorVerb, Raft, SnapshotPolicy};
use serde::{Deserialize, Serialize, de::DeserializeOwned};

use self::log::RedbLog;
use self::state_machine::RedbStateMachine;
use openraft::{StorageError, StorageIOError};

use crate::store::{Command, Response, Store};

pub mod assign;
pub mod log;
pub mod network;
pub mod state_machine;
#[cfg(test)]
pub(crate) mod testing;

/// What applying a command answered: `Err` holds the key a remove did not find.
pub type Reply = Result<Response, String>;

openraft::declare_raft_types!(
    pub TypeConfig:
        D = Command,
        R = Reply,
        NodeId = u64,
        Node = BasicNode,
        SnapshotData = Cursor<Vec<u8>>,
);

/// openraft's defaults already give a 50 ms heartbeat and 150-300 ms elections (a test pins
/// them); snapshots are never taken, so the log is never purged.
pub fn config() -> Result<Config, BoxError> {
    Config {
        snapshot_policy: SnapshotPolicy::Never,
        max_in_snapshot_log_to_keep: 1 << 40,
        ..Config::default()
    }
    .validate()
    .map_err(BoxError::from)
}

pub(crate) type BoxError = Box<dyn std::error::Error + Send + Sync>;

/// Run blocking redb work off the async threads and report failure as a storage error.
/// The error is boxed because openraft's storage error is large.
pub(crate) async fn blocking<T: Send + 'static>(
    subject: ErrorSubject<u64>,
    verb: ErrorVerb,
    work: impl FnOnce() -> Result<T, BoxError> + Send + 'static,
) -> Result<T, Box<StorageError<u64>>> {
    let outcome = match tokio::task::spawn_blocking(work).await {
        Ok(done) => done.map_err(|e| e.to_string()),
        Err(e) => Err(e.to_string()),
    };
    outcome.map_err(|e| Box::new(StorageIOError::new(subject, verb, AnyError::error(e)).into()))
}

/// Why a clustered request could not be answered.
#[derive(Debug, thiserror::Error)]
pub enum ClusterError {
    /// This node is not the leader; the leader, when known, is where to go.
    #[error("not the leader")]
    NotLeader(Option<BasicNode>),
    /// No leader is known or reachable, or a quorum did not answer in time.
    #[error("no quorum")]
    NoQuorum,
    /// The write may be in the leader's log and commit later; it is never retried.
    #[error("outcome unknown")]
    OutcomeUnknown,
    #[error("{0}")]
    Unavailable(String),
}

fn unavailable(e: impl std::fmt::Display) -> ClusterError {
    ClusterError::Unavailable(e.to_string())
}

/// What a committed write answered, and where it sits in the log.
#[derive(Serialize, Deserialize)]
pub struct Written {
    pub reply: Reply,
    pub log_index: u64,
}

/// How long a request keeps looking for a leader and a quorum before it answers `NoQuorum`,
/// or `OutcomeUnknown` once its write was handed on and not answered.
const REQUEST_DEADLINE: Duration = Duration::from_secs(3);
/// The pause before a request retries after finding no leader.
const RETRY_PAUSE: Duration = Duration::from_millis(40);

/// Run `attempt` until it gives an answer other than "no leader yet", within the deadline.
/// `sent` says the attempt handed a write on, so the deadline cannot tell whether it committed.
async fn bounded<T, F: Future<Output = Result<T, ClusterError>>>(
    sent: &AtomicBool,
    attempt: impl Fn() -> F,
) -> Result<T, ClusterError> {
    let retrying = async {
        loop {
            match attempt().await {
                Err(ClusterError::NotLeader(None) | ClusterError::NoQuorum) => {
                    sent.store(false, Ordering::SeqCst);
                    tokio::time::sleep(RETRY_PAUSE).await
                }
                done => return done,
            }
        }
    };
    tokio::time::timeout(REQUEST_DEADLINE, retrying)
        .await
        .unwrap_or_else(|_| {
            Err(if sent.load(Ordering::SeqCst) {
                ClusterError::OutcomeUnknown
            } else {
                ClusterError::NoQuorum
            })
        })
}

/// One cluster member: a Raft instance on its own multi-thread runtime, so consensus
/// never competes with actix workers or the JS pool.
pub struct Node {
    raft: Raft<TypeConfig>,
    runtime: Option<tokio::runtime::Runtime>,
    http: reqwest::Client,
    heard: network::Heard,
    /// When this node last became leader; `None` while it does not lead.
    led_since: std::sync::Mutex<Option<std::time::Instant>>,
}

impl Node {
    /// Start Raft over `db` and join the cluster of `members` (id to address, self included).
    /// Initialising an already initialised cluster is not an error.
    pub async fn start(
        id: u64,
        members: &BTreeMap<u64, String>,
        db: Arc<redb::Database>,
        store: Store,
    ) -> Result<Self, BoxError> {
        let runtime = tokio::runtime::Builder::new_multi_thread()
            .thread_name("rs3-raft")
            .enable_all()
            .build()?;
        let network = network::Network::new()?;
        let started = runtime.spawn(Self::boot(id, members.clone(), db, store, network.clone()));
        match started.await {
            Ok(Ok(raft)) => Ok(Self {
                raft,
                runtime: Some(runtime),
                http: network.client,
                heard: network.heard,
                led_since: Default::default(),
            }),
            Ok(Err(e)) => Self::fail(runtime, e),
            Err(e) => Self::fail(runtime, e.into()),
        }
    }

    /// A runtime must not be dropped inside async code, so a failed start hands it off.
    fn fail<T>(runtime: tokio::runtime::Runtime, e: BoxError) -> Result<T, BoxError> {
        runtime.shutdown_background();
        Err(e)
    }

    async fn boot(
        id: u64,
        members: BTreeMap<u64, String>,
        db: Arc<redb::Database>,
        store: Store,
        network: network::Network,
    ) -> Result<Raft<TypeConfig>, BoxError> {
        let config = Arc::new(config()?);
        let (log, sm) = (RedbLog::new(db.clone())?, RedbStateMachine::new(db, store)?);
        let members: BTreeMap<u64, BasicNode> = members
            .iter()
            .map(|(i, a)| (*i, BasicNode::new(a)))
            .collect();
        let raft = Raft::new(id, config, network, log, sm).await?;
        match raft.initialize(members).await {
            Ok(()) | Err(RaftError::APIError(InitializeError::NotAllowed(_))) => Ok(raft),
            Err(e) => Err(e.into()),
        }
    }

    /// Commit `cmd` through the leader, wherever it runs, and return once this node has applied it.
    pub async fn propose(&self, cmd: Command) -> Result<Reply, ClusterError> {
        let sent = AtomicBool::new(false);
        bounded(&sent, || async {
            let written = match self.write_marking(cmd.clone(), &sent).await {
                Err(ClusterError::NotLeader(leader)) => {
                    sent.store(true, Ordering::SeqCst);
                    self.ask(leader, "forward", &cmd).await?
                }
                done => done?,
            };
            self.applied(Some(written.log_index)).await?;
            Ok(written.reply)
        })
        .await
    }

    /// Commit `cmd` here, which only the leader can do.
    ///
    /// A leader cut off from its peers never steps down, and its append would commit once they
    /// return. So the leader first confirms its quorum, which refuses a write that cannot commit.
    /// Quorum lost between that check and the append can still leave an entry that commits later.
    /// That write is reported to `propose` as `OutcomeUnknown`, never as `NoQuorum`.
    pub async fn write(&self, cmd: Command) -> Result<Written, ClusterError> {
        self.write_marking(cmd, &AtomicBool::new(false)).await
    }

    /// `write`, setting `sent` while the append may have reached the log.
    async fn write_marking(
        &self,
        cmd: Command,
        sent: &AtomicBool,
    ) -> Result<Written, ClusterError> {
        self.read_index().await?;
        sent.store(true, Ordering::SeqCst);
        match self.raft.client_write(cmd).await {
            Ok(done) => Ok(Written {
                reply: done.data,
                log_index: done.log_id.index,
            }),
            Err(RaftError::APIError(ClientWriteError::ForwardToLeader(f))) => {
                sent.store(false, Ordering::SeqCst);
                Err(ClusterError::NotLeader(f.leader_node))
            }
            Err(e) => Err(unavailable(e)),
        }
    }

    /// Wait until this node has applied every write committed before this call.
    pub async fn read_barrier(&self) -> Result<(), ClusterError> {
        bounded(&AtomicBool::new(false), || async {
            let index = match self.read_index().await {
                Err(ClusterError::NotLeader(leader)) => self.ask(leader, "read_index", &()).await?,
                done => done?,
            };
            self.applied(index).await
        })
        .await
    }

    /// The log index a read must see, confirmed by a quorum; only the leader can give it.
    pub async fn read_index(&self) -> Result<Option<u64>, ClusterError> {
        match self.raft.get_read_log_id().await {
            Ok((read, _)) => Ok(read.map(|id| id.index)),
            Err(RaftError::APIError(CheckIsLeaderError::ForwardToLeader(f))) => {
                Err(ClusterError::NotLeader(f.leader_node))
            }
            Err(RaftError::APIError(CheckIsLeaderError::QuorumNotEnough(_))) => {
                Err(ClusterError::NoQuorum)
            }
            Err(e) => Err(unavailable(e)),
        }
    }

    async fn applied(&self, index: Option<u64>) -> Result<(), ClusterError> {
        self.raft
            .wait(None)
            .applied_index_at_least(index, "the request's index applied")
            .await
            .map(drop)
            .map_err(unavailable)
    }

    /// Post `body` to the leader's `/raft/{path}` and decode its answer.
    async fn ask<Q: Serialize, T: DeserializeOwned>(
        &self,
        leader: Option<BasicNode>,
        path: &str,
        body: &Q,
    ) -> Result<T, ClusterError> {
        let leader = leader.ok_or(ClusterError::NotLeader(None))?;
        let url = format!("http://{}/raft/{path}", leader.addr);
        let reply = self.http.post(url).json(body).send().await.map_err(|e| {
            if e.is_connect() {
                ClusterError::NoQuorum
            } else if path == "forward" {
                ClusterError::OutcomeUnknown
            } else {
                unavailable(e)
            }
        })?;
        if !reply.status().is_success() {
            let (status, why) = (reply.status(), reply.text().await.unwrap_or_default());
            if why == ClusterError::NoQuorum.to_string() {
                return Err(ClusterError::NoQuorum);
            }
            return Err(unavailable(format!("{path} refused with {status}: {why}")));
        }
        reply.json().await.map_err(unavailable)
    }

    /// `propose` for callers outside any runtime, such as a worker thread.
    pub fn propose_blocking(&self, cmd: Command) -> Result<Reply, ClusterError> {
        self.handle().block_on(self.propose(cmd))
    }

    /// The members that have not answered this leader for `silence`. A member never heard from
    /// counts from the moment this node became leader. A node that does not lead names no one.
    pub fn lost_peers(&self, silence: Duration) -> Vec<u64> {
        let metrics = self.raft.metrics().borrow().clone();
        let mut led = self
            .led_since
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        if metrics.current_leader != Some(metrics.id) {
            *led = None;
            return Vec::new();
        }
        let since = *led.get_or_insert_with(Instant::now);
        let heard = self.heard.lock().unwrap_or_else(PoisonError::into_inner);
        let members = metrics.membership_config.membership().voter_ids();
        members
            .filter(|&m| m != metrics.id)
            .filter(|m| {
                let at = heard.get(m).map_or(since, |&at| at.max(since));
                !at.elapsed().saturating_sub(silence).is_zero()
            })
            .collect()
    }

    pub fn raft(&self) -> &Raft<TypeConfig> {
        &self.raft
    }

    pub fn handle(&self) -> tokio::runtime::Handle {
        self.runtime
            .as_ref()
            .map_or_else(tokio::runtime::Handle::current, |r| r.handle().clone())
    }
}

impl Drop for Node {
    /// A runtime must not be dropped inside async code; hand its threads off instead.
    fn drop(&mut self) {
        if let Some(runtime) = self.runtime.take() {
            runtime.shutdown_background();
        }
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;
    use std::path::Path;
    use std::sync::Arc;
    use std::time::Duration;

    use openraft::error::{RPCError, RaftError, Unreachable};
    use openraft::network::{RPCOption, RaftNetwork, RaftNetworkFactory};
    use openraft::raft::{AppendEntriesRequest, AppendEntriesResponse};
    use openraft::raft::{InstallSnapshotRequest, InstallSnapshotResponse};
    use openraft::raft::{VoteRequest, VoteResponse};
    use openraft::storage::{RaftLogStorage, RaftStateMachine};
    use openraft::{Raft, RaftLogReader, RaftSnapshotBuilder};

    use super::log::RedbLog;
    use super::state_machine::RedbStateMachine;
    use super::*;
    use crate::store::Store;

    /// A network nobody answers on; a one-node cluster never calls it.
    struct NoNetwork;

    impl RaftNetworkFactory<TypeConfig> for NoNetwork {
        type Network = NoNetwork;
        async fn new_client(&mut self, _target: u64, _node: &BasicNode) -> NoNetwork {
            NoNetwork
        }
    }

    fn down<E: std::error::Error>() -> RPCError<u64, BasicNode, E> {
        RPCError::Unreachable(Unreachable::new(&AnyError::error("no network")))
    }

    impl RaftNetwork<TypeConfig> for NoNetwork {
        async fn append_entries(
            &mut self,
            _: AppendEntriesRequest<TypeConfig>,
            _: RPCOption,
        ) -> Result<AppendEntriesResponse<u64>, RPCError<u64, BasicNode, RaftError<u64>>> {
            Err(down())
        }
        async fn install_snapshot(
            &mut self,
            _: InstallSnapshotRequest<TypeConfig>,
            _: RPCOption,
        ) -> Result<
            InstallSnapshotResponse<u64>,
            RPCError<u64, BasicNode, RaftError<u64, openraft::error::InstallSnapshotError>>,
        > {
            Err(down())
        }
        async fn vote(
            &mut self,
            _: VoteRequest<u64>,
            _: RPCOption,
        ) -> Result<VoteResponse<u64>, RPCError<u64, BasicNode, RaftError<u64>>> {
            Err(down())
        }
    }

    const WAIT: Option<Duration> = Some(Duration::from_secs(10));

    struct Node {
        raft: Raft<TypeConfig>,
        store: Store,
        log: RedbLog,
        sm: RedbStateMachine,
    }

    fn storage(path: &Path) -> (Store, RedbLog, RedbStateMachine) {
        let db = crate::storage::open(path).unwrap();
        let store = Store::durable(db.clone()).unwrap();
        let log = RedbLog::new(db.clone()).unwrap();
        let sm = RedbStateMachine::new(db, store.clone()).unwrap();
        (store, log, sm)
    }

    async fn start(path: &Path) -> Node {
        let (store, log, sm) = storage(path);
        let config = Arc::new(config().unwrap());
        let raft = Raft::new(1, config, NoNetwork, log.clone(), sm.clone())
            .await
            .unwrap();
        Node {
            raft,
            store,
            log,
            sm,
        }
    }

    fn put(k: &str, v: &str) -> Command {
        Command::Put {
            entries: vec![(k.to_owned(), v.to_owned())],
        }
    }

    fn remove(k: &str) -> Command {
        Command::Remove { key: k.to_owned() }
    }

    async fn write(node: &Node, cmd: Command) -> Reply {
        node.raft.client_write(cmd).await.unwrap().data
    }

    /// Start a fresh one-node cluster, write a, b, remove a, and stop it.
    async fn written_node(path: &Path) -> Node {
        let node = start(path).await;
        let members = BTreeMap::from([(1, BasicNode::default())]);
        node.raft.initialize(members).await.unwrap();
        node.raft
            .wait(WAIT)
            .current_leader(1, "leader")
            .await
            .unwrap();
        assert_eq!(write(&node, put("a", "1")).await, Ok(Response::Put));
        assert_eq!(write(&node, put("b", "2")).await, Ok(Response::Put));
        assert_eq!(
            write(&node, remove("a")).await,
            Ok(Response::Removed("1".into()))
        );
        assert_eq!(write(&node, remove("x")).await, Err("x".to_owned()));
        node
    }

    #[test]
    fn the_config_uses_fast_elections_and_never_snapshots_or_purges() {
        let c = config().unwrap();
        assert_eq!(
            (
                c.heartbeat_interval,
                c.election_timeout_min,
                c.election_timeout_max
            ),
            (50, 150, 300)
        );
        assert!(matches!(c.snapshot_policy, SnapshotPolicy::Never));
        assert!(c.max_in_snapshot_log_to_keep >= 1 << 40);
    }

    fn kv(k: &str, v: &str) -> (String, String) {
        (k.to_owned(), v.to_owned())
    }

    #[tokio::test]
    async fn a_one_node_cluster_applies_commands_and_resumes_after_restart() {
        let dir = crate::storage::TempDir::new();
        let path = dir.path().join("db");
        let node = written_node(&path).await;
        assert_eq!(node.store.list(), vec![kv("b", "2")]);
        let last = node.raft.metrics().borrow().last_applied.unwrap();
        node.raft.shutdown().await.unwrap();
        drop(node);

        let (store, mut log, mut sm) = storage(&path);
        assert_eq!(store.list(), vec![kv("b", "2")]);
        assert_eq!(sm.applied_state().await.unwrap().0, Some(last));
        assert_eq!(log.get_log_state().await.unwrap().last_log_id, Some(last));
        assert!(log.read_vote().await.unwrap().is_some());
        let committed = log.read_committed().await.unwrap();
        assert!(committed.is_some_and(|c| c.index <= last.index));
    }

    #[tokio::test]
    async fn truncate_drops_the_log_tail_and_purge_the_head() {
        let dir = crate::storage::TempDir::new();
        let node = written_node(&dir.path().join("db")).await;
        node.raft.shutdown().await.unwrap();
        let mut log = node.log.clone();
        let all = log.try_get_log_entries(..).await.unwrap();
        assert!(all.len() >= 6);
        let indexes = |entries: &[openraft::Entry<TypeConfig>]| -> Vec<u64> {
            entries.iter().map(|e| e.log_id.index).collect()
        };
        assert_eq!(
            indexes(&log.try_get_log_entries(2..4).await.unwrap()),
            vec![2, 3]
        );

        log.truncate(all[3].log_id).await.unwrap();
        let state = log.get_log_state().await.unwrap();
        assert_eq!(state.last_log_id, Some(all[2].log_id));
        assert_eq!(state.last_purged_log_id, None);

        log.purge(all[1].log_id).await.unwrap();
        let state = log.get_log_state().await.unwrap();
        assert_eq!(state.last_purged_log_id, Some(all[1].log_id));
        assert_eq!(state.last_log_id, Some(all[2].log_id));
        assert_eq!(
            indexes(&log.try_get_log_entries(..).await.unwrap()),
            vec![all[2].log_id.index]
        );
    }

    #[tokio::test]
    async fn a_snapshot_installs_the_data_and_position_on_another_node() {
        let dir = crate::storage::TempDir::new();
        let node = written_node(&dir.path().join("a")).await;
        node.raft.shutdown().await.unwrap();
        let mut from = node.sm.clone();
        let snapshot = from.build_snapshot().await.unwrap();

        let path_b = dir.path().join("b");
        let (store, _, mut to) = storage(&path_b);
        store.insert_all([kv("stale", "x")]).unwrap();
        assert!(to.get_current_snapshot().await.unwrap().is_none());
        let data = to.begin_receiving_snapshot().await.unwrap();
        assert!(data.get_ref().is_empty());
        to.install_snapshot(&snapshot.meta, snapshot.snapshot)
            .await
            .unwrap();

        assert_eq!(store.list(), vec![kv("b", "2")]);
        let (applied, membership) = to.applied_state().await.unwrap();
        assert_eq!(applied, snapshot.meta.last_log_id);
        assert_eq!(membership, snapshot.meta.last_membership);
        assert!(applied.is_some());
        let current = to.get_current_snapshot().await.unwrap().unwrap();
        assert_eq!(current.meta, snapshot.meta);
        drop((store, to));
        assert_eq!(storage(&path_b).0.list(), vec![kv("b", "2")]);
    }

    fn goal_start() -> crate::journal::Record {
        crate::journal::Record::InferenceStart {
            engine: "fake".into(),
            model_hash: "fake".into(),
            tokenizer_hash: "fake".into(),
            sampler: "{}".into(),
            rng_state: vec![1],
            prompt_tokens: vec![7],
        }
    }

    /// Propose from a plain thread, as a goal worker does.
    async fn propose_on_thread(node: Arc<super::Node>, cmd: Command) -> Reply {
        tokio::task::spawn_blocking(move || {
            std::thread::spawn(move || node.propose_blocking(cmd))
                .join()
                .unwrap()
        })
        .await
        .unwrap()
        .unwrap()
    }

    #[actix_web::test]
    async fn a_goal_created_through_a_follower_exists_with_the_same_id_on_every_node() {
        let cluster = super::testing::Cluster::start(3, 3).await;
        let follower = cluster.nodes[cluster.follower().await].clone();
        let create = || Command::CreateGoal {
            priority: 0,
            budget: 4,
            start: goal_start(),
        };
        assert_eq!(
            propose_on_thread(follower.clone(), create()).await,
            Ok(Response::GoalCreated(1))
        );
        assert_eq!(
            propose_on_thread(follower, create()).await,
            Ok(Response::GoalCreated(2))
        );
        for (node, db) in cluster.nodes.iter().zip(&cluster.dbs) {
            node.read_barrier().await.unwrap();
            let journal = crate::journal::Journal::new(db.clone()).unwrap();
            for id in [1, 2] {
                assert_eq!(journal.read(id).unwrap(), vec![goal_start()]);
            }
        }
        let cancel = Command::CancelGoal { id: 1 };
        let leader = cluster.nodes[cluster.leader().await].clone();
        assert_eq!(
            propose_on_thread(leader.clone(), cancel.clone()).await,
            Ok(Response::Cancelled)
        );
        assert_eq!(
            propose_on_thread(leader, cancel).await,
            Ok(Response::NotLive)
        );
        cluster.stop().await;
    }

    #[actix_web::test]
    async fn goals_created_through_different_nodes_are_spread_over_several_owners() {
        let cluster = super::testing::Cluster::start(3, 3).await;
        let leader = cluster.leader().await;
        let applied = || cluster.nodes[leader].raft().metrics().borrow().last_applied;
        let mut ids = vec![];
        for goals in cluster.goals.iter().cycle().take(6) {
            let goals = goals.clone();
            let spec = crate::goals::GoalSpec {
                priority: 0,
                budget: 10,
                seed: 1,
                prompt: "hi".into(),
            };
            ids.push(
                tokio::task::spawn_blocking(move || goals.create(spec).unwrap())
                    .await
                    .unwrap(),
            );
        }
        let before = applied();
        cluster.assign();
        let owners = |db: &Arc<redb::Database>| -> Vec<Option<u64>> {
            ids.iter()
                .map(|&id| crate::lease::owner(db, id).unwrap())
                .collect()
        };
        let deadline = std::time::Instant::now() + Duration::from_secs(15);
        let mut seen = owners(&cluster.dbs[0]);
        while seen.contains(&None) && std::time::Instant::now() < deadline {
            tokio::time::sleep(Duration::from_millis(50)).await;
            seen = owners(&cluster.dbs[0]);
        }
        assert!(!seen.contains(&None), "unassigned goals: {seen:?}");
        let distinct: std::collections::BTreeSet<_> = seen.iter().collect();
        assert_eq!(distinct.len(), 3, "owners: {seen:?}");
        // Only the leader plans: a pause of several ticks adds one Assign per goal and no more.
        tokio::time::sleep(Duration::from_millis(500)).await;
        assert_eq!(
            applied().map(|l| l.index),
            before.map(|l| l.index + ids.len() as u64)
        );
        for n in 1..=3 {
            assert_eq!(seen.iter().filter(|o| **o == Some(n)).count(), 2);
        }
        cluster.stop().await;
    }

    fn tokens(n: u32) -> crate::journal::Record {
        crate::journal::Record::Tokens {
            tokens: vec![n],
            rng_state: vec![n as u8],
        }
    }

    /// Every node's journal bytes and lease rows for `id`, once it has caught up.
    async fn snapshots(
        cluster: &super::testing::Cluster,
        id: u64,
    ) -> Vec<(Vec<crate::journal::Record>, u64, Option<u64>)> {
        let mut out = vec![];
        for (node, db) in cluster.nodes.iter().zip(&cluster.dbs) {
            node.read_barrier().await.unwrap();
            let journal = crate::journal::Journal::new(db.clone()).unwrap();
            let txn = db.begin_write().unwrap();
            out.push((
                journal.read(id).unwrap(),
                crate::lease::token_in(&txn, id).unwrap(),
                crate::lease::owner_in(&txn, id).unwrap(),
            ));
        }
        out
    }

    #[actix_web::test]
    async fn a_stale_fence_append_is_refused_on_every_node_and_changes_nothing() {
        let cluster = super::testing::Cluster::start(3, 3).await;
        let leader = cluster.nodes[cluster.leader().await].clone();
        let id = match propose_on_thread(
            leader.clone(),
            Command::CreateGoal {
                priority: 0,
                budget: 4,
                start: goal_start(),
            },
        )
        .await
        {
            Ok(Response::GoalCreated(id)) => id,
            other => panic!("{other:?}"),
        };
        let append = |token, seq| Command::Append {
            task: id,
            token,
            seq,
            records: vec![tokens(1), tokens(2)],
        };
        // A claim before assignment is refused and changes nothing.
        let before = snapshots(&cluster, id).await;
        assert_eq!(
            propose_on_thread(leader.clone(), Command::Claim { task: id, node: 2 }).await,
            Ok(Response::Refused("not_owner".into()))
        );
        assert_eq!(snapshots(&cluster, id).await, before);

        // Assign with a wrong expected token is refused.
        assert_eq!(
            propose_on_thread(
                leader.clone(),
                Command::Assign {
                    task: id,
                    node: 2,
                    expect_token: 5
                }
            )
            .await,
            Ok(Response::Refused("stale_assign".into()))
        );
        assert_eq!(snapshots(&cluster, id).await, before);

        assert_eq!(
            propose_on_thread(
                leader.clone(),
                Command::Assign {
                    task: id,
                    node: 2,
                    expect_token: 0
                }
            )
            .await,
            Ok(Response::Assigned { token: 1 })
        );
        // Claim by another node is refused; the owner's claim bumps the token.
        assert_eq!(
            propose_on_thread(leader.clone(), Command::Claim { task: id, node: 3 }).await,
            Ok(Response::Refused("not_owner".into()))
        );
        assert_eq!(
            propose_on_thread(leader.clone(), Command::Claim { task: id, node: 2 }).await,
            Ok(Response::Claimed { token: 2 })
        );
        assert_eq!(
            propose_on_thread(leader.clone(), append(2, 1)).await,
            Ok(Response::Appended)
        );
        let good = snapshots(&cluster, id).await;
        assert!(
            good.iter()
                .all(|s| s == &good[0] && s.0.len() == 3 && s.1 == 2 && s.2 == Some(2))
        );

        // The old token, a seq conflict and a half-valid batch all leave every node unchanged.
        let stale = propose_on_thread(leader.clone(), append(1, 3)).await;
        assert_eq!(
            stale,
            Ok(Response::Fenced {
                task: id,
                held: 1,
                current: 2
            })
        );
        assert_eq!(
            propose_on_thread(leader.clone(), append(2, 2)).await,
            Ok(Response::Refused("seq_conflict".into()))
        );
        let half = Command::Append {
            task: id,
            token: 2,
            seq: 3,
            records: vec![
                tokens(9),
                crate::journal::Record::InferenceEnd { reason: "x".into() },
                tokens(9),
            ],
        };
        assert_eq!(
            propose_on_thread(leader, half).await,
            Ok(Response::Refused("ended".into()))
        );
        assert_eq!(snapshots(&cluster, id).await, good);
        cluster.stop().await;
    }

    #[actix_web::test]
    async fn a_script_op_commits_its_write_and_record_together_or_not_at_all() {
        use crate::journal::{OpError, Record};
        let cluster = super::testing::Cluster::start(3, 3).await;
        let leader = cluster.nodes[cluster.leader().await].clone();
        let start = Record::ScriptStart {
            name: "s".into(),
            source: String::new(),
        };
        let id = match propose_on_thread(
            leader.clone(),
            Command::CreateGoal {
                priority: 0,
                budget: 0,
                start,
            },
        )
        .await
        {
            Ok(Response::GoalCreated(id)) => id,
            other => panic!("{other:?}"),
        };
        for cmd in [
            Command::Assign {
                task: id,
                node: 2,
                expect_token: 0,
            },
            Command::Claim { task: id, node: 2 },
        ] {
            propose_on_thread(leader.clone(), cmd).await.unwrap();
        }
        let op = |name: &str, result: Result<&str, ()>| Record::OpResult {
            op: name.into(),
            args: vec![],
            result: result.map(str::to_owned).map_err(|()| OpError {
                class: "Error".into(),
                message: "x".into(),
            }),
        };
        let script_op = |token, seq, record, write: Option<Command>| Command::ScriptOp {
            task: id,
            token,
            seq,
            record,
            write: write.map(Box::new),
        };
        assert_eq!(
            propose_on_thread(
                leader.clone(),
                script_op(2, 1, op("w", Ok("a")), Some(put("k", "v")))
            )
            .await,
            Ok(Response::Appended)
        );
        // A removal logs the value it removed, not the guess in the record.
        assert_eq!(
            propose_on_thread(
                leader.clone(),
                script_op(2, 2, op("d", Ok("guess")), Some(remove("k")))
            )
            .await,
            Ok(Response::Removed("v".into()))
        );
        let good = snapshots(&cluster, id).await;
        assert!(good.iter().all(|s| s == &good[0]));
        assert_eq!(good[0].0[1..], [op("w", Ok("a")), op("d", Ok("v"))]);
        for store in &cluster.stores {
            assert!(store.get("k").is_err());
        }
        // A missing key is logged as the refusal the script saw, in the same command.
        let not_found = op("d", Err(()));
        assert_eq!(
            propose_on_thread(
                leader.clone(),
                script_op(2, 3, not_found.clone(), Some(remove("k")))
            )
            .await,
            Ok(Response::Missing)
        );
        let missing = snapshots(&cluster, id).await;
        assert_eq!(missing[0].0.len(), good[0].0.len() + 1);
        assert_eq!(missing[0].0.last(), Some(&not_found));
        // A stale token and a stale seq write and log nothing.
        let refused = [
            (
                script_op(1, 4, op("w", Ok("")), Some(put("z", "1"))),
                Response::Fenced {
                    task: id,
                    held: 1,
                    current: 2,
                },
            ),
            (
                script_op(2, 3, op("w", Ok("")), Some(put("z", "1"))),
                Response::Refused("seq_conflict".into()),
            ),
        ];
        for (cmd, answer) in refused {
            assert_eq!(propose_on_thread(leader.clone(), cmd).await, Ok(answer));
        }
        // A read-only op is just a record.
        assert_eq!(
            propose_on_thread(leader, script_op(2, 4, op("g", Err(())), None)).await,
            Ok(Response::Appended)
        );
        let after = snapshots(&cluster, id).await;
        assert_eq!(after[0].0.len(), missing[0].0.len() + 1);
        for store in &cluster.stores {
            assert!(store.get("z").is_err());
        }
        cluster.stop().await;
    }

    #[actix_web::test]
    async fn a_script_goal_moved_to_another_node_replays_its_log_and_writes_once() {
        use crate::journal::Record;
        let cluster = super::testing::Cluster::start(3, 3).await;
        let leader = cluster.nodes[cluster.leader().await].clone();
        let source = r#"const v = Deno.core.ops.op_rs3_delete('a');
            Deno.core.ops.op_rs3_insert('{"b":"' + v + '"}'); v"#;
        let goals = cluster.goals[0].clone();
        let id = off(move || goals.create_script(0, "s".into(), source.into()).unwrap()).await;
        // Node 1 deleted `a` under its claim, then died: the delete is logged.
        for cmd in [
            put("a", "1"),
            Command::Assign {
                task: id,
                node: 1,
                expect_token: 0,
            },
            Command::Claim { task: id, node: 1 },
            Command::ScriptOp {
                task: id,
                token: 2,
                seq: 1,
                record: Record::OpResult {
                    op: "op_rs3_delete".into(),
                    args: vec!["a".into()],
                    result: Ok("1".into()),
                },
                write: Some(Box::new(remove("a"))),
            },
            Command::Assign {
                task: id,
                node: 3,
                expect_token: 2,
            },
        ] {
            propose_on_thread(leader.clone(), cmd).await.unwrap();
        }
        cluster.work();
        wait_everywhere(&cluster, id, |s| s.status == "done").await;
        for goals in &cluster.goals {
            let status = status_of(goals, id).await.unwrap();
            assert_eq!(status.output.as_deref(), Some("1"));
            assert_eq!(status.node, Some(3));
        }
        for store in &cluster.stores {
            assert!(store.get("a").is_err());
            assert_eq!(store.get("b").as_deref(), Ok("1"));
        }
        cluster.stop().await;
    }

    #[actix_web::test]
    async fn a_script_goal_that_deleted_a_missing_key_replays_it_on_another_node() {
        use crate::journal::{OpError, Record};
        let cluster = super::testing::Cluster::start(3, 3).await;
        let leader = cluster.nodes[cluster.leader().await].clone();
        let source = r#"let seen = 'no throw';
            try { Deno.core.ops.op_rs3_delete('a') } catch (e) { seen = e.message }
            Deno.core.ops.op_rs3_insert('{"b":"x"}'); seen"#;
        let goals = cluster.goals[0].clone();
        let id = off(move || goals.create_script(0, "s".into(), source.into()).unwrap()).await;
        // Node 1 tried to delete the missing `a`, logged the refusal, then died.
        for cmd in [
            Command::Assign {
                task: id,
                node: 1,
                expect_token: 0,
            },
            Command::Claim { task: id, node: 1 },
            Command::ScriptOp {
                task: id,
                token: 2,
                seq: 1,
                record: Record::OpResult {
                    op: "op_rs3_delete".into(),
                    args: vec!["a".into()],
                    result: Err(OpError {
                        class: "Error".into(),
                        message: "key not found: a".into(),
                    }),
                },
                write: Some(Box::new(remove("a"))),
            },
            Command::Assign {
                task: id,
                node: 3,
                expect_token: 2,
            },
        ] {
            propose_on_thread(leader.clone(), cmd).await.unwrap();
        }
        cluster.work();
        wait_everywhere(&cluster, id, |s| s.status == "done").await;
        for goals in &cluster.goals {
            let status = status_of(goals, id).await.unwrap();
            assert_eq!(status.output.as_deref(), Some("key not found: a"));
        }
        everywhere(&cluster, "b", "x").await;
        cluster.stop().await;
    }

    /// Poll until `key` reads `value` on every node, or fail at a deadline.
    async fn everywhere(cluster: &super::testing::Cluster, key: &str, value: &str) {
        let deadline = std::time::Instant::now() + Duration::from_secs(15);
        while !cluster
            .stores
            .iter()
            .all(|s| s.get(key).as_deref() == Ok(value))
        {
            assert!(
                std::time::Instant::now() < deadline,
                "{key} never reached every node"
            );
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
    }

    #[actix_web::test]
    async fn a_write_whose_commit_is_cut_off_answers_outcome_unknown_and_commits_after_healing() {
        let cluster = super::testing::Cluster::start(3, 3).await;
        let (leader, follower) = (cluster.leader().await, cluster.follower().await);
        cluster
            .cut
            .0
            .store(true, std::sync::atomic::Ordering::SeqCst);
        for (who, key) in [(leader, "by-leader"), (follower, "by-follower")] {
            let got = cluster.nodes[who].propose(put(key, "v")).await;
            assert!(
                matches!(got, Err(ClusterError::OutcomeUnknown)),
                "{key}: {got:?}"
            );
        }
        cluster
            .cut
            .0
            .store(false, std::sync::atomic::Ordering::SeqCst);
        everywhere(&cluster, "by-leader", "v").await;
        everywhere(&cluster, "by-follower", "v").await;
        cluster.stop().await;
    }

    use crate::goals::{GoalSpec, GoalStatus, Goals};

    fn spec(budget: u64) -> GoalSpec {
        GoalSpec {
            priority: 0,
            budget,
            seed: 7,
            prompt: "hello".into(),
        }
    }

    /// What a single node writes for `spec(budget)`. Fails, never hangs, after 10 s.
    fn single_node_output(budget: u64) -> String {
        let (tx, rx) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            let dir = crate::storage::TempDir::new();
            let goals = Goals::open(
                crate::storage::open(&dir.path().join("db")).unwrap(),
                super::testing::RATE,
            )
            .unwrap();
            let id = goals.create(spec(budget)).unwrap();
            while let Some(goal) = goals.next_goal() {
                goals.run(goal);
            }
            let _ = tx.send(goals.status(id).unwrap().unwrap().output.unwrap());
        });
        rx.recv_timeout(Duration::from_secs(10))
            .expect("the single-node run overran its deadline")
    }

    /// Run `f` on a plain thread, as a request handler would.
    async fn off<T: Send + 'static>(f: impl FnOnce() -> T + Send + 'static) -> T {
        tokio::task::spawn_blocking(move || std::thread::spawn(f).join().unwrap())
            .await
            .unwrap()
    }

    /// The status of goal `id` on one node; `None` until that node has applied its creation.
    async fn status_of(goals: &Goals, id: u64) -> Option<GoalStatus> {
        let goals = goals.clone();
        off(move || goals.status(id).unwrap()).await
    }

    /// Wait until `ready` holds for goal `id` on every node, or fail after 15 s.
    async fn wait_everywhere(
        cluster: &super::testing::Cluster,
        id: u64,
        ready: impl Fn(&GoalStatus) -> bool,
    ) {
        let deadline = std::time::Instant::now() + Duration::from_secs(15);
        for goals in &cluster.goals {
            loop {
                let status = status_of(goals, id).await;
                if status.as_ref().is_some_and(&ready) {
                    break;
                }
                assert!(std::time::Instant::now() < deadline, "stuck at {status:?}");
                tokio::time::sleep(Duration::from_millis(50)).await;
            }
        }
    }

    async fn create_and_assign(cluster: &super::testing::Cluster, budget: u64, node: u64) -> u64 {
        let goals = cluster.goals[cluster.follower().await].clone();
        let id = off(move || goals.create(spec(budget)).unwrap()).await;
        let leader = cluster.nodes[cluster.leader().await].clone();
        let assign = Command::Assign {
            task: id,
            node,
            expect_token: 0,
        };
        assert!(matches!(
            propose_on_thread(leader, assign).await,
            Ok(Response::Assigned { .. })
        ));
        id
    }

    #[actix_web::test]
    async fn a_goal_assigned_to_one_node_finishes_there_with_a_single_node_output() {
        let cluster = super::testing::Cluster::start(3, 3).await;
        cluster.work();
        let id = create_and_assign(&cluster, 20, 2).await;
        wait_everywhere(&cluster, id, |s| s.status == "done").await;
        let expected = single_node_output(20);
        for goals in &cluster.goals {
            let status = status_of(goals, id).await.unwrap();
            assert_eq!(status.output.as_deref(), Some(expected.as_str()));
            assert_eq!(status.node, Some(2));
        }
        // The worker claimed once, for its whole run: the lease token is still 2.
        let leader = cluster.nodes[cluster.leader().await].clone();
        let again = Command::Assign {
            task: id,
            node: 2,
            expect_token: 2,
        };
        assert_eq!(
            propose_on_thread(leader, again).await,
            Ok(Response::Assigned { token: 3 })
        );
        cluster.stop().await;
    }

    #[actix_web::test]
    async fn a_goal_is_running_everywhere_while_its_owner_works_and_unassigned_goals_are_queued() {
        let cluster = super::testing::Cluster::start(3, 3).await;
        cluster.work();
        cluster.leader().await;
        let goals = cluster.goals[0].clone();
        let id = off(move || goals.create(spec(5000)).unwrap()).await;
        wait_everywhere(&cluster, id, |s| s.status == "queued" && s.node.is_none()).await;
        let leader = cluster.nodes[cluster.leader().await].clone();
        let assign = Command::Assign {
            task: id,
            node: 3,
            expect_token: 0,
        };
        propose_on_thread(leader, assign).await.unwrap();
        wait_everywhere(&cluster, id, |s| s.status == "running" && s.node == Some(3)).await;
        cluster.stop().await;
    }

    #[actix_web::test]
    async fn cancelling_through_a_follower_ends_the_goal_everywhere_and_stops_its_output() {
        let cluster = super::testing::Cluster::start(3, 3).await;
        cluster.work();
        let id = create_and_assign(&cluster, 5000, 2).await;
        wait_everywhere(&cluster, id, |s| {
            s.output.as_deref().is_some_and(|o| !o.is_empty())
        })
        .await;
        let goals = cluster.goals[cluster.follower().await].clone();
        off(move || goals.cancel(id).unwrap()).await;
        wait_everywhere(&cluster, id, |s| s.status == "cancelled").await;
        let frozen = status_of(&cluster.goals[1], id).await.unwrap().output;
        tokio::time::sleep(Duration::from_millis(500)).await;
        for goals in &cluster.goals {
            let status = status_of(goals, id).await.unwrap();
            assert_eq!(
                (status.status, status.output),
                ("cancelled", frozen.clone())
            );
        }
        cluster.stop().await;
    }

    #[actix_web::test]
    async fn only_the_node_a_goal_is_assigned_to_picks_it_up() {
        let cluster = super::testing::Cluster::start(3, 3).await;
        cluster.leader().await;
        let goals = cluster.goals[0].clone();
        let id = off(move || goals.create(spec(5)).unwrap()).await;
        let leader = cluster.nodes[cluster.leader().await].clone();
        let assign = Command::Assign {
            task: id,
            node: 2,
            expect_token: 0,
        };
        propose_on_thread(leader, assign).await.unwrap();
        wait_everywhere(&cluster, id, |s| s.node == Some(2)).await;
        let mut picked = Vec::new();
        for goals in cluster.goals.clone() {
            picked.push(off(move || goals.next_goal().map(|g| g.id)).await);
        }
        assert_eq!(picked, vec![None, Some(id), None]);
        cluster.stop().await;
    }

    #[actix_web::test]
    async fn a_goal_assigned_twice_is_queued_once_and_a_cancelled_one_is_not_queued() {
        let cluster = super::testing::Cluster::start(3, 3).await;
        cluster.leader().await;
        let goals = cluster.goals[0].clone();
        let (a, b) = off(move || {
            (
                goals.create(spec(5)).unwrap(),
                goals.create(spec(5)).unwrap(),
            )
        })
        .await;
        let leader = cluster.nodes[cluster.leader().await].clone();
        for (task, expect_token) in [(a, 0), (a, 1), (b, 0)] {
            let assign = Command::Assign {
                task,
                node: 2,
                expect_token,
            };
            propose_on_thread(leader.clone(), assign).await.unwrap();
        }
        let goals = cluster.goals[2].clone();
        off(move || goals.cancel(b).unwrap()).await;
        wait_everywhere(&cluster, b, |s| s.status == "cancelled").await;
        let goals = cluster.goals[1].clone();
        let picked = off(move || vec![goals.next_goal(), goals.next_goal()]).await;
        assert_eq!(
            picked.iter().flatten().map(|g| g.id).collect::<Vec<_>>(),
            vec![a]
        );
        cluster.stop().await;
    }

    #[actix_web::test]
    async fn a_goal_whose_commits_are_cut_off_for_a_while_still_finishes_identically() {
        let cluster = super::testing::Cluster::start(3, 3).await;
        cluster.work();
        let id = create_and_assign(&cluster, 400, 2).await;
        // A batch is committed, so the next append is the one that gets cut off.
        wait_everywhere(&cluster, id, |s| {
            s.output.as_deref().is_some_and(|o| !o.is_empty())
        })
        .await;
        let cut = |on| cluster.cut.0.store(on, std::sync::atomic::Ordering::SeqCst);
        cut(true);
        tokio::time::sleep(Duration::from_millis(3500)).await;
        cut(false);
        wait_everywhere(&cluster, id, |s| s.status == "done").await;
        let expected = single_node_output(400);
        for goals in &cluster.goals {
            let status = status_of(goals, id).await.unwrap();
            assert_eq!(status.output.as_deref(), Some(expected.as_str()));
        }
        cluster.stop().await;
    }

    #[actix_web::test]
    async fn an_append_that_commits_but_is_not_answered_is_not_generated_again() {
        let cluster = super::testing::Cluster::start(3, 3).await;
        cluster.work();
        let owner = cluster.follower().await;
        cluster
            .lose
            .armed
            .store(true, std::sync::atomic::Ordering::SeqCst);
        let id = create_and_assign(&cluster, 400, owner as u64 + 1).await;
        let deadline = std::time::Instant::now() + Duration::from_secs(15);
        while !cluster.lose.lag.load(std::sync::atomic::Ordering::SeqCst) {
            assert!(std::time::Instant::now() < deadline, "no answer was lost");
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
        // The owner's copy stays behind the commit for a while; then it catches up.
        tokio::time::sleep(Duration::from_millis(1000)).await;
        cluster
            .lose
            .lag
            .store(false, std::sync::atomic::Ordering::SeqCst);
        wait_everywhere(&cluster, id, |s| s.status == "done").await;
        let expected = single_node_output(400);
        for goals in &cluster.goals {
            let status = status_of(goals, id).await.unwrap();
            assert_eq!(status.output.as_deref(), Some(expected.as_str()));
        }
        assert_eq!(cluster.goals[owner].generated_tokens(), 400);
        cluster.stop().await;
    }

    /// Wait up to 10 s for `lost_peers` of node `i` to satisfy `ok`.
    async fn lost_when(
        cluster: &super::testing::Cluster,
        i: usize,
        ok: impl Fn(&[u64]) -> bool,
    ) -> bool {
        let deadline = std::time::Instant::now() + Duration::from_secs(10);
        loop {
            if ok(&cluster.nodes[i].lost_peers(Duration::from_millis(500))) {
                return true;
            }
            if std::time::Instant::now() > deadline {
                return false;
            }
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
    }

    #[actix_web::test]
    async fn the_leader_names_a_stopped_follower_lost_until_it_is_back() {
        let mut cluster = super::testing::Cluster::start(3, 3).await;
        let leader = cluster.leader().await;
        assert!(
            cluster.nodes[leader]
                .lost_peers(Duration::from_millis(500))
                .is_empty()
        );
        let follower = cluster.follower().await;
        cluster.stop_node(follower).await;
        let id = follower as u64 + 1;
        assert!(lost_when(&cluster, leader, |l| l == [id]).await);
        cluster.start_node(follower).await;
        assert!(lost_when(&cluster, leader, <[u64]>::is_empty).await);
        cluster.stop().await;
    }

    #[actix_web::test]
    async fn a_peer_whose_raft_answers_with_an_error_is_not_lost() {
        let cluster = super::testing::Cluster::start(3, 3).await;
        let leader = cluster.leader().await;
        let follower = cluster.follower().await;
        let follower = if follower == leader { 2 } else { follower };
        assert!(
            cluster.nodes[leader]
                .lost_peers(Duration::from_millis(500))
                .is_empty()
        );
        cluster.nodes[follower].raft().shutdown().await.unwrap();
        tokio::time::sleep(Duration::from_millis(1500)).await;
        assert!(
            cluster.nodes[leader]
                .lost_peers(Duration::from_millis(500))
                .is_empty()
        );
        cluster.stop().await;
    }

    #[actix_web::test]
    async fn a_slow_but_answering_owner_keeps_its_goal() {
        let cluster = super::testing::Cluster::start(3, 3).await;
        let leader = cluster.leader().await;
        let owner = if leader == 0 { 2 } else { 1 };
        let id = create_and_assign(&cluster, 400, owner).await;
        cluster
            .slow
            .node
            .store(owner, std::sync::atomic::Ordering::SeqCst);
        cluster
            .slow
            .millis
            .store(1800, std::sync::atomic::Ordering::SeqCst);
        cluster.assign();
        tokio::time::sleep(Duration::from_millis(3000)).await;
        for goals in &cluster.goals {
            assert_eq!(status_of(goals, id).await.unwrap().node, Some(owner));
        }
        cluster.stop().await;
    }

    #[actix_web::test]
    async fn a_plan_submitted_by_a_node_that_is_not_the_leader_commits_nothing() {
        let cluster = super::testing::Cluster::start(3, 3).await;
        let goals = cluster.goals[cluster.follower().await].clone();
        let id = off(move || goals.create(spec(20)).unwrap()).await;
        wait_everywhere(&cluster, id, |_| true).await;
        let plan = |node| {
            vec![Command::Assign {
                task: id,
                node,
                expect_token: 0,
            }]
        };
        let follower = cluster.nodes[cluster.follower().await].clone();
        super::assign::submit(&follower, plan(2)).await;
        tokio::time::sleep(Duration::from_millis(500)).await;
        for goals in &cluster.goals {
            assert_eq!(status_of(goals, id).await.unwrap().node, None);
        }
        let leader = cluster.nodes[cluster.leader().await].clone();
        super::assign::submit(&leader, plan(2)).await;
        wait_everywhere(&cluster, id, |s| s.node == Some(2)).await;
        cluster.stop().await;
    }

    #[actix_web::test]
    async fn a_goal_whose_owner_stops_is_reassigned_and_finishes_with_a_single_node_output() {
        let mut cluster = super::testing::Cluster::start(3, 3).await;
        cluster.work();
        cluster.leader().await;
        let owner = 2;
        let id = create_and_assign(&cluster, 400, owner).await;
        wait_everywhere(&cluster, id, |s| {
            s.output.as_deref().is_some_and(|o| !o.is_empty())
        })
        .await;
        cluster.assign();
        cluster.stop_node(owner as usize - 1).await;
        let live = (0..3).filter(|&i| i != owner as usize - 1);
        let deadline = std::time::Instant::now() + Duration::from_secs(10);
        for i in live.clone() {
            loop {
                let node = status_of(&cluster.goals[i], id).await.unwrap().node;
                if node.is_some_and(|n| n != owner) {
                    break;
                }
                assert!(std::time::Instant::now() < deadline, "never reassigned");
                tokio::time::sleep(Duration::from_millis(50)).await;
            }
        }
        let deadline = std::time::Instant::now() + Duration::from_secs(15);
        let expected = single_node_output(400);
        for i in live {
            loop {
                let status = status_of(&cluster.goals[i], id).await.unwrap();
                if status.status == "done" {
                    assert_eq!(status.output.as_deref(), Some(expected.as_str()));
                    break;
                }
                assert!(std::time::Instant::now() < deadline, "never finished");
                tokio::time::sleep(Duration::from_millis(50)).await;
            }
        }
        cluster.stop().await;
    }
}
