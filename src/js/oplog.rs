//! The op log of one script goal. Every op result a script saw is a record in
//! the goal's journal stream. A run replays the logged ops in order, returning
//! what the first run saw without touching the store, then continues live and
//! logs each new op under the goal's fence. This module is free of actix-web.
//!
//! Lock order: a caller holds the database writer first and this log's mutex
//! second. Nothing takes the mutex and then begins a write.
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};

use redb::{Database, WriteTransaction};

use crate::journal::{Journal, JournalError, OpError, Record, ScriptFailure, storage};
use crate::lease::{Fence, check_in};

/// Why a logged job may not run any more ops.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LogStop {
    /// A later claim took over the goal's lease.
    Fenced,
    /// The script did something the log says it did not do the first time.
    Diverged,
}

/// What the log says about the op a script is about to run.
#[derive(Debug, PartialEq, Eq)]
pub enum Replay {
    /// The log is used up: run the op and log it.
    Live,
    /// The op ran before: answer with this and touch nothing.
    Logged(Result<String, OpError>),
    /// The op differs from the logged one. The log is now stopped.
    Diverged,
}

struct Logged {
    op: String,
    args: Vec<String>,
    result: Result<String, OpError>,
}

struct State {
    ops: Vec<Logged>,
    cursor: usize,
    next_seq: u64,
    stop: Option<LogStop>,
}

/// The ordered op log of one script run, shared by every PATCH level of it.
pub struct OpLog {
    db: Arc<Database>,
    journal: Journal,
    fence: Fence,
    state: Mutex<State>,
}

impl OpLog {
    /// A log over the stream `fence.task`, whose committed `records` are read
    /// back with `Journal::read`.
    pub fn new(db: Arc<Database>, journal: Journal, fence: Fence, records: &[Record]) -> Self {
        let ops = records
            .iter()
            .filter_map(|record| match record {
                Record::OpResult { op, args, result } => Some(Logged {
                    op: op.clone(),
                    args: args.clone(),
                    result: result.clone(),
                }),
                _ => None,
            })
            .collect();
        Self {
            db,
            journal,
            fence,
            state: Mutex::new(State {
                ops,
                cursor: 0,
                next_seq: records.len() as u64,
                stop: None,
            }),
        }
    }

    fn state(&self) -> MutexGuard<'_, State> {
        self.state.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// The stream this log writes to.
    pub fn stream(&self) -> u64 {
        self.fence.task
    }

    /// Why the job may not run any more ops, if it may not.
    pub fn stopped(&self) -> Option<LogStop> {
        self.state().stop
    }

    /// Stop the job. The first stop stays.
    pub fn stop(&self, why: LogStop) {
        self.state().stop.get_or_insert(why);
    }

    /// Whether every logged op has been replayed.
    pub fn fully_replayed(&self) -> bool {
        let state = self.state();
        state.cursor == state.ops.len()
    }

    /// Compare the op about to run with the next logged one. A match moves the
    /// cursor on; a mismatch stops the log as diverged.
    pub fn replay(&self, op: &str, args: &[String]) -> Replay {
        let mut state = self.state();
        let Some(logged) = state.ops.get(state.cursor) else {
            return Replay::Live;
        };
        if logged.op != op || logged.args != args {
            state.stop.get_or_insert(LogStop::Diverged);
            return Replay::Diverged;
        }
        let result = logged.result.clone();
        state.cursor += 1;
        Replay::Logged(result)
    }

    /// Append the result of a live op inside `txn`, after checking the fence in
    /// the same transaction. Commit `txn` to make the record permanent.
    pub fn append(
        &self,
        txn: &WriteTransaction,
        op: &str,
        args: &[String],
        result: &Result<String, OpError>,
    ) -> Result<(), JournalError> {
        check_in(txn, &self.fence)?;
        let mut state = self.state();
        let record = Record::OpResult {
            op: op.to_owned(),
            args: args.to_vec(),
            result: result.clone(),
        };
        self.journal
            .append_in(txn, self.fence.task, state.next_seq, &record)?;
        state.next_seq += 1;
        state.ops.push(Logged {
            op: op.to_owned(),
            args: args.to_vec(),
            result: result.clone(),
        });
        state.cursor = state.ops.len();
        Ok(())
    }

    /// Log the result of a live op that wrote nothing, in its own transaction.
    pub fn log_op(
        &self,
        op: &str,
        args: &[String],
        result: &Result<String, OpError>,
    ) -> Result<(), JournalError> {
        let txn = self.db.begin_write().map_err(storage)?;
        self.append(&txn, op, args, result)?;
        txn.commit().map_err(storage)
    }

    /// End the stream with `outcome`, under the fence.
    pub fn end(&self, outcome: Result<String, ScriptFailure>) -> Result<(), JournalError> {
        let txn = self.db.begin_write().map_err(storage)?;
        check_in(&txn, &self.fence)?;
        let next_seq = self.state().next_seq;
        self.journal.append_in(
            &txn,
            self.fence.task,
            next_seq,
            &Record::ScriptEnd { outcome },
        )?;
        txn.commit().map_err(storage)
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use redb::Database;

    use super::{LogStop, OpLog};
    use crate::journal::{Journal, JournalError, OpError, Record, ScriptFailure};
    use crate::js::executor::JsExecutor;
    use crate::js::ops::ExecCtx;
    use crate::js::runtime::run_script;
    use crate::js::{ExecError, JsSettings, block_on, dedicated_executor, test_executor};
    use crate::lease::{Fence, claim};
    use crate::storage::{TempDir, open};
    use crate::store::Store;

    const STREAM: u64 = 7;

    struct Rig {
        _dir: TempDir,
        db: Arc<Database>,
        journal: Journal,
        store: Store,
    }

    fn rig() -> Rig {
        let dir = TempDir::new();
        let db = open(&dir.path().join("db")).unwrap();
        let journal = Journal::new(db.clone()).unwrap();
        let store = Store::durable(db.clone()).unwrap();
        journal
            .append(
                STREAM,
                0,
                &Record::ScriptStart {
                    name: "test".into(),
                    source: String::new(),
                },
            )
            .unwrap();
        Rig {
            _dir: dir,
            db,
            journal,
            store,
        }
    }

    fn op(name: &str, args: &[&str], result: Result<&str, (&str, &str)>) -> Record {
        Record::OpResult {
            op: name.to_owned(),
            args: args.iter().map(|a| (*a).to_owned()).collect(),
            result: result
                .map(str::to_owned)
                .map_err(|(class, message)| OpError {
                    class: class.to_owned(),
                    message: message.to_owned(),
                }),
        }
    }

    impl Rig {
        fn seed(&self, record: &Record) {
            let next = self.journal.read(STREAM).unwrap().len() as u64;
            self.journal.append(STREAM, next, record).unwrap();
        }

        /// A log over what the journal holds now, under a fresh claim.
        fn log(&self) -> (Arc<OpLog>, Fence) {
            let fence = claim(&self.db, STREAM).unwrap();
            let records = self.journal.read(STREAM).unwrap();
            let log = OpLog::new(
                self.db.clone(),
                self.journal.clone(),
                fence.clone(),
                &records,
            );
            (Arc::new(log), fence)
        }

        fn ops(&self) -> Vec<Record> {
            self.journal.read(STREAM).unwrap().split_off(1)
        }

        /// Run `source` with a fresh log on the shared executor.
        fn run(&self, source: &str) -> Result<String, ExecError> {
            let (log, _) = self.log();
            self.run_with(&log, source).0
        }

        /// Run `source` on `log`; also return the job's context for inspection.
        fn run_with(&self, log: &Arc<OpLog>, source: &str) -> (Result<String, ExecError>, ExecCtx) {
            let mut ctx = ExecCtx::new(self.store.clone(), Arc::default(), "test", 4);
            ctx.log = Some(log.clone());
            let seen = ctx.clone();
            let source = source.to_owned();
            let job = Arc::clone(&ctx.job);
            let r = block_on(test_executor().run_job(job, "test".to_owned(), move || {
                run_script(ctx, "test", source)
            }));
            (r, seen)
        }
    }

    fn ok(s: &str) -> Result<String, ExecError> {
        Ok(s.to_owned())
    }

    const CATCH: &str = "const g = (f) => { try { return f(); } \
                         catch (e) { return e.name + ': ' + e.message; } };";

    #[test]
    fn a_live_run_logs_each_op_in_order() {
        let rig = rig();
        let r = rig.run(
            r#"Deno.core.ops.op_rs3_insert('{"a":"1"}');
               const v = Deno.core.ops.op_rs3_get('a');
               const old = Deno.core.ops.op_rs3_delete('a');
               v + old + Deno.core.ops.op_rs3_list()"#,
        );
        assert_eq!(r, ok("11[]"));
        assert_eq!(
            rig.ops(),
            vec![
                op("op_rs3_insert", &[r#"{"a":"1"}"#], Ok(r#"{"a":"1"}"#)),
                op("op_rs3_get", &["a"], Ok("1")),
                op("op_rs3_delete", &["a"], Ok("1")),
                op("op_rs3_list", &[], Ok("[]")),
            ]
        );
    }

    #[test]
    fn a_replay_returns_the_same_value_and_leaves_the_store_alone() {
        let rig = rig();
        let src = r#"Deno.core.ops.op_rs3_insert('{"a":"1"}');
                     Deno.core.ops.op_rs3_get('a') + Deno.core.ops.op_rs3_list()"#;
        let first = rig.run(src);
        assert!(first.is_ok(), "{first:?}");
        let logged = rig.ops();
        // The key goes away between the runs and must stay away.
        rig.store.remove("a").unwrap();
        assert_eq!(rig.run(src), first);
        assert!(rig.store.get("a").is_err());
        assert_eq!(rig.ops(), logged);
    }

    #[test]
    fn a_replay_is_fully_replayed_only_after_the_last_logged_op() {
        let rig = rig();
        rig.seed(&op("op_rs3_get", &["a"], Ok("1")));
        let (log, _) = rig.log();
        assert!(!log.fully_replayed());
        assert_eq!(
            rig.run_with(&log, "Deno.core.ops.op_rs3_get('a')").0,
            ok("1")
        );
        assert!(log.fully_replayed());
    }

    #[test]
    fn a_logged_write_that_never_reached_the_store_is_not_performed() {
        let rig = rig();
        rig.seed(&op("op_rs3_insert", &[r#"{"a":"1"}"#], Ok(r#"{"a":"1"}"#)));
        rig.seed(&op("op_rs3_delete", &["k"], Ok("v")));
        rig.store.insert("k".into(), "v".into());
        let r = rig
            .run(r#"Deno.core.ops.op_rs3_insert('{"a":"1"}') + Deno.core.ops.op_rs3_delete('k')"#);
        assert_eq!(r, ok(r#"{"a":"1"}v"#));
        assert!(rig.store.get("a").is_err());
        assert_eq!(rig.store.get("k"), Ok("v".to_owned()));
    }

    #[test]
    fn caught_op_errors_replay_with_the_same_name_and_message() {
        let rig = rig();
        let src = format!(
            "{CATCH} [g(() => Deno.core.ops.op_rs3_get('missing')),
                      g(() => Deno.core.ops.op_rs3_delete('missing')),
                      g(() => Deno.core.ops.op_rs3_insert('not json')),
                      g(() => Deno.core.ops.op_rs3_patch('ghost'))].join('|')"
        );
        let first = rig.run(&src).unwrap();
        assert!(
            first.starts_with("Error: key not found: missing|"),
            "{first}"
        );
        assert!(first.contains("|TypeError: malformed JSON"), "{first}");
        assert!(first.ends_with("|Error: key not found: ghost"), "{first}");
        // The keys exist now, but a replay still sees the logged errors.
        for k in ["missing", "ghost"] {
            rig.store.insert(k.into(), "1".into());
        }
        assert_eq!(rig.run(&src), ok(&first));
        assert_eq!(rig.store.get("missing"), Ok("1".to_owned()));
    }

    #[test]
    fn a_nested_patch_replays_with_the_logged_source() {
        let rig = rig();
        rig.store.insert("x".into(), "7".into());
        rig.store
            .insert("inner".into(), "Deno.core.ops.op_rs3_get('x')".into());
        let src = "Deno.core.ops.op_rs3_patch('inner')";
        assert_eq!(rig.run(src), ok("7"));
        assert_eq!(
            rig.ops(),
            vec![
                op(
                    "op_rs3_patch",
                    &["inner"],
                    Ok("Deno.core.ops.op_rs3_get('x')")
                ),
                op("op_rs3_get", &["x"], Ok("7")),
            ]
        );
        rig.store.remove("inner").unwrap();
        rig.store.remove("x").unwrap();
        assert_eq!(rig.run(src), ok("7"));
    }

    #[test]
    fn now_and_random_replay_from_the_log_and_run_without_one() {
        let rig = rig();
        let src = "Deno.core.ops.op_rs3_now() + '|' + Deno.core.ops.op_rs3_random()";
        let first = rig.run(src).unwrap();
        let (now, random) = first.split_once('|').unwrap();
        assert!(now.parse::<u64>().unwrap() > 1_000_000_000_000, "{first}");
        assert!(random.parse::<u64>().is_ok(), "{first}");
        std::thread::sleep(std::time::Duration::from_millis(5));
        assert_eq!(rig.run(src), ok(&first));
        // Without a log both run live.
        let a = crate::js::run_js(&rig.store, src).unwrap();
        let b = crate::js::run_js(&rig.store, src).unwrap();
        assert_ne!(a.split_once('|').unwrap().1, b.split_once('|').unwrap().1);
    }

    #[test]
    fn native_time_and_randomness_are_logged_only_in_a_logged_job() {
        let rig = rig();
        let probe = "[Date.now.toString(), Math.random.toString()].join('|')";
        let native = "function now() { [native code] }|function random() { [native code] }";
        assert_eq!(crate::js::run_js(&rig.store, probe), ok(native));
        let logged = rig.run(probe).unwrap();
        assert!(!logged.contains("[native code]"), "{logged}");
        rig.store.insert("inner".into(), probe.into());
        let nested = rig.run("Deno.core.ops.op_rs3_patch('inner')").unwrap();
        assert!(!nested.contains("[native code]"), "{nested}");
    }

    #[test]
    fn logged_dates_keep_native_behaviour_and_random_stays_below_one() {
        let rig = rig();
        let src = "[new Date(0).toISOString(), Date.UTC(1970, 0, 2), Date.parse('1970-01-02T00:00:00Z'),
                    new Date() instanceof Date, typeof Date(), Date.now() > 1e12,
                    Array.from({ length: 50 }, Math.random).every((r) => r >= 0 && r < 1)].join('|')";
        assert_eq!(
            rig.run(src),
            ok("1970-01-01T00:00:00.000Z|86400000|86400000|true|string|true|true")
        );
    }

    #[test]
    fn a_mismatched_op_diverges_and_refuses_every_later_op() {
        let mismatches = [
            (
                "op_rs3_get",
                "a",
                "Deno.core.ops.op_rs3_list()",
                "op_rs3_list",
            ),
            (
                "op_rs3_get",
                "a",
                "Deno.core.ops.op_rs3_get('b')",
                "op_rs3_get",
            ),
        ];
        for (name, arg, first_op, first_op_name) in mismatches {
            let rig = rig();
            rig.seed(&op(name, &[arg], Ok("1")));
            let (log, _) = rig.log();
            let src = format!(
                r#"{CATCH} [g(() => {first_op}),
                           g(() => Deno.core.ops.op_rs3_insert('{{"w":"1"}}')),
                           g(() => Deno.core.ops.op_rs3_get('a'))].join('|')"#
            );
            let (r, ctx) = rig.run_with(&log, &src);
            assert_eq!(
                r,
                ok(&["Error: replay diverged"; 3].join("|")),
                "{first_op}"
            );
            assert_eq!(log.stopped(), Some(LogStop::Diverged));
            assert!(rig.store.get("w").is_err());
            assert_eq!(rig.ops().len(), 1, "nothing was logged");
            assert!(!ctx.job.host_fault());
            assert_eq!(
                ctx.job.error_lines(),
                [format!(
                    "error: replay diverged in stream {STREAM}: op {first_op_name:?}"
                )]
            );
        }
    }

    #[test]
    fn a_stale_fence_refuses_live_ops_without_a_host_fault() {
        let rig = rig();
        let (log, fence) = rig.log();
        claim(&rig.db, fence.task).unwrap();
        let src = format!(
            r#"{CATCH} [g(() => Deno.core.ops.op_rs3_insert('{{"w":"1"}}')),
                       g(() => Deno.core.ops.op_rs3_get('w')),
                       g(() => Deno.core.ops.op_rs3_list())].join('|')"#
        );
        let (r, ctx) = rig.run_with(&log, &src);
        let first = r.unwrap();
        assert_eq!(first.split('|').count(), 3);
        assert!(
            first.split('|').all(|p| p.starts_with("Error: ")),
            "{first}"
        );
        assert_eq!(log.stopped(), Some(LogStop::Fenced));
        assert!(rig.store.get("w").is_err());
        assert!(rig.ops().is_empty());
        assert!(!ctx.job.host_fault());
        assert_eq!(ctx.job.error_log_count(), 0);
        // The ended record is refused as well.
        assert!(matches!(
            log.end(Ok("x".into())),
            Err(JournalError::Fenced { .. })
        ));
        assert!(rig.ops().is_empty());
    }

    #[test]
    fn a_stale_fence_on_a_read_logs_nothing() {
        let rig = rig();
        rig.store.insert("k".into(), "v".into());
        let (log, fence) = rig.log();
        claim(&rig.db, fence.task).unwrap();
        let (r, _) = rig.run_with(&log, "Deno.core.ops.op_rs3_get('k')");
        assert!(r.is_err(), "{r:?}");
        assert!(rig.ops().is_empty());
    }

    #[test]
    fn a_timed_out_logged_job_logs_nothing_after_the_timeout() {
        let rig = rig();
        rig.store.insert("loop".into(), "while(true){}".into());
        let exec = dedicated_executor(JsSettings {
            workers: 1,
            queue_capacity: 4,
            timeout_ms: 300,
            ..JsSettings::default()
        });
        let (log, _) = rig.log();
        let src = r#"try { Deno.core.ops.op_rs3_patch('loop') } catch (e) {}
                     Deno.core.ops.op_rs3_insert('{"after":"1"}'); 'done'"#;
        let r = block_on(exec.execute_logged(
            rig.store.clone(),
            "test".to_owned(),
            src.to_owned(),
            log,
        ));
        assert_eq!(r, Err(ExecError::Timeout { limit_ms: 300 }));
        assert!(rig.store.get("after").is_err());
        assert_eq!(
            rig.ops(),
            vec![op("op_rs3_patch", &["loop"], Ok("while(true){}"))]
        );
    }

    #[test]
    fn a_failed_log_append_stops_the_job_as_internal_and_writes_nothing() {
        let rig = rig();
        let (log, _) = rig.log();
        // Another writer appended to the stream, so this log's sequence is stale.
        rig.seed(&op("op_rs3_get", &["z"], Ok("1")));
        let src = r#"try { Deno.core.ops.op_rs3_insert('{"w":"1"}') } catch (e) {}
                     'carried on'"#;
        let r = block_on(test_executor().execute_logged(
            rig.store.clone(),
            "test".to_owned(),
            src.to_owned(),
            log,
        ));
        assert!(matches!(r, Err(ExecError::Internal(_))), "{r:?}");
        assert!(rig.store.get("w").is_err());
        assert_eq!(rig.ops().len(), 1);
    }

    #[test]
    fn end_appends_the_outcome_under_the_fence() {
        let rig = rig();
        let (log, _) = rig.log();
        log.end(Err(ScriptFailure::JsError)).unwrap();
        assert_eq!(
            rig.ops(),
            vec![Record::ScriptEnd {
                outcome: Err(ScriptFailure::JsError)
            }]
        );
        assert!(log.end(Ok("again".into())).is_err());
    }

    #[test]
    fn execute_logged_runs_a_script_and_logs_it() {
        let rig = rig();
        let (log, _) = rig.log();
        let exec: JsExecutor = test_executor();
        let r = block_on(exec.execute_logged(
            rig.store.clone(),
            "test".to_owned(),
            r#"Deno.core.ops.op_rs3_insert('{"a":"1"}'); 'ok'"#.to_owned(),
            log,
        ));
        assert_eq!(r, ok("ok"));
        assert_eq!(rig.store.get("a"), Ok("1".to_owned()));
        assert_eq!(rig.ops().len(), 1);
    }
}
