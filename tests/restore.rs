//! A real-model node killed with SIGKILL mid-goal finishes the goal as an uninterrupted run would.
#[cfg(test)]
mod common;
#[cfg(test)]
#[path = "common/model.rs"]
mod model;

#[cfg(test)]
mod tests {
    use std::time::{Duration, Instant};

    use crate::common::{Server, TempDir};
    use crate::model::model_files;

    const BUDGET: u64 = 48;
    const PROMPT: &str =
        "List twenty different animals, one per line, with a short description of each. /no_think";

    fn serve(dir: &TempDir) -> Server {
        let (gguf, tokenizer) = model_files();
        let db = dir.0.join("rs3.redb");
        Server::start(
            &dir.0,
            Some(&db),
            &[
                ("MODEL.PATH", gguf.to_str().unwrap()),
                ("MODEL.TOKENIZER", tokenizer.to_str().unwrap()),
                ("MODEL.THREADS", "2"),
            ],
        )
    }

    /// Poll `f` until it returns `Some`; fail after `deadline`.
    fn wait_until<T>(what: &str, deadline: Duration, mut f: impl FnMut() -> Option<T>) -> T {
        let end = Instant::now() + deadline;
        loop {
            if let Some(found) = f() {
                return found;
            }
            assert!(Instant::now() < end, "timed out waiting for {what}");
            std::thread::sleep(Duration::from_millis(50));
        }
    }

    fn goal(server: &Server) -> serde_json::Value {
        serde_json::from_str(&server.request("GET", "/goals/1", "")).unwrap()
    }

    fn post(server: &Server) {
        let body = serde_json::json!({
            "priority": 0, "budget": BUDGET, "seed": 11, "prompt": PROMPT
        })
        .to_string();
        let (status, text) = server.request_status("POST", "/goals", &body);
        assert_eq!(status, 201, "{text}");
    }

    fn done(server: &Server) -> serde_json::Value {
        wait_until("the goal to finish", Duration::from_secs(120), || {
            let got = goal(server);
            (got["status"] == "done").then_some(got)
        })
    }

    #[test]
    fn a_goal_killed_mid_generation_finishes_identically_and_samples_only_uncommitted_tokens() {
        let reference_dir = TempDir::new("restore-ref");
        let reference = {
            let server = serve(&reference_dir);
            post(&server);
            let got = done(&server);
            assert_eq!(
                got["reason"], "budget",
                "prompt must run to the budget: {got}"
            );
            got
        };

        let dir = TempDir::new("restore-crash");
        let db = dir.0.join("rs3.redb");
        let server = serve(&dir);
        post(&server);
        wait_until("some output", Duration::from_secs(120), || {
            let got = goal(&server);
            (got["status"] != "done" && !got["output"].as_str().unwrap_or("").is_empty())
                .then_some(())
        });
        drop(server); // SIGKILL, then wait for the process to exit.

        let committed = {
            let journal = rs3::journal::Journal::new(rs3::storage::open(&db).unwrap()).unwrap();
            let state = journal.restore(1).unwrap().expect("goal stream exists");
            assert!(
                state.ended.is_none(),
                "the kill landed after the goal ended"
            );
            state.tokens.len() as u64
        };
        assert!(
            0 < committed && committed < BUDGET,
            "committed {committed} of {BUDGET}"
        );

        let server = serve(&dir);
        let got = done(&server);
        assert_eq!(got["output"], reference["output"], "C={committed}");
        assert_eq!(got["reason"], reference["reason"]);
        // The first run's sampled counter died with its process, and its uncommitted batch was
        // legitimately sampled again, so the invariant is on the restart alone: it samples exactly
        // the tokens that were never committed.
        let generated: serde_json::Value =
            serde_json::from_str(&server.request("GET", "/goals/generated", "")).unwrap();
        assert_eq!(
            generated["generated"].as_u64(),
            Some(BUDGET - committed),
            "N={BUDGET} C={committed} generated={generated}"
        );
    }
}
