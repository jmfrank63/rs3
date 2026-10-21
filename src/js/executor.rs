//! A bounded pool of dedicated JS threads, so actix workers never run V8.
//! This module is free of actix-web.
use std::sync::Arc;

use tokio::sync::{Mutex, mpsc, oneshot};

use super::ops::ExecCtx;
use super::runtime::run_script;
use super::{ExecError, JobState, JsSettings};
use crate::store::Store;

type Reply = Result<String, ExecError>;
type Job = Box<dyn FnOnce() -> Reply + Send + 'static>;

struct Envelope {
    run: Job,
    reply: oneshot::Sender<Reply>,
}

/// The per-job limits the executor applies, taken from `JsSettings` once.
#[derive(Clone, Copy, Debug)]
struct Limits {
    /// PATCH levels one job may run, counting the PATCHed key.
    max_patch_depth: u32,
}

impl Limits {
    fn from_settings(settings: &JsSettings) -> Limits {
        Limits {
            max_patch_depth: settings.max_patch_depth,
        }
    }
}

/// Handle to the JS thread pool. Clones are cheap and share the same pool;
/// the workers exit once every handle is dropped.
#[derive(Clone, Debug)]
pub struct JsExecutor {
    tx: mpsc::Sender<Envelope>,
    limits: Limits,
}

impl JsExecutor {
    /// Spawn `settings.workers` threads named `rs3-js-{i}` with
    /// `settings.stack_bytes` of stack, each running its own tokio
    /// current_thread runtime. Zero workers or zero queue capacity are clamped
    /// to 1. Returns once every worker has built its runtime, or the first
    /// spawn or runtime-build error.
    pub fn start(settings: &JsSettings) -> std::io::Result<JsExecutor> {
        let workers = settings.workers.max(1);
        let capacity = settings.queue_capacity.max(1);
        let (tx, rx) = mpsc::channel(capacity);
        let rx = Arc::new(Mutex::new(rx));
        let (ready_tx, ready_rx) = std::sync::mpsc::channel::<std::io::Result<()>>();
        for i in 0..workers {
            let name = format!("rs3-js-{i}");
            let rx = Arc::clone(&rx);
            let ready = ready_tx.clone();
            std::thread::Builder::new()
                .name(name.clone())
                .stack_size(settings.stack_bytes)
                .spawn(move || {
                    let built = tokio::runtime::Builder::new_current_thread()
                        .enable_all()
                        .build();
                    match built {
                        Ok(rt) => {
                            // start() may already have returned on another worker's error.
                            let _ = ready.send(Ok(()));
                            drop(ready);
                            worker_loop(&rx, &rt);
                        }
                        Err(e) => {
                            let _ = ready.send(Err(e));
                        }
                    }
                })
                .map_err(|e| {
                    std::io::Error::new(e.kind(), format!("cannot spawn JS worker {name}: {e}"))
                })?;
        }
        drop(ready_tx);
        for _ in 0..workers {
            match ready_rx.recv() {
                Ok(Ok(())) => {}
                Ok(Err(e)) => {
                    return Err(std::io::Error::new(
                        e.kind(),
                        format!("cannot build JS worker runtime: {e}"),
                    ));
                }
                Err(_) => {
                    return Err(std::io::Error::other(
                        "JS worker exited before reporting ready",
                    ));
                }
            }
        }
        Ok(JsExecutor {
            tx,
            limits: Limits::from_settings(settings),
        })
    }

    /// Run `job` on a JS worker with its tokio runtime entered and return its
    /// result. Waits while the queue is full. A panicking job answers
    /// `ExecError::Internal`; a closed pool answers `ExecError::Unavailable`.
    pub async fn run<F>(&self, job: F) -> Result<String, ExecError>
    where
        F: FnOnce() -> Result<String, ExecError> + Send + 'static,
    {
        let (reply, reply_rx) = oneshot::channel();
        self.tx
            .send(Envelope {
                run: Box::new(job),
                reply,
            })
            .await
            .map_err(|_| ExecError::Unavailable)?;
        reply_rx.await.map_err(|_| ExecError::Unavailable)?
    }

    /// Run `source` as a script named `name` against `store`. The runtime is
    /// created and dropped on the worker; only the owned result crosses back.
    /// Every level of the job shares one `JobState`, and the result is
    /// classified from it once, by `classify`.
    pub async fn execute(
        &self,
        store: Store,
        name: String,
        source: String,
    ) -> Result<String, ExecError> {
        let limits = self.limits;
        self.run(move || {
            let job = Arc::new(JobState::default());
            let ctx = ExecCtx::new(store, Arc::clone(&job), &name, limits.max_patch_depth);
            classify(&job, run_script(ctx, &name, source))
        })
        .await
    }

    /// An executor whose pool is already gone: every call answers
    /// `ExecError::Unavailable`.
    #[cfg(test)]
    pub(crate) fn closed() -> JsExecutor {
        let (tx, rx) = mpsc::channel(1);
        drop(rx);
        JsExecutor {
            tx,
            limits: Limits::from_settings(&JsSettings::default()),
        }
    }
}

/// The one classification point of a job's result, applied after the run.
/// The precedence is D-04: a host fault (500 internal) beats a depth stop
/// (422 js_depth_limit), which beats the script's own result. Every decision
/// is taken from the job's flags, never from a message, so a stop answers the
/// same whether or not the script caught the error. Nested `run_script` calls
/// never classify.
fn classify(job: &JobState, result: Reply) -> Reply {
    match result {
        Err(ExecError::Internal(_)) => result,
        _ if job.host_fault() => Err(ExecError::Internal("a nested JS runtime failed".into())),
        _ => match job.depth_stop() {
            Some(stop) => Err(ExecError::DepthLimit {
                limit: stop.limit,
                chain: stop.chain.clone(),
            }),
            None => result,
        },
    }
}

/// Take jobs until every sender is dropped. Each job runs under catch_unwind
/// with the tokio runtime entered, so a panicking job cannot kill the worker
/// and every JsRuntime is created inside a tokio context.
fn worker_loop(rx: &Mutex<mpsc::Receiver<Envelope>>, rt: &tokio::runtime::Runtime) {
    loop {
        let Some(env) = rt.block_on(async { rx.lock().await.recv().await }) else {
            return;
        };
        let result = {
            // The guard must be dropped before the next block_on.
            let _enter = rt.enter();
            std::panic::catch_unwind(std::panic::AssertUnwindSafe(env.run))
                .unwrap_or_else(|_| Err(ExecError::Internal("js worker panicked".into())))
        };
        // The caller may have gone away; that is fine.
        let _ = env.reply.send(result);
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::time::Duration;

    use tokio::sync::mpsc::error::TrySendError;
    use tokio::sync::oneshot;

    use super::{Envelope, JsExecutor, Limits, classify};
    use crate::js::{
        DepthStop, ExecError, JobState, JsSettings, block_on, init_platform, test_executor,
    };
    use crate::store::Store;

    const MIB16: usize = 16 * 1024 * 1024;

    fn dedicated(workers: usize, queue_capacity: usize) -> JsExecutor {
        JsExecutor::start(&JsSettings {
            workers,
            queue_capacity,
            stack_bytes: MIB16,
            ..JsSettings::default()
        })
        .expect("dedicated executor")
    }

    #[test]
    fn job_runs_on_a_named_js_thread() {
        let name = block_on(
            test_executor().run(|| Ok(std::thread::current().name().unwrap_or("").to_owned())),
        )
        .expect("job result");
        assert!(name.starts_with("rs3-js-"), "ran on {name:?}");
    }

    #[test]
    fn worker_survives_panicking_jobs() {
        let exec = dedicated(2, 4);
        for _ in 0..3 {
            let r = block_on(
                exec.run(|| -> Result<String, ExecError> { panic!("deliberate job panic") }),
            );
            match r {
                Err(ExecError::Internal(m)) => assert!(m.contains("panicked"), "{m}"),
                other => panic!("expected Internal, got {other:?}"),
            }
        }
        for i in 0..4 {
            assert_eq!(
                block_on(exec.run(move || Ok(format!("ok {i}")))),
                Ok(format!("ok {i}"))
            );
        }
    }

    #[test]
    fn execute_runs_a_script_against_its_store() {
        let store = Store::new();
        store.insert("num".into(), "5".into());
        assert_eq!(
            block_on(test_executor().execute(
                store,
                "t".into(),
                "Number(Deno.core.ops.op_rs3_get('num')) * 24".into()
            )),
            Ok("120".to_owned())
        );
    }

    #[test]
    fn stores_on_one_executor_are_isolated() {
        let a = Store::new();
        a.insert("num".into(), "1".into());
        let b = Store::new();
        b.insert("num".into(), "2".into());
        b.insert("only_b".into(), "yes".into());
        let src = "Deno.core.ops.op_rs3_get('num') + '|' + (() => { \
                   try { return Deno.core.ops.op_rs3_get('only_b'); } \
                   catch (e) { return e.message; } })()";
        let exec = test_executor();
        std::thread::scope(|s| {
            for i in 0..8 {
                let (store, want) = if i % 2 == 0 {
                    (a.clone(), "1|key not found: only_b")
                } else {
                    (b.clone(), "2|yes")
                };
                let exec = exec.clone();
                s.spawn(move || {
                    let got = block_on(exec.execute(store, format!("job{i}"), src.to_owned()));
                    assert_eq!(got, Ok(want.to_owned()), "job {i}");
                });
            }
        });
    }

    #[test]
    fn full_queue_makes_callers_wait() {
        let exec = dedicated(1, 1);
        std::thread::scope(|s| {
            // Created inside the scope so a failed assertion drops gate_tx,
            // which unblocks job 0 and lets the scope finish instead of hanging.
            let (started_tx, started_rx) = std::sync::mpsc::channel::<()>();
            let (gate_tx, gate_rx) = std::sync::mpsc::channel::<()>();
            // Job 0 occupies the only worker until the gate opens.
            let first = {
                let exec = exec.clone();
                s.spawn(move || {
                    block_on(exec.run(move || {
                        started_tx.send(()).expect("signal started");
                        gate_rx.recv().expect("gate released");
                        Ok("job 0".to_owned())
                    }))
                })
            };
            started_rx.recv().expect("job 0 started");
            // Job 1 takes the single queue slot.
            let (reply1, reply1_rx) = oneshot::channel();
            let queued = exec.tx.try_send(Envelope {
                run: Box::new(|| Ok("job 1".to_owned())),
                reply: reply1,
            });
            assert!(queued.is_ok(), "the queue slot was not free");
            // The queue is now full: a send that cannot wait is refused.
            let (probe, _probe_rx) = oneshot::channel();
            let refused = exec.tx.try_send(Envelope {
                run: Box::new(|| Ok(String::new())),
                reply: probe,
            });
            assert!(
                matches!(refused, Err(TrySendError::Full(_))),
                "queue accepted more than its capacity"
            );
            // A run() caller waits for room instead of enqueueing. Abandoning
            // it after a short wait must therefore leave its job unqueued.
            let ran = Arc::new(AtomicBool::new(false));
            let flag = Arc::clone(&ran);
            let abandoned = block_on(async {
                let job = exec.run(move || {
                    flag.store(true, Ordering::SeqCst);
                    Ok("abandoned".to_owned())
                });
                tokio::time::timeout(Duration::from_millis(50), job).await
            });
            assert!(
                abandoned.is_err(),
                "run() returned while the worker was blocked"
            );
            // A caller that keeps waiting is admitted once the worker frees up.
            let waiter = {
                let exec = exec.clone();
                s.spawn(move || block_on(exec.run(|| Ok("job 2".to_owned()))))
            };
            gate_tx.send(()).expect("release gate");
            assert_eq!(first.join().expect("job 0 thread"), Ok("job 0".to_owned()));
            assert_eq!(
                block_on(reply1_rx).expect("job 1 reply"),
                Ok("job 1".to_owned())
            );
            assert_eq!(waiter.join().expect("job 2 thread"), Ok("job 2".to_owned()));
            // One worker runs jobs in queue order, so had the abandoned job been
            // queued it would have run before this one.
            assert_eq!(
                block_on(exec.run(|| Ok("job 3".to_owned()))),
                Ok("job 3".to_owned())
            );
            assert!(
                !ran.load(Ordering::SeqCst),
                "the abandoned job was queued although the queue was full"
            );
        });
    }

    #[test]
    fn closed_executor_is_unavailable() {
        assert_eq!(
            block_on(JsExecutor::closed().execute(Store::new(), "t".into(), "1".into())),
            Err(ExecError::Unavailable)
        );
    }

    #[test]
    fn zero_settings_are_clamped() {
        init_platform();
        let exec = dedicated(0, 0);
        assert_eq!(
            block_on(exec.execute(Store::new(), "t".into(), "6 * 7".into())),
            Ok("42".to_owned())
        );
    }

    #[test]
    fn every_job_gets_a_fresh_runtime() {
        let store = Store::new();
        let exec = test_executor();
        for _ in 0..2 {
            assert_eq!(
                block_on(exec.execute(store.clone(), "t".into(), "let q = 1; q".into())),
                Ok("1".to_owned())
            );
        }
    }

    #[test]
    fn max_patch_depth_is_clamped() {
        for (given, want) in [(0, 1), (1, 1), (4, 4), (64, 64), (65, 64), (u32::MAX, 64)] {
            let settings = JsSettings {
                max_patch_depth: given,
                ..JsSettings::default()
            };
            assert_eq!(
                Limits::from_settings(&settings).max_patch_depth,
                want,
                "max_patch_depth {given}"
            );
        }
    }

    #[test]
    fn classify_precedence_host_fault_then_depth_then_result() {
        let stop = || DepthStop {
            limit: 4,
            chain: vec!["a".to_owned(), "b".to_owned()],
        };
        let depth = || {
            Err(ExecError::DepthLimit {
                limit: 4,
                chain: vec!["a".to_owned(), "b".to_owned()],
            })
        };
        let script = || {
            Err(ExecError::Script {
                message: "Uncaught Error: x".to_owned(),
            })
        };
        let internal = |m: &str| Err(ExecError::Internal(m.to_owned()));
        // Host fault beats depth stop beats the result.
        let both = JobState::default();
        both.note_host_fault();
        both.stop_depth(stop());
        assert!(matches!(
            classify(&both, Ok("x".to_owned())),
            Err(ExecError::Internal(_))
        ));
        // Depth stop alone overrides a value and a script error.
        let stopped = JobState::default();
        stopped.stop_depth(stop());
        assert_eq!(classify(&stopped, Ok("x".to_owned())), depth());
        assert_eq!(classify(&stopped, script()), depth());
        // An Internal result stays Internal, with its own detail.
        assert_eq!(classify(&stopped, internal("boom")), internal("boom"));
        // Neither flag: the result is unchanged.
        let clean = JobState::default();
        assert_eq!(classify(&clean, Ok("x".to_owned())), Ok("x".to_owned()));
        assert_eq!(classify(&clean, script()), script());
        assert_eq!(classify(&clean, internal("boom")), internal("boom"));
    }

    #[test]
    fn default_settings_follow_d06() {
        let s = JsSettings::default();
        assert!(s.workers >= 1);
        assert_eq!(s.queue_capacity, s.workers * 16);
        assert_eq!(s.stack_bytes, 16 * 1024 * 1024);
    }
}
