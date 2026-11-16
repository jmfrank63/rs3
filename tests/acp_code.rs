//! `rs3 acp` over a node running a coding goal with canned model replies: the loop streams live.
#[cfg(test)]
#[path = "common/acp.rs"]
mod acp;
#[cfg(test)]
mod common;

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use serde_json::{Value, json};

    use crate::acp::Acp;
    use crate::common::{Server, TempDir};

    const PASSING: &str = "function add(a: number, b: number): number { return a + b; }\n\
        test(\"adds\", () => { expect(add(1, 2)).toEqual(3); });";
    const TYPE_ERROR: &str = "const n: number = \"s\";\n\
        test(\"t\", () => { expect(1).toEqual(1); });";
    const FAILING_TEST: &str = "function add(a: number, b: number): number { return a - b; }\n\
        test(\"adds\", () => { expect(add(1, 2)).toEqual(3); });";
    const SPIN: &str = "while (true) {}";
    const TASK: &str = "/code add two numbers\nexport function add";

    fn reply(source: &str) -> String {
        json!({"explanation": "x", "files": {"a.ts": source}}).to_string()
    }

    fn start(dir: &TempDir, replies: &[String]) -> (Server, Acp) {
        start_at(dir, replies, "200")
    }

    fn start_at(dir: &TempDir, replies: &[String], rate: &str) -> (Server, Acp) {
        let file = dir.0.join("replies.json");
        std::fs::write(&file, serde_json::to_string(replies).unwrap()).unwrap();
        let cache = dir.0.join("checker-cache");
        let server = Server::start(
            &dir.0,
            Some(&dir.0.join("rs3.redb")),
            &[
                ("MODEL.REPLIES", file.to_str().unwrap()),
                ("MODEL.TOKENS_PER_SECOND", rate),
                ("CHECKER.CACHE", cache.to_str().unwrap()),
            ],
        );
        let acp = Acp::start(&dir.0, &[format!("http://127.0.0.1:{}", server.port)]);
        (server, acp)
    }

    /// Run a prompt to the end: the response and every `params.update` as sent.
    fn prompt_updates(acp: &mut Acp, session: &str, text: &str) -> (Value, Vec<Value>) {
        let blocks = json!([{"type": "text", "text": text}]);
        let params = json!({"sessionId": session, "prompt": blocks});
        let (response, updates) = acp.call(2, "session/prompt", params);
        let updates = updates.iter().map(|u| u["params"]["update"].clone());
        (response, updates.collect())
    }

    fn kind(update: &Value) -> &str {
        update["sessionUpdate"].as_str().unwrap()
    }

    fn id_of(update: &Value) -> &str {
        update["toolCallId"].as_str().unwrap_or_default()
    }

    fn position(updates: &[Value], what: &str, id: &str) -> usize {
        let at = updates
            .iter()
            .position(|u| kind(u) == what && id_of(u) == id);
        at.unwrap_or_else(|| panic!("no {what} {id} in {updates:?}"))
    }

    fn goal_of(server: &Server, session: &str) -> u64 {
        let listed = server.request("GET", &format!("/sessions/{session}"), "");
        serde_json::from_str::<Value>(&listed).unwrap()["turns"][0]["goal"]
            .as_u64()
            .unwrap()
    }

    /// The message chunks before `end`, joined.
    fn said(updates: &[Value], from: usize, end: usize) -> String {
        updates[from..end]
            .iter()
            .filter(|u| kind(u) == "agent_message_chunk")
            .map(|u| u["content"]["text"].as_str().unwrap())
            .collect()
    }

    fn count(updates: &[Value], from: usize, end: usize) -> usize {
        let chunks = updates[from..end]
            .iter()
            .filter(|u| kind(u) == "agent_message_chunk");
        chunks.count()
    }

    #[test]
    fn a_fixed_type_error_streams_replies_checks_and_runs_and_ends_end_turn() {
        let dir = TempDir::new("acp-code-fix");
        let (server, mut acp) = start(&dir, &[reply(TYPE_ERROR), reply(PASSING)]);
        let session = acp.new_session(1);
        let (response, updates) = prompt_updates(&mut acp, &session, TASK);
        assert_eq!(response["result"]["stopReason"], "end_turn", "{response}");
        let goal = goal_of(&server, &session);
        let check = format!("goal-{goal}-1-check");
        let opened = position(&updates, "tool_call", &check);
        assert!(count(&updates, 0, opened) > 1, "{updates:?}");
        assert_eq!(said(&updates, 0, opened), reply(TYPE_ERROR));
        let closed = position(&updates, "tool_call_update", &check);
        assert!(opened < closed);
        assert_eq!(updates[closed]["status"], "failed");
        let text = updates[closed]["content"][0]["content"]["text"].as_str();
        let text = text.unwrap();
        assert!(text.contains("TS2322"), "{updates:?}");
        assert!(!text.contains("no type errors"), "{updates:?}");
        let run = format!("goal-{goal}-2-run");
        let ran = position(&updates, "tool_call_update", &run);
        assert_eq!(updates[ran]["status"], "completed");
        let text = updates[ran]["content"][0]["content"]["text"].as_str();
        assert_eq!(text, Some("PASS adds"), "{updates:?}");
        let clean = position(
            &updates,
            "tool_call_update",
            &format!("goal-{goal}-2-check"),
        );
        let text = updates[clean]["content"][0]["content"]["text"].as_str();
        assert_eq!(text, Some("no type errors"), "{updates:?}");
        let second = position(&updates, "tool_call", &format!("goal-{goal}-2-check"));
        assert_eq!(said(&updates, closed, second), reply(PASSING));
        let commands = &acp.advertised()["params"]["update"]["availableCommands"];
        assert_eq!(commands[0]["name"], "run");
        assert_eq!(commands[1]["name"], "code");
        assert!(acp.finish().success());
        assert!(acp.rest_of_stdout().is_empty());
    }

    #[test]
    fn a_check_arrives_while_the_goal_is_still_running() {
        let dir = TempDir::new("acp-code-live");
        let (server, mut acp) = start(&dir, &[reply(TYPE_ERROR), reply(SPIN)]);
        let session = acp.new_session(1);
        let blocks = json!([{"type": "text", "text": TASK}]);
        acp.request(
            2,
            "session/prompt",
            json!({"sessionId": session, "prompt": blocks}),
        );
        for iteration in [1, 2] {
            loop {
                let message = acp.recv();
                let update = &message["params"]["update"];
                if kind(update) == "tool_call_update" && id_of(update).ends_with("-check") {
                    assert!(
                        id_of(update).contains(&format!("-{iteration}-")),
                        "{update}"
                    );
                    break;
                }
            }
            let goal = goal_of(&server, &session);
            let status = server.request("GET", &format!("/goals/{goal}"), "");
            let status = serde_json::from_str::<Value>(&status).unwrap()["status"].clone();
            assert_ne!(
                status, "done",
                "check {iteration} arrived only after the goal ended"
            );
        }
    }

    #[test]
    fn a_failing_test_ends_max_tokens_with_the_failure_in_the_run() {
        let dir = TempDir::new("acp-code-fail");
        let (server, mut acp) = start(&dir, &[reply(FAILING_TEST)]);
        let session = acp.new_session(1);
        let (response, updates) = prompt_updates(&mut acp, &session, TASK);
        assert_eq!(response["result"]["stopReason"], "max_tokens", "{response}");
        let goal = goal_of(&server, &session);
        let ran = position(&updates, "tool_call_update", &format!("goal-{goal}-1-run"));
        assert_eq!(updates[ran]["status"], "failed");
        let text = updates[ran]["content"][0]["content"]["text"].as_str();
        assert!(text.unwrap().contains("FAIL adds: "), "{updates:?}");
    }

    #[test]
    fn a_cancelled_code_turn_answers_cancelled_and_a_plain_prompt_still_works() {
        let dir = TempDir::new("acp-code-cancel");
        let (server, mut acp) = start(&dir, &[reply(PASSING)]);
        let session = acp.new_session(1);
        let blocks = json!([{"type": "text", "text": TASK}]);
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
        let (response, chunks) = acp.prompt(3, &session, &["hello"]);
        assert!(response["error"].is_null(), "{response}");
        assert!(!chunks.concat().is_empty());
        let mut again = Acp::start(&dir.0, &[format!("http://127.0.0.1:{}", server.port)]);
        let (loaded, replayed) = again.load(1, &session, Duration::from_secs(10));
        assert!(loaded["error"].is_null(), "{loaded}");
        assert!(
            replayed.iter().any(|(_, text)| text == "hello"),
            "{replayed:?}"
        );
    }

    #[test]
    fn a_code_prompt_without_a_task_is_an_error_and_starts_no_turn() {
        let dir = TempDir::new("acp-code-empty");
        let (server, mut acp) = start(&dir, &[reply(PASSING)]);
        let session = acp.new_session(1);
        let (response, _) = prompt_updates(&mut acp, &session, "/code");
        assert!(response["error"].is_object(), "{response}");
        let listed = server.request("GET", &format!("/sessions/{session}"), "");
        assert_eq!(
            serde_json::from_str::<Value>(&listed).unwrap(),
            json!({"turns": []})
        );
    }

    /// The updates of `lines` with adjacent message chunks merged into one.
    fn merged(lines: &[Value]) -> Vec<Value> {
        let mut out: Vec<Value> = Vec::new();
        for update in lines {
            let chunk = |u: &Value| kind(u) == "agent_message_chunk";
            match out.last_mut() {
                Some(last) if chunk(last) && chunk(update) => {
                    let joined = format!(
                        "{}{}",
                        last["content"]["text"].as_str().unwrap(),
                        update["content"]["text"].as_str().unwrap()
                    );
                    last["content"]["text"] = json!(joined);
                }
                _ => out.push(update.clone()),
            }
        }
        out
    }

    /// `replayed` equals `live` as sent: tool calls and their updates compare raw, in order, and
    /// message chunks compare in place, joined where adjacent, because a live batch of text may be
    /// split differently from the replayed reply.
    fn assert_replay_matches_live(replayed: &[Value], live: &[Value]) {
        let tools = |updates: &[Value]| {
            let own = updates.iter().filter(|u| kind(u) != "agent_message_chunk");
            own.cloned().collect::<Vec<_>>()
        };
        assert_eq!(tools(replayed), tools(live));
        assert_eq!(merged(replayed), merged(live));
    }

    fn updates_of(lines: &[String]) -> Vec<Value> {
        let parse = |l: &String| serde_json::from_str::<Value>(l).unwrap();
        lines
            .iter()
            .map(|l| parse(l)["params"]["update"].clone())
            .collect()
    }

    fn other_child(dir: &TempDir, server: &Server) -> Acp {
        Acp::start(&dir.0, &[format!("http://127.0.0.1:{}", server.port)])
    }

    #[test]
    fn session_load_replays_a_code_turn_as_it_streamed_and_identically_each_time() {
        let dir = TempDir::new("acp-code-load");
        let (server, mut acp) = start(&dir, &[reply(TYPE_ERROR), reply(PASSING)]);
        let session = acp.new_session(1);
        let (_, live) = prompt_updates(&mut acp, &session, TASK);
        assert!(acp.finish().success());
        let mut first = other_child(&dir, &server);
        let (response, one) = first.load_lines(1, &session, Duration::from_secs(10));
        assert_eq!(response["result"], Value::Null, "{response}");
        let mut second = other_child(&dir, &server);
        let (_, two) = second.load_lines(1, &session, Duration::from_secs(10));
        assert_eq!(one, two);
        let replayed = updates_of(&one);
        assert_eq!(kind(&replayed[0]), "user_message_chunk");
        assert_eq!(replayed[0]["content"]["text"], TASK);
        assert_replay_matches_live(&replayed[1..], &live);
        assert!(replayed.iter().any(|u| kind(u) == "tool_call_update"));
    }

    #[test]
    fn session_load_replays_every_attempts_reply_of_a_malformed_then_valid_goal() {
        let dir = TempDir::new("acp-code-load-malformed");
        let (server, mut acp) = start(&dir, &["not json at all".into(), reply(PASSING)]);
        let session = acp.new_session(1);
        let (response, live) = prompt_updates(&mut acp, &session, TASK);
        assert_eq!(response["result"]["stopReason"], "end_turn", "{response}");
        let mut loaded = other_child(&dir, &server);
        let (_, lines) = loaded.load_lines(1, &session, Duration::from_secs(10));
        let replayed = updates_of(&lines);
        let text = said(&replayed, 0, replayed.len());
        let both = format!("not json at all{}", reply(PASSING));
        assert!(text.contains(&both), "{text}");
        assert_replay_matches_live(&replayed[1..], &live);
    }

    #[test]
    fn loading_a_running_code_goal_lets_the_next_prompt_finish_it() {
        let dir = TempDir::new("acp-code-load-running");
        let (server, mut acp) = start_at(&dir, &[reply(PASSING)], "20");
        let session = acp.new_session(1);
        let blocks = json!([{"type": "text", "text": TASK}]);
        let params = json!({"sessionId": session, "prompt": blocks});
        acp.request(2, "session/prompt", params);
        let first = acp.recv();
        assert_eq!(first["method"], "session/update", "{first}");
        drop(acp);
        let mut acp = other_child(&dir, &server);
        let (response, lines) = acp.load_lines(1, &session, Duration::from_secs(10));
        assert_eq!(response["result"], Value::Null, "{response}");
        let replayed = updates_of(&lines);
        let goal = goal_of(&server, &session);
        let run = format!("goal-{goal}-1-run");
        assert!(
            !replayed.iter().any(|u| id_of(u) == run),
            "the run was replayed before it was committed: {replayed:?}"
        );
        let (response, updates) = prompt_updates(&mut acp, &session, TASK);
        assert!(response["error"].is_null(), "{response}");
        position(&updates, "tool_call_update", &run);
        let mut all = replayed[1..].to_vec();
        all.extend(
            updates
                .iter()
                .filter(|u| kind(u) != "agent_message_chunk" || id_of(u).is_empty())
                .cloned(),
        );
        let opened = all
            .iter()
            .filter(|u| kind(u) == "tool_call" && id_of(u) == run);
        assert_eq!(opened.count(), 1, "{all:?}");
    }
}
