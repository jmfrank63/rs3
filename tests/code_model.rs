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

    const TASK: &str = "Implement the function add.";
    const SPEC: &str = "add(a: number, b: number): number returns the sum of a and b";

    /// Create the add goal with `budget` and one iteration; its id.
    fn create(server: &Server, budget: u64) -> u64 {
        let body = json!({
            "kind": "code", "task": TASK, "spec": SPEC,
            "max_iterations": 1, "seed": 1, "budget": budget,
        });
        let (status, text) = server.request_status("POST", "/goals", &body.to_string());
        assert_eq!(status, 201, "{text}");
        serde_json::from_str::<Value>(&text).unwrap()["id"]
            .as_u64()
            .unwrap()
    }

    /// Poll the goal until `ready` holds; fail after two minutes.
    fn wait(server: &Server, id: u64, ready: impl Fn(&Value) -> bool) -> Value {
        let deadline = Instant::now() + Duration::from_secs(120);
        loop {
            let got: Value =
                serde_json::from_str(&server.request("GET", &format!("/goals/{id}"), "")).unwrap();
            if ready(&got) {
                return got;
            }
            assert!(Instant::now() < deadline, "goal {id} timed out: {got}");
            std::thread::sleep(Duration::from_millis(200));
        }
    }

    #[test]
    fn a_model_call_runs_only_if_its_prompt_and_budget_fit_the_context() {
        let (gguf, tokenizer) = model_files();
        let candle = rs3::model::Candle::load(&gguf, &tokenizer, 2).unwrap();
        let prompt = rs3::code::prompt(TASK, SPEC, &[], false);
        let room = 4096 - candle.encode(&prompt).unwrap().len() as u64;
        let dir = TempDir::new("code-model-fit");
        let server = serve(&dir);

        // One token too many: the call cannot be made, and the goal ends at once.
        let over = create(&server, room + 1);
        let got = wait(&server, over, |g| g["status"] == "done");
        assert_eq!(got["outcome"], "llm_error", "{got}");
        assert_eq!(got["iterations"], json!([]), "{got}");

        // An exact fit: the model is called. Cancel the goal, since a full reply takes minutes.
        let fits = create(&server, room);
        let started = wait(&server, fits, |g| {
            g["status"] == "done" || g["iterations"].as_array().is_some_and(|i| !i.is_empty())
        });
        assert_ne!(started["outcome"], "llm_error", "{started}");
        assert_eq!(started["iterations"][0]["prompt"], prompt, "{started}");
        server.request("DELETE", &format!("/goals/{fits}"), "");
    }
}
