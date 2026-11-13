//! `rs3 acp` over a node running the real model: stop reasons and conversation history.
#[cfg(test)]
#[path = "common/acp.rs"]
mod acp;
#[cfg(test)]
mod common;
#[cfg(test)]
#[path = "common/model.rs"]
mod model;

#[cfg(test)]
mod tests {
    use std::sync::{Mutex, MutexGuard};
    use std::time::{Duration, Instant};

    use serde_json::{Value, json};

    use crate::acp::Acp;
    use crate::common::{Server, TempDir};
    use crate::model::model_files;

    /// One real model at a time: parallel instrumented models starve each other of CPU.
    static ONE_MODEL: Mutex<()> = Mutex::new(());

    fn start(name: &str) -> (MutexGuard<'static, ()>, TempDir, Server) {
        let turn = ONE_MODEL.lock().unwrap_or_else(|e| e.into_inner());
        let dir = TempDir::new(name);
        let (gguf, tokenizer) = model_files();
        let server = Server::start(
            &dir.0,
            Some(&dir.0.join("rs3.redb")),
            &[
                ("MODEL.PATH", gguf.to_str().unwrap()),
                ("MODEL.TOKENIZER", tokenizer.to_str().unwrap()),
                ("MODEL.THREADS", "4"),
            ],
        );
        (turn, dir, server)
    }

    fn adapter(dir: &TempDir, server: &Server) -> Acp {
        Acp::start(&dir.0, &[format!("http://127.0.0.1:{}", server.port)])
    }

    /// Send a prompt and wait up to `wait` for its response, for a turn that is slow by design.
    fn prompt_within(acp: &mut Acp, session: &str, text: &str, wait: Duration) -> Value {
        let blocks = json!([{"type": "text", "text": text}]);
        acp.request(
            2,
            "session/prompt",
            json!({"sessionId": session, "prompt": blocks}),
        );
        let end = Instant::now() + wait;
        loop {
            let left = end.saturating_duration_since(Instant::now());
            let message = acp.recv_within(left).expect("no response in time");
            if message["id"] == 2 {
                return message;
            }
        }
    }

    #[test]
    fn an_ended_turn_answers_end_turn_with_its_text() {
        let (_turn, dir, server) = start("acp-model-end");
        let mut acp = adapter(&dir, &server);
        let session = acp.new_session(1);
        let (response, chunks) =
            acp.prompt(2, &session, &["What is the capital of France? /no_think"]);
        assert_eq!(response["result"]["stopReason"], "end_turn", "{response}");
        assert!(chunks.concat().contains("Paris"), "{chunks:?}");
        assert_eq!(
            acp.advertised()["params"]["update"]["availableCommands"][0]["name"],
            "run"
        );
        assert!(acp.finish().success());
        assert!(acp.rest_of_stdout().is_empty());
    }

    #[test]
    fn a_later_turn_sees_the_earlier_turns() {
        let (_turn, dir, server) = start("acp-model-history");
        let mut acp = adapter(&dir, &server);
        let session = acp.new_session(1);
        acp.prompt(2, &session, &["Hello! My name is Ada. /no_think"]);
        let (response, chunks) = acp.prompt(
            3,
            &session,
            &["What is my name? Answer with one word. /no_think"],
        );
        assert_eq!(response["result"]["stopReason"], "end_turn", "{response}");
        assert!(chunks.concat().contains("Ada"), "{chunks:?}");
        let (loaded, replayed) = acp.load(4, &session, Duration::from_secs(10));
        assert!(loaded["error"].is_null(), "{loaded}");
        assert!(
            replayed.iter().any(|(_, text)| text.contains("Ada")),
            "{replayed:?}"
        );
        let (status, _) = server.request_status("GET", "/sessions/999", "");
        assert_eq!(status, 404);
        assert!(
            server
                .request("GET", &format!("/sessions/{session}"), "")
                .contains("Ada")
        );
    }

    #[test]
    fn a_turn_that_cannot_finish_in_its_budget_answers_max_tokens() {
        let (_turn, dir, server) = start("acp-model-budget");
        let mut acp = adapter(&dir, &server);
        let session = acp.new_session(1);
        let text = "Count from 1 to 2000, one number per line. /no_think";
        let response = prompt_within(&mut acp, &session, text, Duration::from_secs(300));
        assert_eq!(response["result"]["stopReason"], "max_tokens", "{response}");
    }

    #[test]
    fn a_turn_cancelled_while_the_model_writes_answers_cancelled() {
        let (_turn, dir, server) = start("acp-model-cancel");
        let mut acp = adapter(&dir, &server);
        let session = acp.new_session(1);
        let blocks = json!([{"type": "text", "text": "Count from 1 to 2000. /no_think"}]);
        acp.request(
            2,
            "session/prompt",
            json!({"sessionId": session, "prompt": blocks}),
        );
        let first = acp.recv();
        assert_eq!(first["method"], "session/update", "{first}");
        acp.cancel(&session);
        let (response, _) = acp.read_until(2);
        assert_eq!(response["result"]["stopReason"], "cancelled", "{response}");
    }

    fn per_kind(kinds: &[(String, String)], kind: &str) -> String {
        let of_kind = kinds.iter().filter(|(k, _)| k == kind);
        of_kind.map(|(_, text)| text.as_str()).collect()
    }

    fn first(kinds: &[(String, String)], kind: &str) -> usize {
        let at = kinds.iter().position(|(k, _)| k == kind);
        at.unwrap_or_else(|| panic!("no {kind} in {kinds:?}"))
    }

    #[test]
    fn thinking_streams_as_thought_before_the_message_and_a_load_replays_both() {
        let (_turn, dir, server) = start("acp-model-think");
        let mut acp = adapter(&dir, &server);
        let session = acp.new_session(1);
        let (response, live) = acp.prompt_kinds(2, &session, &["What is 2+3? Answer briefly."]);
        assert_eq!(response["result"]["stopReason"], "end_turn", "{response}");
        assert!(first(&live, "agent_thought_chunk") < first(&live, "agent_message_chunk"));
        assert!(!per_kind(&live, "agent_message_chunk").trim().is_empty());
        assert!(
            live.iter()
                .all(|(_, t)| !t.contains("<think>") && !t.contains("</think>"))
        );
        let mut again = adapter(&dir, &server);
        let (loaded, replayed) = again.load(1, &session, Duration::from_secs(10));
        assert!(loaded["error"].is_null(), "{loaded}");
        let replayed: Vec<_> = replayed.into_iter().skip(1).collect();
        for kind in ["agent_thought_chunk", "agent_message_chunk"] {
            assert_eq!(per_kind(&replayed, kind), per_kind(&live, kind));
        }
        assert!(first(&replayed, "agent_thought_chunk") < first(&replayed, "agent_message_chunk"));
    }

    #[test]
    fn a_no_think_turn_sends_no_thought_chunk() {
        let (_turn, dir, server) = start("acp-model-nothink");
        let mut acp = adapter(&dir, &server);
        let session = acp.new_session(1);
        let (response, kinds) = acp.prompt_kinds(2, &session, &["What is 2+3? /no_think"]);
        assert_eq!(response["result"]["stopReason"], "end_turn", "{response}");
        assert_eq!(per_kind(&kinds, "agent_thought_chunk"), "");
    }
}
