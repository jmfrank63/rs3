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
        let body = format!(r#"{{"priority":0,"budget":{budget},"seed":{seed},"prompt":"p"}}"#);
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

    fn wait_done(server: &Server, id: u64) -> Value {
        let deadline = Instant::now() + Duration::from_secs(30);
        loop {
            let goal = get(server, id);
            if goal["status"] == "done" {
                return goal;
            }
            assert!(
                Instant::now() < deadline,
                "goal {id} never finished: {goal}"
            );
            std::thread::sleep(Duration::from_millis(20));
        }
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
}
