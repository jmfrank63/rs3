//! A bounded pool of dedicated JS threads, so actix workers never run V8.
//! This module is free of actix-web.
use std::sync::Arc;

use tokio::sync::{Mutex, mpsc, oneshot};

use super::ops::ExecCtx;
use super::runtime::run_script;
use super::{ExecError, JsSettings};
use crate::store::Store;

type Reply = Result<String, ExecError>;
type Job = Box<dyn FnOnce() -> Reply + Send + 'static>;

struct Envelope {
    run: Job,
    reply: oneshot::Sender<Reply>,
}

/// Handle to the JS thread pool. Clones are cheap and share the same pool;
/// the workers exit once every handle is dropped.
#[derive(Clone, Debug)]
pub struct JsExecutor {
    tx: mpsc::Sender<Envelope>,
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
        Ok(JsExecutor { tx })
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
    pub async fn execute(
        &self,
        store: Store,
        name: String,
        source: String,
    ) -> Result<String, ExecError> {
        self.run(move || run_script(ExecCtx::new(store), &name, source))
            .await
    }

    /// An executor whose pool is already gone: every call answers
    /// `ExecError::Unavailable`.
    #[cfg(test)]
    pub(crate) fn closed() -> JsExecutor {
        let (tx, rx) = mpsc::channel(1);
        drop(rx);
        JsExecutor { tx }
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
    use super::JsExecutor;
    use crate::js::{ExecError, JsSettings, block_on, init_platform, test_executor};
    use crate::store::Store;

    const MIB16: usize = 16 * 1024 * 1024;

    fn dedicated(workers: usize, queue_capacity: usize) -> JsExecutor {
        JsExecutor::start(&JsSettings {
            workers,
            queue_capacity,
            stack_bytes: MIB16,
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
        let (started_tx, started_rx) = std::sync::mpsc::channel::<()>();
        let (gate_tx, gate_rx) = std::sync::mpsc::channel::<()>();
        let results: Vec<Result<String, ExecError>> = std::thread::scope(|s| {
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
            let rest: Vec<_> = (1..8)
                .map(|i| {
                    let exec = exec.clone();
                    s.spawn(move || block_on(exec.run(move || Ok(format!("job {i}")))))
                })
                .collect();
            gate_tx.send(()).expect("release gate");
            std::iter::once(first)
                .chain(rest)
                .map(|h| h.join().expect("submitter thread"))
                .collect()
        });
        let want: Vec<Result<String, ExecError>> = (0..8).map(|i| Ok(format!("job {i}"))).collect();
        assert_eq!(results, want);
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
    fn default_settings_follow_d06() {
        let s = JsSettings::default();
        assert!(s.workers >= 1);
        assert_eq!(s.queue_capacity, s.workers * 16);
        assert_eq!(s.stack_bytes, 16 * 1024 * 1024);
    }
}
