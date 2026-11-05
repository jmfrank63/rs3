//! Goals through the real binary: they survive kill -9 and finish byte-identical.
#[cfg(test)]
mod common;

#[cfg(test)]
mod tests {
    use std::process::Command;
    use std::time::{Duration, Instant};

    use serde_json::Value;

    use crate::common::{Server, TempDir};

    const BUDGET: usize = 1000;
    const RATE: &[(&str, &str)] = &[("MODEL.TOKENS_PER_SECOND", "500")];

    fn create(server: &Server, seed: u64, budget: usize) -> u64 {
        create_with_priority(server, 0, seed, budget)
    }

    fn create_with_priority(server: &Server, priority: u8, seed: u64, budget: usize) -> u64 {
        let body =
            format!(r#"{{"priority":{priority},"budget":{budget},"seed":{seed},"prompt":"p"}}"#);
        let created: Value =
            serde_json::from_str(&server.request("POST", "/goals", &body)).unwrap();
        created["id"].as_u64().unwrap()
    }

    fn get(server: &Server, id: u64) -> Value {
        serde_json::from_str(&server.request("GET", &format!("/goals/{id}"), "")).unwrap()
    }

    fn output(goal: &Value) -> String {
        goal["output"].as_str().unwrap().to_owned()
    }

    fn wait_until(server: &Server, id: u64, what: &str, ready: impl Fn(&Value) -> bool) -> Value {
        let deadline = Instant::now() + Duration::from_secs(30);
        loop {
            let goal = get(server, id);
            if ready(&goal) {
                return goal;
            }
            assert!(
                Instant::now() < deadline,
                "goal {id} never became {what}: {goal}"
            );
            std::thread::sleep(Duration::from_millis(20));
        }
    }

    fn wait_done(server: &Server, id: u64) -> Value {
        wait_until(server, id, "done", |goal| goal["status"] == "done")
    }

    fn has_output(goal: &Value) -> bool {
        !goal["output"].as_str().unwrap().is_empty()
    }

    fn words(goal: &Value) -> usize {
        output(goal).split_whitespace().count()
    }

    /// The output of goals run to the end on an undisturbed, fast server.
    fn reference(name: &str, goals: &[(u64, usize)]) -> Vec<String> {
        let dir = TempDir::new(name);
        let server = Server::start(&dir.0, Some(&dir.0.join("rs3.db")), &[]);
        let ids: Vec<u64> = goals.iter().map(|(s, b)| create(&server, *s, *b)).collect();
        ids.iter()
            .map(|id| output(&wait_done(&server, *id)))
            .collect()
    }

    #[test]
    fn goals_killed_with_kill_9_finish_byte_identical_to_an_undisturbed_run() {
        let seeds = [11, 22];
        let reference = TempDir::new("goals-ref");
        let expected: Vec<String> = {
            let server = Server::start(&reference.0, Some(&reference.0.join("rs3.db")), RATE);
            let ids: Vec<u64> = seeds.iter().map(|s| create(&server, *s, BUDGET)).collect();
            ids.iter()
                .map(|id| output(&wait_done(&server, *id)))
                .collect()
        };
        assert_ne!(expected[0], expected[1]);

        let dir = TempDir::new("goals-kill");
        let db = dir.0.join("rs3.db");
        let mut server = Server::start(&dir.0, Some(&db), RATE);
        let ids: Vec<u64> = seeds.iter().map(|s| create(&server, *s, BUDGET)).collect();
        let mut seen = vec![String::new(); ids.len()];
        let mut state = 0x2545_F491_4F6C_DD1D_u64;
        let mut unfinished_at_last_kill = false;
        for _ in 0..5 {
            state ^= state << 13;
            state ^= state >> 7;
            state ^= state << 17;
            std::thread::sleep(Duration::from_millis(20 + state % 131));
            drop(server);
            server = Server::start(&dir.0, Some(&db), RATE);
            unfinished_at_last_kill = false;
            for (i, id) in ids.iter().enumerate() {
                let goal = get(&server, *id);
                let out = output(&goal);
                assert!(
                    out.starts_with(&seen[i]),
                    "output lost across a kill: {goal}"
                );
                assert!(expected[i].starts_with(&out), "output diverged: {goal}");
                unfinished_at_last_kill |= goal["status"] != "done";
                seen[i] = out;
            }
        }
        assert!(
            unfinished_at_last_kill,
            "the run finished before the last kill"
        );
        assert!(
            seen.iter().any(|out| !out.is_empty()),
            "no output was committed across the kills"
        );

        for (i, id) in ids.iter().enumerate() {
            let goal = wait_done(&server, *id);
            assert_eq!(output(&goal), expected[i]);
            assert_eq!(output(&goal).split_whitespace().count(), BUDGET);
        }
    }

    #[test]
    fn a_goal_resumes_and_finishes_after_a_graceful_stop() {
        let dir = TempDir::new("goals-term");
        let db = dir.0.join("rs3.db");
        let mut server = Server::start(&dir.0, Some(&db), RATE);
        let id = create(&server, 5, 600);
        let pid = server.child.id().to_string();
        assert!(
            Command::new("kill")
                .args(["-TERM", &pid])
                .status()
                .unwrap()
                .success()
        );
        assert_eq!(server.child.wait().unwrap().code(), Some(0));

        let server = Server::start(&dir.0, Some(&db), RATE);
        let goal = wait_done(&server, id);
        assert_eq!(output(&goal).split_whitespace().count(), 600);
    }

    #[test]
    fn a_higher_goal_takes_the_only_worker_and_both_outputs_are_correct() {
        let expected = reference("ref-preempt", &[(1, 400), (2, 80)]);
        let dir = TempDir::new("goals-preempt");
        let env = [
            ("SCHEDULER.WORKERS", "1"),
            ("MODEL.TOKENS_PER_SECOND", "100"),
        ];
        let server = Server::start(&dir.0, Some(&dir.0.join("rs3.db")), &env);
        let low = create_with_priority(&server, 1, 1, 400);
        wait_until(&server, low, "started", has_output);
        let high = create_with_priority(&server, 5, 2, 80);
        wait_until(&server, low, "paused", |goal| goal["status"] == "paused");
        wait_done(&server, high);
        let mid = get(&server, low);
        assert_ne!(mid["status"], "done", "{mid}");
        assert!((1..400).contains(&words(&mid)), "{mid}");

        let low_goal = wait_done(&server, low);
        assert_eq!(output(&low_goal), expected[0]);
        assert_eq!(words(&low_goal), 400);
        let high_goal = get(&server, high);
        assert_eq!(output(&high_goal), expected[1]);
        assert_eq!(words(&high_goal), 80);
    }

    #[test]
    fn a_cancelled_goal_stays_cancelled_with_its_output_across_kill_9() {
        let expected = reference("ref-cancel", &[(1, 400), (2, 60)]);
        let dir = TempDir::new("goals-cancel");
        let db = dir.0.join("rs3.db");
        let env = [("MODEL.TOKENS_PER_SECOND", "100")];
        let mut server = Server::start(&dir.0, Some(&db), &env);
        let doomed = create(&server, 1, 400);
        wait_until(&server, doomed, "started", has_output);
        let other = create(&server, 2, 60);
        let cancelled: Value =
            serde_json::from_str(&server.request("DELETE", &format!("/goals/{doomed}"), ""))
                .unwrap();
        assert_eq!(cancelled["status"], "cancelled");
        let kept = output(&cancelled);
        assert!(expected[0].starts_with(&kept), "{cancelled}");
        std::thread::sleep(Duration::from_millis(200));
        assert_eq!(output(&get(&server, doomed)), kept);

        drop(server);
        server = Server::start(&dir.0, Some(&db), &env);
        std::thread::sleep(Duration::from_millis(200));
        let after = get(&server, doomed);
        assert_eq!(after["status"], "cancelled");
        assert_eq!(output(&after), kept);
        assert_eq!(output(&wait_done(&server, other)), expected[1]);
    }
}
