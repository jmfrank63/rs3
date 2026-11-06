//! Generation goals: durable creation, batched commits, status and recovery.
//!
//! The journal stream of a goal is the source of truth for its progress and end.
//! The `goals` table holds only what the stream cannot: priority and budget. It
//! is written in the same transaction as the stream's start record, so a goal
//! exists completely or not at all. A goal's id is its stream id.
use std::cmp::Reverse;
use std::collections::{BTreeMap, HashMap};
use std::num::{NonZeroU32, NonZeroUsize};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Condvar, Mutex, MutexGuard, OnceLock, PoisonError};
use std::time::Duration;

use redb::{Database, ReadableDatabase, ReadableTable, TableDefinition};

use crate::journal::{Journal, JournalError, Record, ScriptFailure, storage};
use crate::js::ExecError;
use crate::js::executor::JsExecutor;
use crate::js::oplog::{LogStop, OpLog};
use crate::lease::{self, Fence};
use crate::model::{FakeModel, Model};
use crate::raft::Node;
use crate::store::{Command, GoalEvent, Response, Store};

const GOALS: TableDefinition<u64, (i64, u64)> = TableDefinition::new("goals");

/// Tokens committed per transaction.
const BATCH: u64 = 8;

/// What a client asks for.
pub struct GoalSpec {
    pub priority: i64,
    pub budget: u64,
    pub seed: u64,
    pub prompt: String,
}

/// A goal that is queued or running.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Goal {
    pub id: u64,
    pub priority: i64,
    pub budget: u64,
}

/// What a client sees of a goal. The output holds committed tokens only.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GoalStatus {
    pub status: &'static str,
    pub output: Option<String>,
    pub error: Option<&'static str>,
    /// The node the goal is assigned to; `None` on a single node or while unassigned.
    pub node: Option<u64>,
}

/// How long a worker waits before it tries a cluster command again.
const RETRY_PAUSE: Duration = Duration::from_millis(100);

/// The status and error of a goal that has no durable result.
fn flags(
    cancelled: bool,
    running: bool,
    failed: bool,
    done: bool,
    progress: bool,
) -> (&'static str, Option<&'static str>) {
    match (cancelled, done, failed, running, progress) {
        (true, ..) => ("cancelled", None),
        (_, true, ..) => ("done", None),
        (_, _, true, ..) => ("failed", Some("storage_failure")),
        (_, _, _, true, _) => ("running", None),
        (.., true) => ("paused", None),
        _ => ("queued", None),
    }
}

#[derive(Default)]
struct Sched {
    queue: Vec<Goal>,
    /// Running goals and their priorities.
    running: HashMap<u64, i64>,
    /// The worker count; 0 until `start_workers`, which then counts as unlimited.
    workers: usize,
    failures: HashMap<u64, String>,
}

struct Inner {
    db: Arc<Database>,
    journal: Journal,
    sched: Mutex<Sched>,
    /// Signalled when a goal is queued.
    wake: Condvar,
    /// Seconds one token takes.
    token_secs: f64,
    /// What script goals run against; unset until `enable_scripts`.
    scripts: OnceLock<(Store, JsExecutor)>,
    /// This node's id in a cluster; `None` on a single node.
    me: Option<u64>,
    /// The Raft node goal commands go through; unset until `connect`.
    cluster: OnceLock<Arc<Node>>,
    /// Tokens this node has generated, including any it generated again.
    generated: AtomicU64,
}

/// Add the stream of goal `start` and row in `txn` under the next free id. The stream is written
/// first, so a refusal leaves no row behind.
pub(crate) fn create_in(
    txn: &redb::WriteTransaction,
    journal: &Journal,
    priority: i64,
    budget: u64,
    start: &Record,
) -> Result<u64, JournalError> {
    let mut table = txn.open_table(GOALS).map_err(storage)?;
    let id = table
        .last()
        .map_err(storage)?
        .map_or(1, |(key, _)| key.value() + 1);
    journal.append_in(txn, id, 0, start)?;
    table.insert(id, (priority, budget)).map_err(storage)?;
    Ok(id)
}

/// End goal `id` in `txn`, taking over its lease so a running worker is fenced out. Returns
/// false, writing nothing, when the goal has ended, is corrupt or does not exist.
pub(crate) fn cancel_in(
    txn: &redb::WriteTransaction,
    journal: &Journal,
    id: u64,
) -> Result<bool, JournalError> {
    // The transaction holds the writer, so the read sees the latest committed state.
    let seq = match journal.read(id) {
        Ok(records) if records.last().is_some_and(|last| !last.ends_stream()) => {
            records.len() as u64
        }
        Ok(_) | Err(JournalError::Corrupt { .. }) => return Ok(false),
        Err(e) => return Err(e),
    };
    lease::claim_in(txn, id, None)?;
    journal.append_in(txn, id, seq, &Record::Cancelled)?;
    Ok(true)
}

/// Shared handle to the goals of one database.
#[derive(Clone)]
pub struct Goals(Arc<Inner>);

impl Goals {
    /// Use `db` for goals, queueing every goal that has not ended. A goal whose
    /// stream fails verification is logged once and left out.
    /// Workers emit `rate` tokens per second.
    pub fn open(db: Arc<Database>, rate: NonZeroU32) -> Result<Self, JournalError> {
        Self::build(db, rate, None)
    }

    /// Like `open` for node `me` of a cluster: only goals assigned to `me` are queued, and goal
    /// commands go through Raft once `connect` has run. `store` tells this node's goals of the
    /// goals assigned to it and cancelled.
    pub fn open_in_cluster(
        db: Arc<Database>,
        rate: NonZeroU32,
        me: u64,
        store: &Store,
    ) -> Result<Self, JournalError> {
        let goals = Self::build(db, rate, Some(me))?;
        let weak = Arc::downgrade(&goals.0);
        store.on_goal_event(move |event| {
            if let Some(inner) = weak.upgrade() {
                Self(inner).on_event(&event);
            }
        });
        Ok(goals)
    }

    /// The `Assign` commands that hand every live goal without a live owner to the live member
    /// with the fewest goals, counting its own picks; ties go to the lowest id. A goal whose owner
    /// is not in `live` is moved with its current token, so a replayed plan changes nothing. Ended
    /// and corrupt goals are ignored.
    pub fn plan_assignments(&self, live: &[u64]) -> Result<Vec<Command>, JournalError> {
        let mut load: BTreeMap<u64, usize> = live.iter().map(|&m| (m, 0)).collect();
        let mut orphans = Vec::new();
        for task in self.0.journal.live_streams()? {
            let owner = lease::owner(&self.0.db, task)?;
            match owner.and_then(|node| load.get_mut(&node)) {
                Some(n) => *n += 1,
                None => orphans.push(task),
            }
        }
        let mut plan = Vec::new();
        for task in orphans {
            let Some((&node, _)) = load.iter().min_by_key(|&(&id, &n)| (n, id)) else {
                break;
            };
            load.insert(node, load[&node] + 1);
            plan.push(Command::Assign {
                task,
                node,
                expect_token: lease::token(&self.0.db, task)?,
            });
        }
        Ok(plan)
    }

    /// How many tokens this node has generated so far, tokens it had to generate again included.
    pub fn generated_tokens(&self) -> u64 {
        self.0.generated.load(Ordering::Relaxed)
    }

    /// Send goal commands through `node`. Until then they answer `Unavailable`.
    pub fn connect(&self, node: Arc<Node>) {
        let _ = self.0.cluster.set(node);
    }

    fn build(db: Arc<Database>, rate: NonZeroU32, me: Option<u64>) -> Result<Self, JournalError> {
        let journal = Journal::new(db.clone())?;
        let txn = db.begin_write().map_err(storage)?;
        txn.open_table(GOALS).map_err(storage)?;
        txn.commit().map_err(storage)?;
        let goals = Self(Arc::new(Inner {
            db,
            journal,
            sched: Mutex::default(),
            wake: Condvar::new(),
            token_secs: 1.0 / f64::from(rate.get()),
            scripts: OnceLock::new(),
            me,
            cluster: OnceLock::new(),
            generated: AtomicU64::new(0),
        }));
        for goal in goals.all()? {
            match goals.0.journal.read(goal.id) {
                Ok(records) if records.last().is_some_and(|last| !last.ends_stream()) => {
                    if goals.mine(goal.id)? {
                        goals.sched().queue.push(goal);
                    }
                }
                Ok(_) => {}
                Err(e @ JournalError::Corrupt { .. }) => eprintln!("error: goal {}: {e}", goal.id),
                Err(e) => return Err(e),
            }
        }
        Ok(goals)
    }

    fn all(&self) -> Result<Vec<Goal>, JournalError> {
        let txn = self.0.db.begin_read().map_err(storage)?;
        let table = txn.open_table(GOALS).map_err(storage)?;
        let mut goals = Vec::new();
        for entry in table.iter().map_err(storage)? {
            let (id, row) = entry.map_err(storage)?;
            goals.push(Goal {
                id: id.value(),
                priority: row.value().0,
                budget: row.value().1,
            });
        }
        Ok(goals)
    }

    fn sched(&self) -> MutexGuard<'_, Sched> {
        self.0.sched.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// Whether this node runs goal `id`: always on a single node, else when it is assigned here.
    fn mine(&self, id: u64) -> Result<bool, JournalError> {
        Ok(self.0.me.is_none() || lease::owner(&self.0.db, id)? == self.0.me)
    }

    /// Queue goal `id` unless it is queued or running. Never proposes: it runs on the apply thread.
    fn on_event(&self, event: &GoalEvent) {
        match *event {
            GoalEvent::Assigned { task, node } if Some(node) == self.0.me => {
                let goal = self
                    .all()
                    .ok()
                    .and_then(|all| all.into_iter().find(|g| g.id == task));
                let mut sched = self.sched();
                if let Some(goal) = goal
                    && !sched.running.contains_key(&task)
                    && !sched.queue.iter().any(|g| g.id == task)
                {
                    sched.queue.push(goal);
                    self.0.wake.notify_one();
                }
            }
            GoalEvent::Cancelled { task } => self.sched().queue.retain(|g| g.id != task),
            GoalEvent::Assigned { .. } => {}
        }
    }

    /// Run `cmd` through Raft and wait for this node to apply it. A cluster that cannot answer
    /// now is `Unavailable`, and so is a node that has not been connected.
    fn propose(&self, cmd: Command) -> Result<Response, JournalError> {
        let node = self
            .0
            .cluster
            .get()
            .ok_or_else(|| JournalError::Unavailable("node is not connected".into()))?;
        match node.propose_blocking(cmd) {
            Ok(Ok(response)) => Ok(response),
            Ok(Err(e)) => Err(JournalError::Storage(e)),
            Err(e) => Err(JournalError::Unavailable(e.to_string())),
        }
    }

    /// Run script goals against `store` on `js`. Call it once, before the
    /// workers start; a script goal that runs earlier fails in memory only.
    pub fn enable_scripts(&self, store: Store, js: JsExecutor) {
        let _ = self.0.scripts.set((store, js));
    }

    /// Create a goal and queue it. The caller validates the budget.
    pub fn create(&self, spec: GoalSpec) -> Result<u64, JournalError> {
        let start = Record::InferenceStart {
            engine: "fake".into(),
            model_hash: "fake".into(),
            tokenizer_hash: "fake".into(),
            sampler: serde_json::json!({ "seed": spec.seed }).to_string(),
            rng_state: FakeModel.start_state(spec.seed, &spec.prompt),
            prompt_tokens: spec.prompt.bytes().map(u32::from).collect(),
        };
        self.insert_goal(spec.priority, spec.budget, &start)
    }

    /// Create a goal that runs `source` as the script `name`, and queue it.
    pub fn create_script(
        &self,
        priority: i64,
        name: String,
        source: String,
    ) -> Result<u64, JournalError> {
        self.insert_goal(priority, 0, &Record::ScriptStart { name, source })
    }

    fn insert_goal(&self, priority: i64, budget: u64, start: &Record) -> Result<u64, JournalError> {
        if self.0.me.is_some() {
            // The leader assigns it; nothing runs until then.
            return match self.propose(Command::CreateGoal {
                priority,
                budget,
                start: start.clone(),
            })? {
                Response::GoalCreated(id) => Ok(id),
                other => Err(JournalError::Storage(format!(
                    "unexpected answer: {other:?}"
                ))),
            };
        }
        let txn = self.0.db.begin_write().map_err(storage)?;
        let id = create_in(&txn, &self.0.journal, priority, budget, start)?;
        txn.commit().map_err(storage)?;
        self.sched().queue.push(Goal {
            id,
            priority,
            budget,
        });
        self.0.wake.notify_one();
        self.preempt();
        Ok(id)
    }

    /// The status of goal `id`, or `None` when there is no such goal.
    pub fn status(&self, id: u64) -> Result<Option<GoalStatus>, JournalError> {
        // Read the flags before the stream: the other order can report a goal
        // that finished in between as queued.
        let node = if self.0.me.is_some() {
            lease::owner(&self.0.db, id)?
        } else {
            None
        };
        let (running, failed) = {
            let sched = self.sched();
            (
                // Assigned elsewhere and not ended: running there.
                sched.running.contains_key(&id) || node.is_some_and(|n| Some(n) != self.0.me),
                sched.failures.contains_key(&id),
            )
        };
        let records = match self.0.journal.read(id) {
            Ok(records) => records,
            Err(JournalError::Corrupt { .. }) => {
                return Ok(Some(GoalStatus {
                    status: "failed",
                    output: None,
                    error: Some("journal_corrupt"),
                    node,
                }));
            }
            Err(e) => return Err(e),
        };
        let progress = records.len() > 1;
        let cancelled = matches!(records.last(), Some(Record::Cancelled));
        let (status, error, output) = match records.last() {
            Some(Record::ScriptEnd { outcome: Ok(value) }) => ("done", None, Some(value.clone())),
            Some(Record::ScriptEnd { outcome: Err(f) }) => ("failed", Some(f.code()), None),
            _ if matches!(records.first(), Some(Record::ScriptStart { .. })) => {
                let (status, error) = flags(cancelled, running, failed, false, progress);
                (status, error, None)
            }
            _ => {
                let Some(state) = Journal::rebuild(id, records)? else {
                    return Ok(None);
                };
                let (status, error) =
                    flags(cancelled, running, failed, state.ended.is_some(), progress);
                (status, error, Some(FakeModel.render(&state.tokens)))
            }
        };
        Ok(Some(GoalStatus {
            status,
            output,
            error,
            node,
        }))
    }

    /// End goal `id` durably. The cancel takes over the goal's lease and writes
    /// the end record in one transaction, so a worker cannot claim in between
    /// and its next commit is refused. A goal that has already ended, or whose
    /// stream is corrupt, is left as it is. Returns the goal's status.
    pub fn cancel(&self, id: u64) -> Result<Option<GoalStatus>, JournalError> {
        if self.0.me.is_some() {
            self.propose(Command::CancelGoal { id })?;
            return self.status(id);
        }
        let txn = self.0.db.begin_write().map_err(storage)?;
        if cancel_in(&txn, &self.0.journal, id)? {
            txn.commit().map_err(storage)?;
            self.sched().queue.retain(|g| g.id != id);
        }
        self.status(id)
    }

    /// Commit the next batch of `goal` in one transaction, after the time the
    /// model needs for it, under `fence`. Returns whether the goal has ended;
    /// a stale fence is refused as `Fenced` and commits nothing.
    pub fn step(&self, goal: &Goal, fence: &Fence) -> Result<bool, JournalError> {
        let state = self
            .0
            .journal
            .restore(goal.id)?
            .ok_or_else(|| JournalError::Storage(format!("goal {} has no stream", goal.id)))?;
        if state.ended.is_some() {
            return Ok(true);
        }
        let done = state.tokens.len() as u64;
        let count = BATCH.min(goal.budget.saturating_sub(done));
        let mut rng = state.rng_state;
        let mut tokens = Vec::new();
        for _ in 0..count {
            let (token, next) = FakeModel.step(&rng);
            tokens.push(token);
            rng = next;
        }
        self.0.generated.fetch_add(count, Ordering::Relaxed);
        let finished = done + count >= goal.budget;
        std::thread::sleep(Duration::from_secs_f64(count as f64 * self.0.token_secs));
        let record = Record::Tokens {
            tokens,
            rng_state: rng,
        };
        if self.0.me.is_some() {
            let mut records = vec![record];
            if finished {
                records.push(Record::InferenceEnd {
                    reason: "budget".into(),
                });
            }
            return self.append(goal.id, fence, state.next_seq, records);
        }
        let txn = self.0.db.begin_write().map_err(storage)?;
        lease::check_in(&txn, fence)?;
        self.0
            .journal
            .append_in(&txn, goal.id, state.next_seq, &record)?;
        if finished {
            let end = Record::InferenceEnd {
                reason: "budget".into(),
            };
            self.0
                .journal
                .append_in(&txn, goal.id, state.next_seq + 1, &end)?;
        }
        txn.commit().map_err(storage)?;
        Ok(finished)
    }

    /// Append `records` through Raft under `fence`; returns whether the goal had ended or now has.
    fn append(
        &self,
        id: u64,
        fence: &Fence,
        seq: u64,
        records: Vec<Record>,
    ) -> Result<bool, JournalError> {
        let ends = records.last().is_some_and(Record::ends_stream);
        match self.propose(Command::Append {
            task: id,
            token: fence.token,
            seq,
            records,
        })? {
            Response::Appended => Ok(ends),
            Response::Fenced {
                task,
                held,
                current,
            } => Err(JournalError::Fenced {
                task,
                held,
                current,
            }),
            // This node's copy was behind; the caller reads again and retries.
            Response::Refused(code) => Err(JournalError::Unavailable(code)),
            other => Err(JournalError::Storage(format!(
                "unexpected answer: {other:?}"
            ))),
        }
    }

    /// When every worker is busy and a queued goal outranks the lowest running
    /// one, take over that goal's lease. Its worker's next commit is then
    /// refused as fenced, so it stops, and `run` queues the goal again. A
    /// failed claim is logged only: the queued goal is already durable.
    fn preempt(&self) {
        let victim = {
            let sched = self.sched();
            let queued = sched.queue.iter().map(|g| g.priority).max();
            let lowest = sched.running.iter().min_by_key(|&(_, &p)| p);
            match (queued, lowest) {
                (Some(q), Some((&id, &p))) if sched.running.len() >= sched.workers && q > p => {
                    Some(id)
                }
                _ => None,
            }
        };
        if let Some(id) = victim
            && let Err(e) = lease::claim(&self.0.db, id)
        {
            eprintln!("error: goal {id}: {e}");
        }
    }

    fn take(sched: &mut Sched) -> Option<Goal> {
        let next = (0..sched.queue.len())
            .max_by_key(|&i| (sched.queue[i].priority, Reverse(sched.queue[i].id)))?;
        let goal = sched.queue.remove(next);
        sched.running.insert(goal.id, goal.priority);
        Some(goal)
    }

    /// Take the next queued goal and mark it running. Pair it with `run`.
    pub fn next_goal(&self) -> Option<Goal> {
        Self::take(&mut self.sched())
    }

    /// Start `workers` threads that run queued goals for the life of the
    /// process. There is no shutdown: the threads end with the process.
    pub fn start_workers(&self, workers: NonZeroUsize) {
        self.sched().workers = workers.get();
        for _ in 0..workers.get() {
            let goals = self.clone();
            std::thread::spawn(move || {
                loop {
                    let mut sched = goals.sched();
                    let goal = loop {
                        if let Some(goal) = Self::take(&mut sched) {
                            break goal;
                        }
                        sched = goals
                            .0
                            .wake
                            .wait(sched)
                            .unwrap_or_else(PoisonError::into_inner);
                    };
                    drop(sched);
                    goals.run(goal);
                }
            });
        }
    }

    /// Run the script of `goal` against its op log, then end its stream with
    /// the outcome. A fenced stop ends nothing: the new holder carries on. A
    /// host failure is a storage error, so the goal resumes on the next start.
    fn run_script(
        &self,
        goal: &Goal,
        fence: Fence,
        name: &str,
        source: &str,
        records: &[Record],
    ) -> Result<(), JournalError> {
        let Some((store, js)) = self.0.scripts.get() else {
            return Err(JournalError::Storage("scripts are not enabled".into()));
        };
        let mut log = OpLog::new(self.0.db.clone(), self.0.journal.clone(), fence, records);
        if self.0.me.is_some() {
            let goals = self.clone();
            log = log.in_cluster(Arc::new(move |cmd| goals.propose(cmd)));
        }
        let log = Arc::new(log);
        let result = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .map_err(|e| JournalError::Storage(e.to_string()))?
            .block_on(js.execute_logged(
                store.clone(),
                name.to_owned(),
                source.to_owned(),
                log.clone(),
            ));
        let outcome = match (log.stopped(), result) {
            (Some(LogStop::Fenced), _) => return Ok(()),
            (Some(LogStop::Diverged), _) => Err(ScriptFailure::ReplayDiverged),
            (Some(LogStop::Unavailable), _) => {
                return Err(JournalError::Unavailable("op not committed".into()));
            }
            (None, Err(ExecError::Internal(e))) => return Err(JournalError::Storage(e)),
            (None, Err(ExecError::Unavailable)) => {
                return Err(JournalError::Storage(ExecError::Unavailable.to_string()));
            }
            (None, Err(ExecError::Timeout { .. })) => Err(ScriptFailure::JsTimeout),
            (None, Ok(_) | Err(ExecError::Script { .. } | ExecError::DepthLimit { .. }))
                if !log.fully_replayed() =>
            {
                eprintln!("error: goal {}: the script does not match its log", goal.id);
                Err(ScriptFailure::ReplayDiverged)
            }
            (None, Ok(value)) => Ok(value),
            (None, Err(_)) => Err(ScriptFailure::JsError),
        };
        log.end(outcome)
    }

    /// Run `f` until the cluster can answer: wait out `Unavailable` and try again. Before each
    /// retry this node waits until it has applied everything committed, so `f` does not redo a
    /// write whose answer was lost. While there is no quorum, it keeps waiting.
    fn retry<T>(&self, mut f: impl FnMut() -> Result<T, JournalError>) -> Result<T, JournalError> {
        loop {
            match f() {
                Err(JournalError::Unavailable(_)) => {
                    std::thread::sleep(RETRY_PAUSE);
                    if let Some(node) = self.0.cluster.get() {
                        while node.handle().block_on(node.read_barrier()).is_err() {
                            std::thread::sleep(RETRY_PAUSE);
                        }
                    }
                }
                done => return done,
            }
        }
    }

    /// Run `goal` on node `me`: claim it through Raft, then step it until it ends. A cluster that
    /// cannot answer, or a copy that lags, is waited out and retried from the committed state.
    fn drive_cluster(&self, goal: &Goal, me: u64) -> Result<(), JournalError> {
        let records = self.0.journal.read(goal.id)?;
        if records.last().is_some_and(Record::ends_stream) {
            return Ok(());
        }
        let claim = Command::Claim {
            task: goal.id,
            node: me,
        };
        let fence = match self.retry(|| self.propose(claim.clone()))? {
            Response::Claimed { token } => Fence {
                task: goal.id,
                token,
            },
            Response::Refused(_) => return Ok(()),
            other => {
                return Err(JournalError::Storage(format!(
                    "unexpected answer: {other:?}"
                )));
            }
        };
        if matches!(records.first(), Some(Record::ScriptStart { .. })) {
            // Each try replays the committed log, which a failed commit may have extended.
            return self.retry(|| {
                let records = self.0.journal.read(goal.id)?;
                match records.first() {
                    Some(Record::ScriptStart { name, source }) => {
                        self.run_script(goal, fence.clone(), name, source, &records)
                    }
                    _ => Ok(()),
                }
            });
        }
        while !self.retry(|| self.step(goal, &fence))? {}
        Ok(())
    }

    fn drive(&self, goal: &Goal) -> Result<(), JournalError> {
        if let Some(me) = self.0.me {
            return self.drive_cluster(goal, me);
        }
        let fence = lease::claim(&self.0.db, goal.id)?;
        // A takeover between `take` and this claim was just overwritten.
        self.preempt();
        let records = self.0.journal.read(goal.id)?;
        if records.last().is_some_and(Record::ends_stream) {
            return Ok(());
        }
        if let Some(Record::ScriptStart { name, source }) = records.first() {
            return self.run_script(goal, fence, name, source, &records);
        }
        while !self.step(goal, &fence)? {}
        Ok(())
    }

    /// Claim `goal`, then step it until it ends. A worker whose lease was taken
    /// over stops quietly, and the goal is queued again, paused. A storage
    /// failure is logged once and kept in memory only, so the goal resumes on
    /// the next start.
    pub fn run(&self, goal: Goal) {
        let requeue = match self.drive(&goal) {
            Err(e) if !matches!(e, JournalError::Fenced { .. }) => Err(e),
            _ => self.0.journal.read(goal.id).and_then(|records| {
                // Queue it again only while it is live and still assigned here.
                if records.last().is_some_and(|last| !last.ends_stream()) {
                    self.mine(goal.id)
                } else {
                    Ok(false)
                }
            }),
        };
        let mut sched = self.sched();
        sched.running.remove(&goal.id);
        match requeue {
            Ok(true) => {
                sched.queue.push(goal);
                self.0.wake.notify_one();
            }
            Ok(false) => {}
            Err(e) => {
                eprintln!("error: goal {}: {e}", goal.id);
                sched.failures.insert(goal.id, e.to_string());
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::storage::{TempDir, open};
    use std::path::Path;

    const FAST: NonZeroU32 = NonZeroU32::new(1_000_000).unwrap();
    const HEADS: TableDefinition<u64, &[u8]> = TableDefinition::new("journal_heads");
    const RECORDS: TableDefinition<(u64, u64), &[u8]> = TableDefinition::new("journal_records");

    fn spec(budget: u64, seed: u64) -> GoalSpec {
        GoalSpec {
            priority: 0,
            budget,
            seed,
            prompt: "hello".into(),
        }
    }

    fn assign_to(db: &Database, task: u64, node: u64) {
        let txn = db.begin_write().unwrap();
        lease::claim_in(&txn, task, Some(node)).unwrap();
        txn.commit().unwrap();
    }

    fn picks(plan: Vec<Command>) -> Vec<(u64, u64)> {
        plan.into_iter()
            .map(|c| match c {
                Command::Assign {
                    task,
                    node,
                    expect_token: 0,
                } => (task, node),
                other => panic!("{other:?}"),
            })
            .collect()
    }

    #[test]
    fn plan_moves_goals_of_a_lost_node_to_the_least_loaded_live_node() {
        let dir = TempDir::new();
        let db = open(&dir.path().join("db")).unwrap();
        let goals = Goals::open(db.clone(), FAST).unwrap();
        for seed in 0..4 {
            goals.create(spec(5, seed)).unwrap();
        }
        for (task, node) in [(1, 1), (2, 2), (3, 1), (4, 2)] {
            assign_to(&db, task, node);
        }
        assert!(goals.plan_assignments(&[1, 2, 3]).unwrap().is_empty());
        let plan = goals.plan_assignments(&[2, 3]).unwrap();
        let moves: Vec<_> = plan
            .iter()
            .map(|c| match c {
                Command::Assign {
                    task,
                    node,
                    expect_token,
                } => (*task, *node, *expect_token),
                other => panic!("{other:?}"),
            })
            .collect();
        assert_eq!(moves, vec![(1, 3, 1), (3, 3, 1)]);
        // Once the plan is applied nothing is left to move.
        assign_to(&db, 1, 3);
        assign_to(&db, 3, 3);
        assert!(goals.plan_assignments(&[2, 3]).unwrap().is_empty());
    }

    #[test]
    fn plan_spreads_unassigned_goals_one_per_member_lowest_id_first() {
        let dir = TempDir::new();
        let goals = goals_at(&dir.path().join("db"));
        for seed in 0..3 {
            goals.create(spec(5, seed)).unwrap();
        }
        let plan = picks(goals.plan_assignments(&[3, 1, 2]).unwrap());
        assert_eq!(plan, vec![(1, 1), (2, 2), (3, 3)]);
        assert_eq!(goals.plan_assignments(&[]).unwrap().len(), 0);
    }

    #[test]
    fn plan_respects_existing_load_and_ignores_ended_goals() {
        let dir = TempDir::new();
        let db = open(&dir.path().join("db")).unwrap();
        let goals = Goals::open(db.clone(), FAST).unwrap();
        for seed in 0..5 {
            goals.create(spec(5, seed)).unwrap();
        }
        assign_to(&db, 1, 1);
        assign_to(&db, 2, 1);
        assign_to(&db, 3, 9); // node 9 is live with one goal
        goals.cancel(5).unwrap();
        // Goal 4 is the only live unassigned goal; node 2 carries less than node 1.
        let plan = picks(goals.plan_assignments(&[1, 2, 9]).unwrap());
        assert_eq!(plan, vec![(4, 2)]);
    }

    fn goals_at(path: &Path) -> Goals {
        Goals::open(open(path).unwrap(), FAST).unwrap()
    }

    /// Run `f` on its own thread and fail the test, never hang it, if it takes
    /// over 20 s. A stuck thread dies with the test process.
    fn within_deadline<T: Send + 'static>(f: impl FnOnce() -> T + Send + 'static) -> T {
        within(20, f)
    }

    /// Like `within_deadline`, with a limit of `secs`.
    fn within<T: Send + 'static>(secs: u64, f: impl FnOnce() -> T + Send + 'static) -> T {
        let (tx, rx) = std::sync::mpsc::channel();
        let thread = std::thread::spawn(move || {
            let _ = tx.send(f());
        });
        match rx.recv_timeout(Duration::from_secs(secs)) {
            Ok(value) => value,
            Err(std::sync::mpsc::RecvTimeoutError::Timeout) => panic!("stuck for over {secs} s"),
            Err(_) => std::panic::resume_unwind(thread.join().unwrap_err()),
        }
    }

    fn run(goals: &Goals, goal: Goal) {
        let goals = goals.clone();
        within_deadline(move || goals.run(goal));
    }

    fn step(goals: &Goals, goal: &Goal, fence: &Fence) -> Result<bool, JournalError> {
        let (goals, goal, fence) = (goals.clone(), goal.clone(), fence.clone());
        within_deadline(move || goals.step(&goal, &fence))
    }

    fn drain(goals: &Goals) {
        let goals = goals.clone();
        within_deadline(move || {
            while let Some(goal) = goals.next_goal() {
                goals.run(goal);
            }
        });
    }

    fn status(goals: &Goals, id: u64) -> GoalStatus {
        goals.status(id).unwrap().unwrap()
    }

    fn output(goals: &Goals, id: u64) -> String {
        status(goals, id).output.unwrap()
    }

    fn token_count(text: &str) -> usize {
        text.split_whitespace().count()
    }

    fn tamper_record(db: &Database, key: (u64, u64)) {
        let txn = db.begin_write().unwrap();
        {
            let mut table = txn.open_table(RECORDS).unwrap();
            let mut bytes = table.get(key).unwrap().unwrap().value().to_vec();
            let last = bytes.len() - 1;
            bytes[last] ^= 1;
            table.insert(key, bytes.as_slice()).unwrap();
        }
        txn.commit().unwrap();
    }

    fn flip_head(db: &Database, id: u64) {
        let txn = db.begin_write().unwrap();
        {
            let mut table = txn.open_table(HEADS).unwrap();
            let mut bytes = table.get(id).unwrap().unwrap().value().to_vec();
            bytes[0] ^= 1;
            table.insert(id, bytes.as_slice()).unwrap();
        }
        txn.commit().unwrap();
    }

    fn head_bytes(db: &Database, id: u64) -> Vec<u8> {
        let txn = db.begin_read().unwrap();
        let table = txn.open_table(HEADS).unwrap();
        table.get(id).unwrap().unwrap().value().to_vec()
    }

    fn set_head(db: &Database, id: u64, bytes: &[u8]) {
        let txn = db.begin_write().unwrap();
        txn.open_table(HEADS).unwrap().insert(id, bytes).unwrap();
        txn.commit().unwrap();
    }

    fn undisturbed(budget: u64, seed: u64) -> String {
        let dir = TempDir::new();
        let goals = goals_at(&dir.path().join("db"));
        let id = goals.create(spec(budget, seed)).unwrap();
        drain(&goals);
        output(&goals, id)
    }

    #[test]
    fn a_created_goal_survives_reopen_and_is_queued_again() {
        let dir = TempDir::new();
        let path = dir.path().join("db");
        let id = goals_at(&path).create(spec(5, 1)).unwrap();
        let goals = goals_at(&path);
        assert_eq!(
            status(&goals, id),
            GoalStatus {
                status: "queued",
                output: Some(String::new()),
                error: None,
                node: None
            }
        );
        assert_eq!(
            goals.next_goal(),
            Some(Goal {
                id,
                priority: 0,
                budget: 5
            })
        );
        assert_eq!(goals.next_goal(), None);
    }

    #[test]
    fn a_running_goal_reports_running() {
        let dir = TempDir::new();
        let goals = goals_at(&dir.path().join("db"));
        let id = goals.create(spec(5, 1)).unwrap();
        let _goal = goals.next_goal().unwrap();
        assert_eq!(status(&goals, id).status, "running");
    }

    #[test]
    fn a_cluster_node_queues_after_a_restart_only_the_goals_assigned_to_it() {
        let dir = TempDir::new();
        let db = open(&dir.path().join("db")).unwrap();
        let id = Goals::open(db.clone(), FAST)
            .unwrap()
            .create(spec(5, 1))
            .unwrap();
        let txn = db.begin_write().unwrap();
        lease::claim_in(&txn, id, Some(2)).unwrap();
        txn.commit().unwrap();
        let on = |node| Goals::open_in_cluster(db.clone(), FAST, node, &Store::new()).unwrap();
        assert_eq!(on(1).next_goal(), None);
        assert_eq!(on(2).next_goal().map(|g| g.id), Some(id));
    }

    #[test]
    fn ids_count_up_across_reopen() {
        let dir = TempDir::new();
        let path = dir.path().join("db");
        let goals = goals_at(&path);
        assert_eq!(goals.create(spec(1, 1)).unwrap(), 1);
        assert_eq!(goals.create(spec(1, 1)).unwrap(), 2);
        drop(goals);
        assert_eq!(goals_at(&path).create(spec(1, 1)).unwrap(), 3);
    }

    #[test]
    fn a_failed_create_leaves_no_goal_behind() {
        let dir = TempDir::new();
        let db = open(&dir.path().join("db")).unwrap();
        let goals = Goals::open(db.clone(), FAST).unwrap();
        let txn = db.begin_write().unwrap();
        txn.delete_table(HEADS).unwrap();
        txn.open_table(TableDefinition::<&str, &str>::new("journal_heads"))
            .unwrap();
        txn.commit().unwrap();
        assert!(goals.create(spec(5, 1)).is_err());
        let txn = db.begin_write().unwrap();
        txn.delete_table(TableDefinition::<&str, &str>::new("journal_heads"))
            .unwrap();
        txn.open_table(HEADS).unwrap();
        txn.commit().unwrap();
        assert_eq!(goals.create(spec(5, 1)).unwrap(), 1);
    }

    #[test]
    fn budgets_are_met_exactly_and_finished_goals_are_not_requeued() {
        let dir = TempDir::new();
        let path = dir.path().join("db");
        let goals = goals_at(&path);
        let budgets = [1, 8, 9, 20];
        let ids: Vec<u64> = budgets
            .iter()
            .map(|b| goals.create(spec(*b, 3)).unwrap())
            .collect();
        drain(&goals);
        for (id, budget) in ids.iter().zip(budgets) {
            let s = status(&goals, *id);
            assert_eq!(s.status, "done");
            assert_eq!(token_count(&s.output.unwrap()), budget as usize);
        }
        drop(goals);
        assert_eq!(goals_at(&path).next_goal(), None);
    }

    #[test]
    fn a_resumed_goal_matches_an_undisturbed_one() {
        let dir = TempDir::new();
        let path = dir.path().join("db");
        let db = open(&path).unwrap();
        let goals = Goals::open(db.clone(), FAST).unwrap();
        let id = goals.create(spec(20, 9)).unwrap();
        let goal = goals.next_goal().unwrap();
        let fence = lease::claim(&db, id).unwrap();
        assert!(!step(&goals, &goal, &fence).unwrap());
        let first = output(&goals, id);
        assert!(!step(&goals, &goal, &fence).unwrap());
        let second = output(&goals, id);
        drop((goals, db));
        let goals = goals_at(&path);
        drain(&goals);
        let last = output(&goals, id);
        assert!(second.starts_with(&first) && last.starts_with(&second));
        assert_eq!(token_count(&first), 8);
        assert_eq!(token_count(&second), 16);
        assert_eq!(last, undisturbed(20, 9));
    }

    #[test]
    fn a_worker_whose_lease_is_taken_over_stops_quietly() {
        let dir = TempDir::new();
        let db = open(&dir.path().join("db")).unwrap();
        let slow = NonZeroU32::new(80).unwrap();
        let goals = Goals::open(db.clone(), slow).unwrap();
        let id = goals.create(spec(800, 5)).unwrap();
        let goal = goals.next_goal().unwrap();
        let worker = {
            let goals = goals.clone();
            let goal = goal.clone();
            std::thread::spawn(move || goals.run(goal))
        };
        let deadline = std::time::Instant::now() + Duration::from_secs(10);
        while output(&goals, id).is_empty() {
            assert!(std::time::Instant::now() < deadline, "no batch committed");
            std::thread::sleep(Duration::from_millis(5));
        }
        lease::claim(&db, id).unwrap();
        let snapshot = output(&goals, id);
        within_deadline(move || worker.join().unwrap());
        let s = status(&goals, id);
        assert_eq!(s.output.as_deref(), Some(snapshot.as_str()));
        assert!(token_count(&snapshot) < 800);
        assert!(s.status != "failed" && s.status != "running");
        assert_eq!(s.error, None);
        let fence = lease::claim(&db, id).unwrap();
        let stepper = goals.clone();
        within_deadline(move || while !stepper.step(&goal, &fence).unwrap() {});
        assert_eq!(output(&goals, id), undisturbed(800, 5));
    }

    fn spec_at(priority: i64) -> GoalSpec {
        GoalSpec {
            priority,
            ..spec(1, 1)
        }
    }

    fn order(goals: &Goals) -> Vec<u64> {
        std::iter::from_fn(|| goals.next_goal().map(|g| g.id)).collect()
    }

    #[test]
    fn higher_priorities_come_first_and_ties_keep_creation_order() {
        let dir = TempDir::new();
        let path = dir.path().join("db");
        let goals = goals_at(&path);
        for p in [1, 5, 5, 3, -1] {
            goals.create(spec_at(p)).unwrap();
        }
        assert_eq!(order(&goals), [2, 3, 4, 1, 5]);
        drop(goals);
        assert_eq!(order(&goals_at(&path)), [2, 3, 4, 1, 5]);
    }

    #[test]
    fn a_script_goals_priority_counts() {
        let r = rig();
        let low = r.script("1");
        let high = r.goals.create_script(9, "s".into(), "2".into()).unwrap();
        assert_eq!(order(&r.goals), [high, low]);
        assert_eq!(order(&r.reopen().goals), [high, low]);
    }

    #[test]
    fn a_fenced_generation_goal_is_queued_again_and_paused() {
        let dir = TempDir::new();
        let db = open(&dir.path().join("db")).unwrap();
        let slow = NonZeroU32::new(80).unwrap();
        let goals = Goals::open(db.clone(), slow).unwrap();
        let id = goals.create(spec(800, 5)).unwrap();
        let goal = goals.next_goal().unwrap();
        let worker = {
            let goals = goals.clone();
            std::thread::spawn(move || goals.run(goal))
        };
        let deadline = std::time::Instant::now() + Duration::from_secs(10);
        while output(&goals, id).is_empty() {
            assert!(std::time::Instant::now() < deadline, "no batch committed");
            std::thread::sleep(Duration::from_millis(5));
        }
        lease::claim(&db, id).unwrap();
        within_deadline(move || worker.join().unwrap());
        assert_eq!(status(&goals, id).status, "paused");
        let goal = goals.next_goal().unwrap();
        assert_eq!(goal.id, id);
        assert_eq!(status(&goals, id).status, "running");
        run(&goals, goal);
        assert_eq!(status(&goals, id).status, "done");
        assert_eq!(output(&goals, id), undisturbed(800, 5));
        assert_eq!(goals.next_goal(), None);
    }

    // ---- preemption ----

    fn spec_p(priority: i64, seed: u64) -> GoalSpec {
        GoalSpec {
            priority,
            ..spec(20, seed)
        }
    }

    /// Create a goal at `low`, take it and commit one batch under its claim,
    /// then create a goal at `high`. The takeover lands before `run(low)`.
    fn low_then_high(low: i64, high: i64) -> (Goals, Goal, u64) {
        let dir = TempDir::new();
        let db = open(&dir.path().join("db")).unwrap();
        std::mem::forget(dir);
        let goals = Goals::open(db.clone(), FAST).unwrap();
        goals.create(spec_p(low, 1)).unwrap();
        let goal = goals.next_goal().unwrap();
        let fence = lease::claim(&db, goal.id).unwrap();
        assert!(!step(&goals, &goal, &fence).unwrap());
        let high = goals.create(spec_p(high, 2)).unwrap();
        (goals, goal, high)
    }

    fn wait_for(what: &str, cond: impl Fn() -> bool) {
        let deadline = std::time::Instant::now() + Duration::from_secs(20);
        while !cond() {
            assert!(std::time::Instant::now() < deadline, "never: {what}");
            std::thread::sleep(Duration::from_millis(5));
        }
    }

    #[test]
    fn a_higher_goal_takes_the_worker_and_the_lower_one_resumes_after() {
        within(10, || {
            let (goals, low, high) = low_then_high(0, 5);
            run(&goals, low.clone());
            let s = status(&goals, low.id);
            assert_eq!(s.status, "paused");
            assert!(token_count(&s.output.unwrap()) < 20);
            let next = goals.next_goal().unwrap();
            assert_eq!(next.id, high);
            run(&goals, next);
            assert_eq!(status(&goals, high).status, "done");
            assert_ne!(status(&goals, low.id).status, "done");
            drain(&goals);
            assert_eq!(output(&goals, low.id), undisturbed(20, 1));
            assert_eq!(output(&goals, high), undisturbed(20, 2));
        });
    }

    #[test]
    fn an_equal_priority_goal_does_not_preempt() {
        let (goals, low, high) = low_then_high(3, 3);
        run(&goals, low.clone());
        assert_eq!(status(&goals, low.id).status, "done");
        assert_eq!(status(&goals, high).status, "queued");
    }

    #[test]
    fn the_lowest_running_goal_is_the_victim() {
        let dir = TempDir::new();
        let db = open(&dir.path().join("db")).unwrap();
        let goals = Goals::open(db.clone(), FAST).unwrap();
        goals.create(spec_p(3, 1)).unwrap();
        goals.create(spec_p(1, 2)).unwrap();
        let (three, one) = (goals.next_goal().unwrap(), goals.next_goal().unwrap());
        let fence = lease::claim(&db, one.id).unwrap();
        assert!(!step(&goals, &one, &fence).unwrap());
        goals.create(spec_p(5, 3)).unwrap();
        run(&goals, three.clone());
        assert_eq!(status(&goals, three.id).status, "done");
        run(&goals, one.clone());
        assert_eq!(status(&goals, one.id).status, "paused");
    }

    fn slow_goals(rate: u32) -> (Goals, Arc<Database>, TempDir) {
        let dir = TempDir::new();
        let db = open(&dir.path().join("db")).unwrap();
        let goals = Goals::open(db.clone(), NonZeroU32::new(rate).unwrap()).unwrap();
        (goals, db, dir)
    }

    #[test]
    fn a_record_corrupted_while_running_fails_that_goal_and_frees_its_worker() {
        let (goals, db, _dir) = slow_goals(80);
        goals.start_workers(NonZeroUsize::new(1).unwrap());
        let bad = goals.create(spec(100_000, 1)).unwrap();
        wait_for("bad progress", || !output(&goals, bad).is_empty());
        let good = goals.create(spec(20, 2)).unwrap();
        tamper_record(&db, (bad, 0));
        let failed = GoalStatus {
            status: "failed",
            output: None,
            error: Some("journal_corrupt"),
            node: None,
        };
        assert_eq!(status(&goals, bad), failed);
        wait_for("good done", || status(&goals, good).status == "done");
        assert_eq!(output(&goals, good), undisturbed(20, 2));
        assert_eq!(status(&goals, bad), failed);
    }

    #[test]
    fn a_free_worker_is_not_preempted() {
        let (goals, db, _dir) = slow_goals(80);
        goals.start_workers(NonZeroUsize::new(2).unwrap());
        let low = goals.create(spec_p(0, 1)).unwrap();
        wait_for("low running", || status(&goals, low).status == "running");
        let high = goals.create(spec_p(5, 2)).unwrap();
        wait_for("high done", || status(&goals, high).status == "done");
        assert_eq!(lease::claim(&db, low).unwrap().token, 2);
    }

    #[test]
    fn with_one_worker_the_higher_goal_finishes_first_and_both_come_out_right() {
        within(15, || {
            let (goals, _db, _dir) = slow_goals(80);
            goals.start_workers(NonZeroUsize::new(1).unwrap());
            let low = goals
                .create(GoalSpec {
                    priority: 0,
                    budget: 400,
                    seed: 1,
                    prompt: "hello".into(),
                })
                .unwrap();
            wait_for("low progress", || !output(&goals, low).is_empty());
            let high = goals.create(spec_p(5, 2)).unwrap();
            wait_for("high done", || status(&goals, high).status == "done");
            assert_ne!(status(&goals, low).status, "done");
            assert!(token_count(&output(&goals, low)) < 400);
            wait_for("low done", || status(&goals, low).status == "done");
            assert_eq!(output(&goals, low), undisturbed(400, 1));
            assert_eq!(output(&goals, high), undisturbed(20, 2));
        });
    }

    fn ops(r: &Rig, id: u64) -> usize {
        let records = r.journal.read(id).unwrap();
        records
            .iter()
            .filter(|rec| matches!(rec, Record::OpResult { .. }))
            .count()
    }

    #[test]
    fn a_preempted_script_writes_nothing_more_and_finishes_each_write_once() {
        let r = rig();
        let id = r.script(r#"Deno.core.ops.op_rs3_insert('{"a":"1"}'); 'x'"#);
        let low = r.goals.next_goal().unwrap();
        let high = r.goals.create(spec_p(5, 2)).unwrap();
        run(&r.goals, low);
        assert!(r.store.get("a").is_err());
        assert_eq!(status(&r.goals, id).status, "queued");
        let next = r.goals.next_goal().unwrap();
        assert_eq!(next.id, high);
        run(&r.goals, next);
        drain(&r.goals);
        assert_eq!(status(&r.goals, id), st("done", Some("x"), None));
        assert_eq!(r.store.get("a"), Ok("1".to_owned()));
        assert_eq!(ops(&r, id), 1);
    }

    #[test]
    fn a_preempted_script_does_not_repeat_a_logged_op() {
        let r = rig();
        let id = r.script(
            r#"Deno.core.ops.op_rs3_insert('{"a":"1"}');
               Deno.core.ops.op_rs3_insert('{"b":"2"}'); 'x'"#,
        );
        r.seed(id, &op("op_rs3_insert", &[r#"{"a":"1"}"#], r#"{"a":"1"}"#));
        let low = r.goals.next_goal().unwrap();
        r.goals.create(spec_p(5, 2)).unwrap();
        run(&r.goals, low);
        assert!(r.store.get("b").is_err());
        assert_eq!(status(&r.goals, id).status, "paused");
        drain(&r.goals);
        assert_eq!(status(&r.goals, id), st("done", Some("x"), None));
        assert!(r.store.get("a").is_err());
        assert_eq!(r.store.get("b"), Ok("2".to_owned()));
        assert_eq!(ops(&r, id), 2);
    }

    #[test]
    fn a_preempted_script_that_times_out_is_queued_again_not_failed() {
        let js = dedicated_executor(JsSettings {
            timeout_ms: 100,
            ..JsSettings::default()
        });
        let r = rig_with(js, true);
        let id = r.script("while (true) {}");
        let low = r.goals.next_goal().unwrap();
        r.goals.create(spec_p(5, 2)).unwrap();
        run(&r.goals, low);
        assert_eq!(status(&r.goals, id), st("queued", None, None));
        let last = r.journal.read(id).unwrap().pop().unwrap();
        assert!(!matches!(last, Record::ScriptEnd { .. }));
    }

    #[test]
    fn running_a_finished_goal_changes_nothing() {
        let dir = TempDir::new();
        let goals = goals_at(&dir.path().join("db"));
        let id = goals.create(spec(5, 1)).unwrap();
        let goal = goals.next_goal().unwrap();
        run(&goals, goal.clone());
        let before = status(&goals, id);
        run(&goals, goal);
        assert_eq!(status(&goals, id), before);
        assert_eq!(goals.next_goal(), None);
    }

    #[test]
    fn a_stale_fence_commits_nothing_even_on_the_final_batch() {
        let dir = TempDir::new();
        let db = open(&dir.path().join("db")).unwrap();
        let goals = Goals::open(db.clone(), FAST).unwrap();
        let id = goals.create(spec(8, 2)).unwrap();
        let goal = goals.next_goal().unwrap();
        let stale = lease::claim(&db, id).unwrap();
        lease::claim(&db, id).unwrap();
        assert!(matches!(
            step(&goals, &goal, &stale),
            Err(JournalError::Fenced { .. })
        ));
        let s = status(&goals, id);
        assert_eq!(s.output.as_deref(), Some(""));
        assert_ne!(s.status, "done");
    }

    #[test]
    fn the_seed_decides_the_output() {
        let dir = TempDir::new();
        let goals = goals_at(&dir.path().join("db"));
        let a = goals.create(spec(10, 1)).unwrap();
        let b = goals.create(spec(10, 1)).unwrap();
        let c = goals.create(spec(10, 2)).unwrap();
        drain(&goals);
        assert_eq!(output(&goals, a), output(&goals, b));
        assert_ne!(output(&goals, a), output(&goals, c));
    }

    #[test]
    fn a_corrupt_stream_fails_without_blocking_other_goals() {
        let dir = TempDir::new();
        let path = dir.path().join("db");
        let db = open(&path).unwrap();
        let goals = Goals::open(db.clone(), FAST).unwrap();
        let bad = goals.create(spec(10, 1)).unwrap();
        let good = goals.create(spec(10, 2)).unwrap();
        tamper_record(&db, (bad, 0));
        assert_eq!(
            status(&goals, bad),
            GoalStatus {
                status: "failed",
                output: None,
                error: Some("journal_corrupt"),
                node: None
            }
        );
        drop((goals, db));
        let goals = goals_at(&path);
        assert_eq!(goals.next_goal().map(|g| g.id), Some(good));
        assert_eq!(goals.next_goal(), None);
    }

    #[test]
    fn a_storage_failure_is_reported_and_resumes_after_restart() {
        let dir = TempDir::new();
        let path = dir.path().join("db");
        let db = open(&path).unwrap();
        let goals = Goals::open(db.clone(), FAST).unwrap();
        let id = goals.create(spec(20, 4)).unwrap();
        let original = head_bytes(&db, id);
        flip_head(&db, id);
        drain(&goals);
        set_head(&db, id, &original);
        assert_eq!(
            status(&goals, id),
            GoalStatus {
                status: "failed",
                output: Some(String::new()),
                error: Some("storage_failure"),
                node: None
            }
        );
        assert_eq!(goals.next_goal(), None);
        drop((goals, db));
        let goals = goals_at(&path);
        drain(&goals);
        assert_eq!(output(&goals, id), undisturbed(20, 4));
    }

    #[test]
    fn unknown_goals_and_unreadable_storage() {
        let dir = TempDir::new();
        let db = open(&dir.path().join("db")).unwrap();
        let goals = Goals::open(db.clone(), FAST).unwrap();
        assert_eq!(goals.status(7), Ok(None));
        let txn = db.begin_write().unwrap();
        txn.delete_table(HEADS).unwrap();
        txn.open_table(TableDefinition::<&str, &str>::new("journal_heads"))
            .unwrap();
        txn.commit().unwrap();
        assert!(goals.status(1).is_err());
    }

    // ---- script goals ----

    use crate::journal::OpError;
    use crate::js::{JsSettings, dedicated_executor, test_executor};

    struct Rig {
        _dir: TempDir,
        db: Arc<Database>,
        store: Store,
        goals: Goals,
        journal: Journal,
    }

    fn rig_with(js: JsExecutor, enable: bool) -> Rig {
        let dir = TempDir::new();
        let db = open(&dir.path().join("db")).unwrap();
        let store = Store::durable(db.clone()).unwrap();
        let goals = Goals::open(db.clone(), FAST).unwrap();
        if enable {
            goals.enable_scripts(store.clone(), js);
        }
        let journal = Journal::new(db.clone()).unwrap();
        Rig {
            _dir: dir,
            db,
            store,
            goals,
            journal,
        }
    }

    fn rig() -> Rig {
        rig_with(test_executor(), true)
    }

    impl Rig {
        fn script(&self, source: &str) -> u64 {
            self.goals
                .create_script(0, "s".into(), source.into())
                .unwrap()
        }

        /// Append `record` to the stream of `id`, as an earlier run would have.
        fn seed(&self, id: u64, record: &Record) {
            let next = self.journal.read(id).unwrap().len() as u64;
            self.journal.append(id, next, record).unwrap();
        }

        /// Open the same database again, with scripts enabled.
        fn reopen(self) -> Rig {
            let Rig {
                _dir, db, store, ..
            } = self;
            let goals = Goals::open(db.clone(), FAST).unwrap();
            goals.enable_scripts(store.clone(), test_executor());
            let journal = Journal::new(db.clone()).unwrap();
            Rig {
                _dir,
                db,
                store,
                goals,
                journal,
            }
        }
    }

    fn op(name: &str, args: &[&str], result: &str) -> Record {
        Record::OpResult {
            op: name.into(),
            args: args.iter().map(|a| (*a).to_owned()).collect(),
            result: Ok::<_, OpError>(result.into()),
        }
    }

    fn st(status: &'static str, output: Option<&str>, error: Option<&'static str>) -> GoalStatus {
        GoalStatus {
            status,
            output: output.map(str::to_owned),
            error,
            node: None,
        }
    }

    #[test]
    fn a_script_goal_runs_to_done_with_its_value_and_writes() {
        let r = rig();
        let id = r.script(
            r#"Deno.core.ops.op_rs3_insert('{"n":"1"}');
               const n = Number(Deno.core.ops.op_rs3_get('n'));
               Deno.core.ops.op_rs3_insert(JSON.stringify({n: String(n + 1)}));
               'finished'"#,
        );
        assert_eq!(r.goals.status(id).unwrap().unwrap().status, "queued");
        drain(&r.goals);
        assert_eq!(status(&r.goals, id), st("done", Some("finished"), None));
        assert_eq!(r.store.get("n"), Ok("2".to_owned()));
    }

    #[test]
    fn a_script_without_ops_ends_with_its_value() {
        let r = rig();
        let id = r.script("6 * 7");
        drain(&r.goals);
        assert_eq!(status(&r.goals, id), st("done", Some("42"), None));
    }

    #[test]
    fn a_crashed_script_does_not_repeat_its_logged_write() {
        let r = rig();
        let id = r.script(
            r#"Deno.core.ops.op_rs3_insert('{"a":"1"}');
               Deno.core.ops.op_rs3_insert('{"b":"2"}');
               'ok'"#,
        );
        // The first run logged its first write, which the store lacks.
        r.seed(id, &op("op_rs3_insert", &[r#"{"a":"1"}"#], r#"{"a":"1"}"#));
        let r = r.reopen();
        drain(&r.goals);
        assert_eq!(status(&r.goals, id), st("done", Some("ok"), None));
        assert!(r.store.get("a").is_err());
        assert_eq!(r.store.get("b"), Ok("2".to_owned()));
    }

    #[test]
    fn a_fully_logged_script_finishes_without_running_anything() {
        let r = rig();
        let id = r.script(r#"Deno.core.ops.op_rs3_insert('{"a":"1"}'); 'ok'"#);
        r.seed(id, &op("op_rs3_insert", &[r#"{"a":"1"}"#], r#"{"a":"1"}"#));
        let r = r.reopen();
        drain(&r.goals);
        assert_eq!(status(&r.goals, id), st("done", Some("ok"), None));
        assert!(r.store.get("a").is_err());
    }

    #[test]
    fn queued_script_goals_survive_reopen_and_finished_ones_do_not() {
        let r = rig();
        let queued = r.script("1");
        let r = r.reopen();
        assert_eq!(r.goals.next_goal().map(|g| g.id), Some(queued));
        run(
            &r.goals,
            Goal {
                id: queued,
                priority: 0,
                budget: 0,
            },
        );
        let r = r.reopen();
        assert_eq!(r.goals.next_goal(), None);
        assert_eq!(status(&r.goals, queued).status, "done");
    }

    #[test]
    fn script_errors_are_durable_failures_with_no_output() {
        let r = rig();
        let thrown = r.script("throw new Error('boom')");
        r.store
            .insert("loop".into(), "Deno.core.ops.op_rs3_patch('loop')".into());
        let deep = r.script("Deno.core.ops.op_rs3_patch('loop')");
        drain(&r.goals);
        let r = r.reopen();
        assert_eq!(r.goals.next_goal(), None);
        for id in [thrown, deep] {
            assert_eq!(status(&r.goals, id), st("failed", None, Some("js_error")));
        }
    }

    #[test]
    fn a_script_that_runs_too_long_fails_with_js_timeout() {
        let js = dedicated_executor(JsSettings {
            timeout_ms: 100,
            ..JsSettings::default()
        });
        let r = rig_with(js, true);
        let id = r.script("while (true) {}");
        drain(&r.goals);
        assert_eq!(status(&r.goals, id), st("failed", None, Some("js_timeout")));
    }

    #[test]
    fn a_script_that_disagrees_with_its_log_fails_with_replay_diverged() {
        let r = rig();
        let mismatch = r.script("Deno.core.ops.op_rs3_get('a')");
        r.seed(mismatch, &op("op_rs3_get", &["b"], "1"));
        let longer = r.script("'short'");
        r.seed(longer, &op("op_rs3_get", &["a"], "1"));
        let writer =
            r.script(r#"Deno.core.ops.op_rs3_get('x'); Deno.core.ops.op_rs3_insert('{"w":"1"}')"#);
        r.seed(writer, &op("op_rs3_get", &["y"], "1"));
        let r = r.reopen();
        drain(&r.goals);
        for id in [mismatch, longer, writer] {
            assert_eq!(
                status(&r.goals, id),
                st("failed", None, Some("replay_diverged"))
            );
        }
        assert!(r.store.get("w").is_err());
        assert_eq!(r.journal.read(longer).unwrap().len(), 3);
    }

    #[test]
    fn a_script_whose_lease_is_taken_over_stops_without_ending() {
        let r = rig();
        let id = r.script(
            r#"Deno.core.ops.op_rs3_insert('{"a":"1"}');
               while (((() => { try { return Deno.core.ops.op_rs3_get('go'); } catch (e) { return ''; } })()) !== '1') {}
               Deno.core.ops.op_rs3_insert('{"late":"1"}'); 'x'"#,
        );
        let goal = r.goals.next_goal().unwrap();
        let worker = {
            let goals = r.goals.clone();
            std::thread::spawn(move || goals.run(goal))
        };
        let deadline = std::time::Instant::now() + Duration::from_secs(10);
        while r.store.get("a").is_err() {
            assert!(std::time::Instant::now() < deadline, "no write");
            std::thread::sleep(Duration::from_millis(5));
        }
        lease::claim(&r.db, id).unwrap();
        r.store.insert("go".into(), "1".into());
        within_deadline(move || worker.join().unwrap());
        let s = status(&r.goals, id);
        assert!(s.error.is_none());
        assert!(r.store.get("late").is_err());
        let last = r.journal.read(id).unwrap().pop().unwrap();
        assert!(!matches!(last, Record::ScriptEnd { .. }));
        assert_eq!(s.status, "paused");
        assert_eq!(r.goals.next_goal().map(|g| g.id), Some(id));
    }

    #[test]
    fn a_script_with_logged_ops_reads_paused_and_finishes_each_write_once() {
        let r = rig();
        let id = r.script(
            r#"Deno.core.ops.op_rs3_insert('{"a":"1"}');
               Deno.core.ops.op_rs3_insert('{"b":"2"}'); 'x'"#,
        );
        assert_eq!(status(&r.goals, id).status, "queued");
        r.seed(id, &op("op_rs3_insert", &[r#"{"a":"1"}"#], r#"{"a":"1"}"#));
        assert_eq!(status(&r.goals, id).status, "paused");
        drain(&r.goals);
        assert_eq!(status(&r.goals, id), st("done", Some("x"), None));
        assert_eq!(r.store.get("b").unwrap(), "2");
    }

    #[test]
    fn a_corrupt_script_record_fails_that_goal_only() {
        let r = rig();
        let bad = r.script("1");
        let good = r.script("2");
        tamper_record(&r.db, (bad, 0));
        let r = r.reopen();
        drain(&r.goals);
        assert_eq!(
            status(&r.goals, bad),
            st("failed", None, Some("journal_corrupt"))
        );
        assert_eq!(status(&r.goals, good), st("done", Some("2"), None));
    }

    #[test]
    fn without_scripts_enabled_a_script_goal_fails_in_memory_and_runs_after_reopen() {
        let r = rig_with(test_executor(), false);
        let id = r.script("'later'");
        drain(&r.goals);
        assert_eq!(
            status(&r.goals, id),
            st("failed", None, Some("storage_failure"))
        );
        let r = r.reopen();
        drain(&r.goals);
        assert_eq!(status(&r.goals, id), st("done", Some("later"), None));
    }

    #[test]
    fn a_closed_js_pool_fails_in_memory_and_the_goal_resumes() {
        let r = rig_with(JsExecutor::closed(), true);
        let id = r.script("'later'");
        drain(&r.goals);
        assert_eq!(
            status(&r.goals, id),
            st("failed", None, Some("storage_failure"))
        );
        let r = r.reopen();
        drain(&r.goals);
        assert_eq!(status(&r.goals, id), st("done", Some("later"), None));
    }

    // ---- cancellation ----

    fn cancelled(output: Option<&str>) -> GoalStatus {
        st("cancelled", output, None)
    }

    fn spawn_run(goals: &Goals, goal: Goal) -> std::thread::JoinHandle<()> {
        let goals = goals.clone();
        std::thread::spawn(move || goals.run(goal))
    }

    fn join(worker: std::thread::JoinHandle<()>) {
        within_deadline(move || worker.join().unwrap());
    }

    #[test]
    fn cancelling_a_running_goal_stops_its_worker_and_keeps_its_output() {
        let (goals, _db, _dir) = slow_goals(80);
        let id = goals.create(spec(400, 1)).unwrap();
        let worker = spawn_run(&goals, goals.next_goal().unwrap());
        wait_for("progress", || !output(&goals, id).is_empty());
        let result = goals.cancel(id).unwrap().unwrap();
        join(worker);
        assert_eq!(result.status, "cancelled");
        assert_eq!(result.error, None);
        assert!(token_count(result.output.as_ref().unwrap()) < 400);
        assert_eq!(status(&goals, id), result);
        assert_eq!(goals.next_goal(), None);
    }

    #[test]
    fn a_cancel_survives_a_reopen() {
        let dir = TempDir::new();
        let path = dir.path().join("db");
        let goals = goals_at(&path);
        let id = goals.create(spec(20, 1)).unwrap();
        let goal = goals.next_goal().unwrap();
        let fence = lease::claim(&goals.0.db, id).unwrap();
        step(&goals, &goal, &fence).unwrap();
        let result = goals.cancel(id).unwrap().unwrap();
        assert!(!result.output.as_ref().unwrap().is_empty());
        drop(goals);
        let goals = goals_at(&path);
        assert_eq!(goals.next_goal(), None);
        assert_eq!(status(&goals, id), result);
    }

    #[test]
    fn cancelling_a_queued_goal_leaves_the_others_queued() {
        let dir = TempDir::new();
        let goals = goals_at(&dir.path().join("db"));
        let a = goals.create(spec(5, 1)).unwrap();
        let b = goals.create(spec(5, 2)).unwrap();
        assert_eq!(goals.cancel(a).unwrap().unwrap(), cancelled(Some("")));
        assert_eq!(goals.next_goal().map(|g| g.id), Some(b));
        assert_eq!(goals.next_goal(), None);
    }

    #[test]
    fn a_goal_cancelled_after_it_was_taken_runs_nothing() {
        let r = rig();
        let id = r.script(r#"Deno.core.ops.op_rs3_insert('{"a":"1"}'); 'x'"#);
        let goal = r.goals.next_goal().unwrap();
        r.goals.cancel(id).unwrap();
        run(&r.goals, goal);
        assert!(r.store.get("a").is_err());
        assert_eq!(status(&r.goals, id), cancelled(None));
        assert_eq!(r.goals.next_goal(), None);
    }

    #[test]
    fn a_generation_goal_cancelled_after_it_was_taken_stays_cancelled() {
        let dir = TempDir::new();
        let goals = goals_at(&dir.path().join("db"));
        let id = goals.create(spec(20, 1)).unwrap();
        let goal = goals.next_goal().unwrap();
        goals.cancel(id).unwrap();
        run(&goals, goal);
        assert_eq!(status(&goals, id), cancelled(Some("")));
        assert_eq!(goals.next_goal(), None);
    }

    #[test]
    fn cancelling_a_running_script_refuses_its_next_op() {
        let r = rig();
        let id = r.script(
            r#"Deno.core.ops.op_rs3_insert('{"a":"1"}');
               while (((() => { try { return Deno.core.ops.op_rs3_get('go'); } catch (e) { return ''; } })()) !== '1') {}
               Deno.core.ops.op_rs3_insert('{"late":"1"}'); 'x'"#,
        );
        let worker = spawn_run(&r.goals, r.goals.next_goal().unwrap());
        wait_for("first write", || r.store.get("a").is_ok());
        r.goals.cancel(id).unwrap();
        r.store.insert("go".into(), "1".into());
        join(worker);
        assert!(r.store.get("late").is_err());
        assert_eq!(status(&r.goals, id), cancelled(None));
        assert_eq!(r.goals.next_goal(), None);
    }

    #[test]
    fn a_cancel_takes_over_the_lease_so_an_earlier_fence_is_stale() {
        let dir = TempDir::new();
        let goals = goals_at(&dir.path().join("db"));
        let id = goals.create(spec(20, 1)).unwrap();
        let goal = goals.next_goal().unwrap();
        let stale = lease::claim(&goals.0.db, id).unwrap();
        goals.cancel(id).unwrap();
        assert!(!matches!(step(&goals, &goal, &stale), Ok(false)));
        assert_eq!(status(&goals, id), cancelled(Some("")));
        assert_eq!(lease::claim(&goals.0.db, id).unwrap().token, 3);
    }

    #[test]
    fn cancelling_twice_ends_the_stream_once() {
        let dir = TempDir::new();
        let goals = goals_at(&dir.path().join("db"));
        let id = goals.create(spec(5, 1)).unwrap();
        let first = goals.cancel(id).unwrap();
        assert_eq!(goals.cancel(id).unwrap(), first);
        let ends = goals.0.journal.read(id).unwrap();
        assert_eq!(ends.iter().filter(|r| r.ends_stream()).count(), 1);
        assert_eq!(lease::claim(&goals.0.db, id).unwrap().token, 2);
    }

    #[test]
    fn a_cancel_changes_no_goal_that_has_already_ended() {
        let r = rig();
        let done = r.goals.create(spec(5, 1)).unwrap();
        let failed = r.script("throw new Error('boom')");
        drain(&r.goals);
        let (d, f) = (status(&r.goals, done), status(&r.goals, failed));
        assert_eq!(d.status, "done");
        assert_eq!(r.goals.cancel(done).unwrap(), Some(d));
        assert_eq!(f.error, Some("js_error"));
        assert_eq!(r.goals.cancel(failed).unwrap(), Some(f));
        assert_eq!(r.goals.cancel(99).unwrap(), None);
    }

    #[test]
    fn cancelling_a_corrupt_goal_leaves_its_bytes_alone() {
        let dir = TempDir::new();
        let db = open(&dir.path().join("db")).unwrap();
        let goals = Goals::open(db.clone(), FAST).unwrap();
        let id = goals.create(spec(5, 1)).unwrap();
        tamper_record(&db, (id, 0));
        let before = head_bytes(&db, id);
        assert_eq!(
            goals.cancel(id).unwrap(),
            Some(st("failed", None, Some("journal_corrupt")))
        );
        assert_eq!(head_bytes(&db, id), before);
    }

    #[test]
    fn a_goal_with_a_storage_failure_can_be_cancelled() {
        let dir = TempDir::new();
        let db = open(&dir.path().join("db")).unwrap();
        let goals = Goals::open(db.clone(), FAST).unwrap();
        let id = goals.create(spec(20, 4)).unwrap();
        let original = head_bytes(&db, id);
        flip_head(&db, id);
        drain(&goals);
        set_head(&db, id, &original);
        assert_eq!(status(&goals, id).status, "failed");
        assert_eq!(goals.cancel(id).unwrap(), Some(cancelled(Some(""))));
    }

    #[test]
    fn cancelling_with_a_broken_journal_table_is_an_error() {
        let dir = TempDir::new();
        let db = open(&dir.path().join("db")).unwrap();
        let goals = Goals::open(db.clone(), FAST).unwrap();
        let txn = db.begin_write().unwrap();
        txn.delete_table(HEADS).unwrap();
        txn.open_table(TableDefinition::<&str, &str>::new("journal_heads"))
            .unwrap();
        txn.commit().unwrap();
        assert!(goals.cancel(1).is_err());
    }
}
