//! Sessions on the real model: a turn sees the earlier turns, and a prompt that cannot fit is refused.
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

    fn json(text: &str) -> serde_json::Value {
        serde_json::from_str(text).unwrap()
    }

    fn new_session(server: &Server) -> u64 {
        json(&server.request("POST", "/sessions", ""))["id"]
            .as_u64()
            .unwrap()
    }

    fn turn(server: &Server, session: u64, turn: u64, prompt: &str) -> (u16, String) {
        let body = serde_json::json!({ "turn": turn, "prompt": prompt }).to_string();
        server.request_status("POST", &format!("/sessions/{session}/turns"), &body)
    }

    fn answer(server: &Server, session: u64, n: u64, prompt: &str) -> serde_json::Value {
        let (status, text) = turn(server, session, n, prompt);
        assert_eq!(status, 201, "{text}");
        let goal = json(&text)["goal"].as_u64().unwrap();
        let end = Instant::now() + Duration::from_secs(180);
        loop {
            let got = json(&server.request("GET", &format!("/goals/{goal}"), ""));
            if got["status"] == "done" {
                return got;
            }
            assert!(Instant::now() < end, "turn {n} never finished: {got}");
            std::thread::sleep(Duration::from_millis(100));
        }
    }

    #[test]
    fn a_later_turn_remembers_an_earlier_one() {
        let dir = TempDir::new("sessions-ada");
        let server = serve(&dir);
        let session = new_session(&server);
        answer(&server, session, 0, "Hello! My name is Ada. /no_think");
        let got = answer(
            &server,
            session,
            1,
            "What is my name? Answer with one word. /no_think",
        );
        assert!(got["output"].as_str().unwrap().contains("Ada"), "{got}");
        assert_eq!(got["reason"], "eos");
    }

    #[test]
    fn a_prompt_that_leaves_no_room_for_the_budget_is_refused() {
        let (gguf, tokenizer) = model_files();
        let candle = rs3::model::Candle::load(&gguf, &tokenizer, 2).unwrap();
        // The context holds 4096 tokens; a turn generates up to 1000 of them.
        let sized = |want: usize| {
            let mut words = want;
            loop {
                let prompt = "a ".repeat(words);
                let have = candle.prompt_tokens(&prompt).unwrap().len();
                if have == want {
                    return prompt;
                }
                assert!(have < want + words, "cannot size a prompt to {want} tokens");
                words = (words + want).saturating_sub(have);
            }
        };
        let dir = TempDir::new("sessions-cap");
        let server = serve(&dir);

        let fits = new_session(&server);
        assert_eq!(turn(&server, fits, 0, &sized(3096)).0, 201);

        let over = new_session(&server);
        let (status, text) = turn(&server, over, 0, &sized(3097));
        assert_eq!(status, 422, "{text}");
        assert!(text.contains("invalid_input"), "{text}");
        assert!(text.contains("3097") && text.contains("4096"), "{text}");
        let listed = json(&server.request("GET", &format!("/sessions/{over}"), ""));
        assert_eq!(listed["turns"], serde_json::json!([]));
    }
}
