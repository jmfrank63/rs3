//! Generation goals: durable creation, batched commits, status and recovery.
//!
//! The journal stream of a goal is the source of truth for its progress and end.
//! The `goals` table holds only what the stream cannot: priority and budget. It
//! is written in the same transaction as the stream's start record, so a goal
//! exists completely or not at all. A goal's id is its stream id.
use std::cmp::Reverse;
use std::collections::{HashMap, HashSet};
use std::num::{NonZeroU32, NonZeroUsize};
use std::sync::{Arc, Condvar, Mutex, MutexGuard, OnceLock, PoisonError};
use std::time::Duration;

use redb::{Database, ReadableDatabase, ReadableTable, TableDefinition};

use crate::journal::{Journal, JournalError, Record, ScriptFailure, storage};
use crate::js::ExecError;
use crate::js::executor::JsExecutor;
use crate::js::oplog::{LogStop, OpLog};
use crate::lease::{self, Fence};
use crate::model::{FakeModel, Model};
use crate::store::Store;

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
}

/// The status and error of a goal that has no durable result.
fn flags(
    running: bool,
    failed: bool,
    done: bool,
    progress: bool,
) -> (&'static str, Option<&'static str>) {
    match (done, failed, running, progress) {
        (true, ..) => ("done", None),
        (_, true, ..) => ("failed", Some("storage_failure")),
        (_, _, true, _) => ("running", None),
        (.., true) => ("paused", None),
        _ => ("queued", None),
    }
}

#[derive(Default)]
struct Sched {
    queue: Vec<Goal>,
    running: HashSet<u64>,
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
}

/// Shared handle to the goals of one database.
#[derive(Clone)]
pub struct Goals(Arc<Inner>);

impl Goals {
    /// Use `db` for goals, queueing every goal that has not ended. A goal whose
    /// stream fails verification is logged once and left out.
    /// Workers emit `rate` tokens per second.
    pub fn open(db: Arc<Database>, rate: NonZeroU32) -> Result<Self, JournalError> {
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
        }));
        for goal in goals.all()? {
            match goals.0.journal.read(goal.id) {
                Ok(records) if records.last().is_some_and(|last| !last.ends_stream()) => {
                    goals.sched().queue.push(goal);
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
        let txn = self.0.db.begin_write().map_err(storage)?;
        let id = {
            let mut table = txn.open_table(GOALS).map_err(storage)?;
            let id = table
                .last()
                .map_err(storage)?
                .map_or(1, |(key, _)| key.value() + 1);
            table.insert(id, (priority, budget)).map_err(storage)?;
            id
        };
        self.0.journal.append_in(&txn, id, 0, start)?;
        txn.commit().map_err(storage)?;
        self.sched().queue.push(Goal {
            id,
            priority,
            budget,
        });
        self.0.wake.notify_one();
        Ok(id)
    }

    /// The status of goal `id`, or `None` when there is no such goal.
    pub fn status(&self, id: u64) -> Result<Option<GoalStatus>, JournalError> {
        // Read the flags before the stream: the other order can report a goal
        // that finished in between as queued.
        let (running, failed) = {
            let sched = self.sched();
            (
                sched.running.contains(&id),
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
                }));
            }
            Err(e) => return Err(e),
        };
        let progress = records.len() > 1;
        let (status, error, output) = match records.last() {
            Some(Record::ScriptEnd { outcome: Ok(value) }) => ("done", None, Some(value.clone())),
            Some(Record::ScriptEnd { outcome: Err(f) }) => ("failed", Some(f.code()), None),
            _ if matches!(records.first(), Some(Record::ScriptStart { .. })) => {
                let (status, error) = flags(running, failed, false, progress);
                (status, error, None)
            }
            _ => {
                let Some(state) = Journal::rebuild(id, records)? else {
                    return Ok(None);
                };
                let (status, error) = flags(running, failed, state.ended.is_some(), progress);
                (status, error, Some(FakeModel.render(&state.tokens)))
            }
        };
        Ok(Some(GoalStatus {
            status,
            output,
            error,
        }))
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
        let finished = done + count >= goal.budget;
        std::thread::sleep(Duration::from_secs_f64(count as f64 * self.0.token_secs));
        let txn = self.0.db.begin_write().map_err(storage)?;
        lease::check_in(&txn, fence)?;
        let record = Record::Tokens {
            tokens,
            rng_state: rng,
        };
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

    fn take(sched: &mut Sched) -> Option<Goal> {
        let next = (0..sched.queue.len())
            .max_by_key(|&i| (sched.queue[i].priority, Reverse(sched.queue[i].id)))?;
        let goal = sched.queue.remove(next);
        sched.running.insert(goal.id);
        Some(goal)
    }

    /// Take the next queued goal and mark it running. Pair it with `run`.
    pub fn next_goal(&self) -> Option<Goal> {
        Self::take(&mut self.sched())
    }

    /// Start `workers` threads that run queued goals for the life of the
    /// process. There is no shutdown: the threads end with the process.
    pub fn start_workers(&self, workers: NonZeroUsize) {
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
        let log = Arc::new(OpLog::new(
            self.0.db.clone(),
            self.0.journal.clone(),
            fence,
            records,
        ));
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

    fn drive(&self, goal: &Goal) -> Result<(), JournalError> {
        let fence = lease::claim(&self.0.db, goal.id)?;
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
            _ => self
                .0
                .journal
                .read(goal.id)
                .map(|records| records.last().is_some_and(|last| !last.ends_stream())),
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

    fn goals_at(path: &Path) -> Goals {
        Goals::open(open(path).unwrap(), FAST).unwrap()
    }

    /// Run `f` on its own thread and fail the test, never hang it, if it takes
    /// over 30 s. A stuck thread dies with the test process.
    fn within_deadline<T: Send + 'static>(f: impl FnOnce() -> T + Send + 'static) -> T {
        let (tx, rx) = std::sync::mpsc::channel();
        let thread = std::thread::spawn(move || {
            let _ = tx.send(f());
        });
        match rx.recv_timeout(Duration::from_secs(30)) {
            Ok(value) => value,
            Err(std::sync::mpsc::RecvTimeoutError::Timeout) => panic!("stuck for over 30 s"),
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
                error: None
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
                error: Some("journal_corrupt")
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
                error: Some("storage_failure")
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
               while (Deno.core.ops.op_rs3_get('go') !== '1') {}
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
}
