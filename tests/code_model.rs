//! Coding goals on the real Qwen3-0.6B model, through the real binary. The goal that reaches
//! `success` takes minutes on a debug build, so `scripts/ci-smoke.sh` runs it on the release binary.
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
        let deadline = Instant::now() + Duration::from_secs(120);
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
    fn a_budget_that_leaves_no_room_for_the_prompt_ends_llm_error() {
        let dir = TempDir::new("code-model-overflow");
        let server = serve(&dir);
        // The prompt alone is over 100 tokens, so a 4096 budget cannot fit in the context.
        let got = solve_add(&server, 2, 1, 4096);
        assert_eq!(got["outcome"], "llm_error", "{got}");
    }
}
