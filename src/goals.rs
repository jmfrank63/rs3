//! Generation goals: durable creation, batched commits, status and recovery.
//!
//! The journal stream of a goal is the source of truth for its progress and end.
//! The `goals` table holds only what the stream cannot: priority and budget. It
//! is written in the same transaction as the stream's start record, so a goal
//! exists completely or not at all. A goal's id is its stream id.
use std::collections::{HashMap, HashSet, VecDeque};
use std::num::{NonZeroU32, NonZeroUsize};
use std::sync::{Arc, Condvar, Mutex, MutexGuard, PoisonError};
use std::time::Duration;

use redb::{Database, ReadableDatabase, ReadableTable, TableDefinition};

use crate::journal::{Journal, JournalError, Record, storage};
use crate::lease::{self, Fence};
use crate::model::{FakeModel, Model};

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
    pub budget: u64,
}

/// What a client sees of a goal. The output holds committed tokens only.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GoalStatus {
    pub status: &'static str,
    pub output: Option<String>,
    pub error: Option<&'static str>,
}

#[derive(Default)]
struct Sched {
    queue: VecDeque<Goal>,
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
        }));
        for goal in goals.all()? {
            match goals.0.journal.restore(goal.id) {
                Ok(Some(state)) if state.ended.is_none() => goals.sched().queue.push_back(goal),
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
                budget: row.value().1,
            });
        }
        Ok(goals)
    }

    fn sched(&self) -> MutexGuard<'_, Sched> {
        self.0.sched.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// Create a goal and queue it. The caller validates the budget.
    pub fn create(&self, spec: GoalSpec) -> Result<u64, JournalError> {
        let txn = self.0.db.begin_write().map_err(storage)?;
        let id = {
            let mut table = txn.open_table(GOALS).map_err(storage)?;
            let id = table
                .last()
                .map_err(storage)?
                .map_or(1, |(key, _)| key.value() + 1);
            table
                .insert(id, (spec.priority, spec.budget))
                .map_err(storage)?;
            id
        };
        let start = Record::InferenceStart {
            engine: "fake".into(),
            model_hash: "fake".into(),
            tokenizer_hash: "fake".into(),
            sampler: serde_json::json!({ "seed": spec.seed }).to_string(),
            rng_state: FakeModel.start_state(spec.seed, &spec.prompt),
            prompt_tokens: spec.prompt.bytes().map(u32::from).collect(),
        };
        self.0.journal.append_in(&txn, id, 0, &start)?;
        txn.commit().map_err(storage)?;
        self.sched().queue.push_back(Goal {
            id,
            budget: spec.budget,
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
        let state = match self.0.journal.restore(id) {
            Ok(Some(state)) => state,
            Ok(None) => return Ok(None),
            Err(JournalError::Corrupt { .. }) => {
                return Ok(Some(GoalStatus {
                    status: "failed",
                    output: None,
                    error: Some("journal_corrupt"),
                }));
            }
            Err(e) => return Err(e),
        };
        let (status, error) = match (state.ended.is_some(), failed, running) {
            (true, _, _) => ("done", None),
            (_, true, _) => ("failed", Some("storage_failure")),
            (_, _, true) => ("running", None),
            _ => ("queued", None),
        };
        Ok(Some(GoalStatus {
            status,
            output: Some(FakeModel.render(&state.tokens)),
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
        let goal = sched.queue.pop_front()?;
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

    /// Claim `goal`, then step it until it ends. A storage failure is logged
    /// once and kept in memory only, so the goal resumes on the next start. A
    /// worker whose lease was taken over stops quietly: that is normal control
    /// flow, not a failure.
    pub fn run(&self, goal: Goal) {
        let result = lease::claim(&self.0.db, goal.id).and_then(|fence| {
            loop {
                if self.step(&goal, &fence)? {
                    break Ok(());
                }
            }
        });
        let mut sched = self.sched();
        if let Err(e) = result
            && !matches!(e, JournalError::Fenced { .. })
        {
            eprintln!("error: goal {}: {e}", goal.id);
            sched.failures.insert(goal.id, e.to_string());
        }
        sched.running.remove(&goal.id);
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

    fn drain(goals: &Goals) {
        while let Some(goal) = goals.next_goal() {
            goals.run(goal);
        }
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
        assert_eq!(goals.next_goal(), Some(Goal { id, budget: 5 }));
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
        assert!(!goals.step(&goal, &fence).unwrap());
        let first = output(&goals, id);
        assert!(!goals.step(&goal, &fence).unwrap());
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
        worker.join().unwrap();
        let s = status(&goals, id);
        assert_eq!(s.output.as_deref(), Some(snapshot.as_str()));
        assert!(token_count(&snapshot) < 800);
        assert!(s.status != "failed" && s.status != "running");
        assert_eq!(s.error, None);
        let fence = lease::claim(&db, id).unwrap();
        while !goals.step(&goal, &fence).unwrap() {}
        assert_eq!(output(&goals, id), undisturbed(800, 5));
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
            goals.step(&goal, &stale),
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
}
