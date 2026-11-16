//! Coding goals on the real Qwen3-0.6B model, through the real binary.
#[cfg(test)]
mod common;
#[cfg(test)]
#[path = "common/model.rs"]
mod model;

#[cfg(test)]
mod tests {
    use std::time::{Duration, Instant};

    use serde_json::{Value, json};

    use crate::common::{Server, TempDir};
    use crate::model::model_files;

    // Found by scanning seeds 1..7 at budget 1024 and 5 iterations on the release binary: seed 4
    // reaches "success" in 2 iterations (the last one's single test passes), 2 to 7 minutes
    // depending on the build and the machine load. Seeds 1, 2, 3, 5, 6 and 7 end max_iterations or llm_malformed.
    const SEED: u64 = 4;
    const MAX_ITERATIONS: u64 = 5;
    const BUDGET: u64 = 1024;

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

    fn solve_add(server: &Server, max_iterations: u64, seed: u64, budget: u64) -> Value {
        let body = json!({
            "kind": "code",
            "task": "Implement the function add.",
            "spec": "add(a: number, b: number): number returns the sum of a and b",
            "max_iterations": max_iterations,
            "seed": seed,
            "budget": budget,
        });
        let (status, text) = server.request_status("POST", "/goals", &body.to_string());
        assert_eq!(status, 201, "{text}");
        let id = serde_json::from_str::<Value>(&text).unwrap()["id"]
            .as_u64()
            .unwrap();
        let deadline = Instant::now() + Duration::from_secs(900);
        loop {
            let got: Value =
                serde_json::from_str(&server.request("GET", &format!("/goals/{id}"), "")).unwrap();
            if got["status"] == "done" {
                return got;
            }
            assert!(Instant::now() < deadline, "goal {id} never finished: {got}");
            std::thread::sleep(Duration::from_millis(200));
        }
    }

    #[test]
    fn a_coding_goal_runs_on_the_model_and_is_the_same_on_two_fresh_nodes() {
        // The nodes run side by side: the debug build needs minutes per goal.
        let bodies: Vec<Value> = std::thread::scope(|scope| {
            let nodes: Vec<_> = ["code-model-a", "code-model-b"]
                .map(|name| {
                    scope.spawn(move || {
                        solve_add(&serve(&TempDir::new(name)), MAX_ITERATIONS, SEED, BUDGET)
                    })
                })
                .into();
            nodes.into_iter().map(|n| n.join().unwrap()).collect()
        });
        assert_eq!(bodies[0]["outcome"], "success", "{}", bodies[0]);
        let last = bodies[0]["iterations"].as_array().unwrap().last().unwrap();
        let tests = last["tests"].as_array().unwrap();
        assert!(
            !tests.is_empty() && tests.iter().all(|t| t["passed"] == true),
            "{last}"
        );
        assert_eq!(bodies[0], bodies[1]);
    }

    #[test]
    fn a_budget_that_leaves_no_room_for_the_prompt_ends_llm_error() {
        let dir = TempDir::new("code-model-overflow");
        let server = serve(&dir);
        // The prompt alone is over 100 tokens, so a 4096 budget cannot fit in the context.
        let got = solve_add(&server, 2, 1, 4096);
        assert_eq!(got["outcome"], "llm_error", "{got}");
    }
}
