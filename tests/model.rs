//! Startup with MODEL.* settings: bad settings stop the node and name the culprit; good ones serve.
#[cfg(test)]
mod common;
#[cfg(test)]
#[path = "common/model.rs"]
mod model;

#[cfg(test)]
mod tests {
    use std::io::Read;
    use std::time::{Duration, Instant};

    use crate::common::{Server, TempDir, free_port, rs3};
    use crate::model::model_files;

    /// Start a node with `env`; it must exit non-zero within the deadline. Returns its stderr.
    fn refused(dir: &TempDir, env: &[(&str, &str)]) -> String {
        let db = dir.0.join("rs3.redb");
        let mut child = rs3(free_port(), &dir.0, Some(&db))
            .envs(env.iter().copied())
            .spawn()
            .unwrap();
        let deadline = Instant::now() + Duration::from_secs(60);
        let status = loop {
            if let Some(status) = child.try_wait().unwrap() {
                break status;
            }
            if Instant::now() > deadline {
                let _ = child.kill();
                let _ = child.wait();
                panic!("node still running after 60s with {env:?}");
            }
            std::thread::sleep(Duration::from_millis(50));
        };
        assert!(!status.success(), "node started with {env:?}");
        let mut stderr = String::new();
        child
            .stderr
            .take()
            .unwrap()
            .read_to_string(&mut stderr)
            .unwrap();
        stderr
    }

    #[test]
    fn a_model_path_without_a_tokenizer_is_refused() {
        let dir = TempDir::new("model-half");
        let (gguf, _) = model_files();
        let stderr = refused(&dir, &[("MODEL.PATH", gguf.to_str().unwrap())]);
        assert!(stderr.contains("invalid configuration"), "{stderr}");
        assert!(stderr.contains("MODEL.TOKENIZER"), "{stderr}");
    }

    #[test]
    fn a_model_file_that_is_not_a_model_is_refused() {
        let dir = TempDir::new("model-bad");
        let (_, tokenizer) = model_files();
        let t = tokenizer.to_str().unwrap();
        let stderr = refused(&dir, &[("MODEL.PATH", t), ("MODEL.TOKENIZER", t)]);
        assert!(stderr.contains("cannot load model"), "{stderr}");
    }

    #[test]
    fn a_node_with_a_valid_model_serves() {
        let dir = TempDir::new("model-ok");
        let (gguf, tokenizer) = model_files();
        let db = dir.0.join("rs3.redb");
        let server = Server::start(
            &dir.0,
            Some(&db),
            &[
                ("MODEL.PATH", gguf.to_str().unwrap()),
                ("MODEL.TOKENIZER", tokenizer.to_str().unwrap()),
                ("MODEL.THREADS", "2"),
            ],
        );
        let body = r#"{"priority":0,"budget":5,"seed":1,"prompt":"p"}"#;
        let created: serde_json::Value =
            serde_json::from_str(&server.request("POST", "/goals", body)).unwrap();
        let id = created["id"].as_u64().unwrap();
        let (status, _) = server.request_status("GET", &format!("/goals/{id}"), "");
        assert_eq!(status, 200);
    }

    /// Poll `f` until it returns `Some`; fail after `deadline`.
    fn wait_until<T>(what: &str, deadline: Duration, mut f: impl FnMut() -> Option<T>) -> T {
        let end = Instant::now() + deadline;
        loop {
            if let Some(found) = f() {
                return found;
            }
            assert!(Instant::now() < end, "timed out waiting for {what}");
            std::thread::sleep(Duration::from_millis(100));
        }
    }

    const FRANCE: &str = "What is the capital of France? Answer with one word. /no_think";

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

    fn ask(server: &Server, seed: u64, budget: u64) -> serde_json::Value {
        let body = serde_json::json!({
            "priority": 0, "budget": budget, "seed": seed, "prompt": FRANCE
        })
        .to_string();
        let created: serde_json::Value =
            serde_json::from_str(&server.request("POST", "/goals", &body)).unwrap();
        let id = created["id"].as_u64().unwrap();
        wait_until("the goal to finish", Duration::from_secs(120), || {
            let got: serde_json::Value =
                serde_json::from_str(&server.request("GET", &format!("/goals/{id}"), "")).unwrap();
            (got["status"] == "done").then_some(got)
        })
    }

    #[test]
    fn a_goal_answers_the_france_prompt_and_ends_at_eos() {
        let dir = TempDir::new("model-france");
        let server = serve(&dir);
        let got = ask(&server, 1, 200);
        assert!(got["output"].as_str().unwrap().contains("Paris"), "{got}");
        assert_eq!(got["reason"], "eos");
        // A budget smaller than the answer ends the goal at the budget.
        let short = ask(&server, 1, 2);
        assert_eq!(short["reason"], "budget", "{short}");
        let (gguf, tokenizer) = model_files();
        let candle = rs3::model::Candle::load(&gguf, &tokenizer, 2).unwrap();
        let tokens = |text: &str| candle.prompt_tokens(text).unwrap().len();
        assert_eq!(tokens(short["output"].as_str().unwrap()) - tokens(""), 2);
        // The context holds exactly 4096 tokens of prompt plus budget: one more is refused.
        let room = 4096 - candle.prompt_tokens(FRANCE).unwrap().len();
        let post = |budget: usize| {
            let body = serde_json::json!({
                "priority": 0, "budget": budget, "seed": 1, "prompt": FRANCE
            })
            .to_string();
            server.request_status("POST", "/goals", &body)
        };
        assert_eq!(post(room).0, 201);
        let (status, text) = post(room + 1);
        assert_eq!(status, 422, "{text}");
        assert!(text.contains("invalid_input"), "{text}");
    }

    #[test]
    fn every_committed_batch_reads_back_as_whole_characters() {
        let dir = TempDir::new("model-utf8");
        let server = serve(&dir);
        let body = serde_json::json!({
            "priority": 0, "budget": 48, "seed": 3,
            "prompt": "Write the Japanese word for cat, then three cat emoji. /no_think"
        })
        .to_string();
        let created: serde_json::Value =
            serde_json::from_str(&server.request("POST", "/goals", &body)).unwrap();
        let id = created["id"].as_u64().unwrap();
        let mut seen = Vec::new();
        let done = wait_until("the goal to finish", Duration::from_secs(120), || {
            let got: serde_json::Value =
                serde_json::from_str(&server.request("GET", &format!("/goals/{id}"), "")).unwrap();
            seen.push(got["output"].as_str().unwrap_or_default().to_string());
            (got["status"] == "done").then_some(got)
        });
        let whole = done["output"].as_str().unwrap();
        assert!(
            !whole.is_ascii(),
            "the answer needs multi-byte characters: {whole:?}"
        );
        for text in &seen {
            assert!(
                !text.contains('\u{fffd}'),
                "a character was split: {text:?}"
            );
            assert!(
                whole.starts_with(text.as_str()),
                "{text:?} is not a prefix of {whole:?}"
            );
        }
    }

    #[test]
    fn two_fresh_nodes_give_the_same_text_for_the_same_seed() {
        let texts: Vec<String> = ["model-det-a", "model-det-b"]
            .iter()
            .map(|name| {
                let dir = TempDir::new(name);
                let server = serve(&dir);
                ask(&server, 7, 64)["output"].as_str().unwrap().to_string()
            })
            .collect();
        assert_eq!(texts[0], texts[1]);
    }
}
