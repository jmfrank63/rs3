//! `rs3 acp` is a real ACP agent over stdio that runs a turn against one node.
#[cfg(test)]
#[path = "common/acp.rs"]
mod acp;
#[cfg(test)]
mod common;
#[cfg(test)]
#[path = "common/proxy.rs"]
mod proxy;

#[cfg(test)]
mod tests {
    use std::io::{Read, Write};
    use std::net::TcpListener;
    use std::sync::Arc;
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::time::{Duration, Instant};

    use serde_json::{Value, json};

    use crate::acp::Acp;
    use crate::common::{Server, TempDir};
    use crate::proxy::Proxy;

    fn node(dir: &TempDir, rate: &str) -> Server {
        Server::start(
            &dir.0,
            Some(&dir.0.join("rs3.db")),
            &[("MODEL.TOKENS_PER_SECOND", rate)],
        )
    }

    fn adapter(dir: &TempDir, server: &Server) -> Acp {
        Acp::start(&dir.0, &[format!("http://127.0.0.1:{}", server.port)])
    }

    const SHORT: &[(&str, &str)] = &[("ACP_UNAVAILABLE_TIMEOUT", "1")];

    fn url(port: u16) -> String {
        format!("http://127.0.0.1:{port}")
    }

    fn assert_cluster_unavailable(response: &Value) {
        let error = &response["error"];
        assert_eq!(error["code"], -32001, "{response}");
        assert_eq!(error["message"], "cluster_unavailable", "{response}");
        assert_eq!(error["data"]["error"], "cluster_unavailable", "{response}");
    }

    fn goal(server: &Server, id: u64) -> Value {
        serde_json::from_str(&server.request("GET", &format!("/goals/{id}"), "")).unwrap()
    }

    #[test]
    fn a_turn_streams_the_goal_output_as_chunks() {
        let dir = TempDir::new("acp-turn");
        let server = node(&dir, "500");
        let mut acp = adapter(&dir, &server);
        let (init, _) = acp.call(1, "initialize", json!({"protocolVersion": 1}));
        let result = &init["result"];
        assert_eq!(result["protocolVersion"], 1);
        assert_eq!(result["agentCapabilities"]["loadSession"], true);
        assert_eq!(result["authMethods"], json!([]));
        assert_eq!(result["agentInfo"]["name"], "rs3");
        let session = acp.new_session(2);
        acp.request(
            3,
            "session/prompt",
            json!({"sessionId": session, "prompt": [{"type": "text", "text": "hi"}]}),
        );
        let mut chunks = Vec::new();
        let response = loop {
            let message = acp.recv();
            if message["id"] == 3 {
                break message;
            }
            assert_eq!(message["method"], "session/update");
            assert_eq!(message["params"]["sessionId"], session);
            let update = &message["params"]["update"];
            assert_eq!(update["sessionUpdate"], "agent_message_chunk");
            assert_eq!(update["content"]["type"], "text");
            chunks.push(update["content"]["text"].as_str().unwrap().to_owned());
        };
        assert_eq!(response["result"]["stopReason"], "end_turn");
        assert!(chunks.len() >= 2, "{chunks:?}");
        assert!(chunks.iter().all(|c| !c.is_empty()));
        let done = goal(&server, 1);
        assert_eq!(done["status"], "done");
        assert_eq!(chunks.concat(), done["output"].as_str().unwrap());
        assert!(acp.finish().success());
    }

    #[test]
    fn each_new_session_has_its_own_id() {
        let dir = TempDir::new("acp-sessions");
        let server = node(&dir, "500");
        let mut acp = adapter(&dir, &server);
        let first = acp.new_session(1);
        let second = acp.new_session(2);
        assert_ne!(first, second);
        let (response, chunks) = acp.prompt(3, &second, &["x"]);
        assert_eq!(response["result"]["stopReason"], "end_turn");
        assert!(!chunks.is_empty());
    }

    fn output_for(name: &str, blocks: Vec<Value>) -> String {
        let dir = TempDir::new(name);
        let server = node(&dir, "500");
        let mut acp = adapter(&dir, &server);
        let session = acp.new_session(1);
        let (response, updates) = acp.call(
            2,
            "session/prompt",
            json!({"sessionId": session, "prompt": blocks}),
        );
        assert_eq!(response["result"]["stopReason"], "end_turn");
        assert!(updates.len() >= 2);
        goal(&server, 1)["output"].as_str().unwrap().to_owned()
    }

    fn text(t: &str) -> Value {
        json!({"type": "text", "text": t})
    }

    #[test]
    fn prompt_text_blocks_are_concatenated_and_images_ignored() {
        let image = json!({"type": "image", "data": "", "mimeType": "image/png"});
        let split = output_for("acp-split", vec![text("hel"), image, text("lo")]);
        let whole = output_for("acp-whole", vec![text("hello")]);
        assert_eq!(split, whole);
    }

    #[test]
    fn a_resource_link_reaches_the_goal_as_its_uri() {
        let link = json!({"type": "resource_link", "uri": "file:///a.rs", "name": "a.rs"});
        let linked = output_for("acp-link", vec![text("see "), link]);
        let written = output_for("acp-link-text", vec![text("see file:///a.rs")]);
        assert_eq!(linked, written);
    }

    #[test]
    fn closing_stdin_mid_turn_exits_cleanly_and_leaves_the_goal_running() {
        let dir = TempDir::new("acp-exit");
        let server = node(&dir, "200");
        let mut acp = adapter(&dir, &server);
        let session = acp.new_session(1);
        acp.request(
            2,
            "session/prompt",
            json!({"sessionId": session, "prompt": [{"type": "text", "text": "x"}]}),
        );
        assert_eq!(acp.recv()["method"], "session/update");
        assert_eq!(acp.finish().code(), Some(0));
        let deadline = Instant::now() + Duration::from_secs(30);
        loop {
            let status = goal(&server, 1)["status"].clone();
            assert_ne!(status, "cancelled");
            if status == "done" {
                break;
            }
            assert!(Instant::now() < deadline, "goal 1 never finished: {status}");
            std::thread::sleep(Duration::from_millis(50));
        }
    }

    #[test]
    fn a_response_written_just_before_stdin_closes_is_not_lost() {
        let dir = TempDir::new("acp-flush");
        let server = node(&dir, "500");
        let mut acp = adapter(&dir, &server);
        acp.request(1, "initialize", json!({"protocolVersion": 1}));
        let status = acp.finish();
        assert_eq!(acp.recv()["id"], 1);
        assert!(status.success());
    }

    #[test]
    fn a_turn_against_an_unreachable_node_answers_an_error() {
        let dir = TempDir::new("acp-down");
        let server = node(&dir, "500");
        let mut acp = Acp::start_with(&dir.0, &[url(server.port)], SHORT);
        let session = acp.new_session(1);
        drop(server);
        let (response, chunks) = acp.prompt(2, &session, &["x"]);
        assert_cluster_unavailable(&response);
        assert!(chunks.is_empty());
    }

    #[test]
    fn a_goal_cancelled_by_someone_else_answers_cancelled() {
        let dir = TempDir::new("acp-cancel");
        let server = node(&dir, "200");
        let mut acp = adapter(&dir, &server);
        let session = acp.new_session(1);
        acp.request(
            2,
            "session/prompt",
            json!({"sessionId": session, "prompt": [{"type": "text", "text": "x"}]}),
        );
        assert_eq!(acp.recv()["method"], "session/update");
        server.request("DELETE", "/goals/1", "");
        loop {
            let message = acp.recv();
            if message["id"] == 2 {
                assert_eq!(message["result"]["stopReason"], "cancelled", "{message}");
                break;
            }
        }
        assert_normal_turn(&mut acp, &server, 3, 2);
        assert!(acp.finish().success());
    }

    fn pair(kind: &str, text: &str) -> (String, String) {
        (kind.to_owned(), text.to_owned())
    }

    fn loaded(acp: &mut Acp, id: u64, session: &str) -> (Value, Vec<(String, String)>) {
        acp.load(id, session, Duration::from_secs(10))
    }

    #[test]
    fn a_new_adapter_replays_a_finished_session_and_continues_it() {
        let dir = TempDir::new("acp-load");
        let server = node(&dir, "500");
        let mut first = adapter(&dir, &server);
        let session = first.new_session(1);
        first.prompt(2, &session, &["p1"]);
        first.prompt(3, &session, &["p2"]);
        assert!(first.finish().success());
        let mut acp = adapter(&dir, &server);
        let (response, updates) = loaded(&mut acp, 1, &session);
        assert_eq!(response["result"], Value::Null, "{response}");
        assert!(response.get("error").is_none(), "{response}");
        let out = |id| goal(&server, id)["output"].as_str().unwrap().to_owned();
        let expected = [
            pair("user_message_chunk", "p1"),
            pair("agent_message_chunk", &out(1)),
            pair("user_message_chunk", "p2"),
            pair("agent_message_chunk", &out(2)),
        ];
        assert_eq!(updates, expected);
        assert_eq!(server.request_status("GET", "/goals/3", "").0, 404);
        let (response, chunks) = acp.prompt(2, &session, &["p3"]);
        assert_eq!(response["result"]["stopReason"], "end_turn", "{response}");
        assert_eq!(chunks.concat(), out(3));
        let turns = server.request("GET", &format!("/sessions/{session}"), "");
        let turns: Value = serde_json::from_str(&turns).unwrap();
        assert_eq!(turns["turns"].as_array().unwrap().len(), 3, "{turns}");
        assert!(acp.finish().success());
    }

    #[test]
    fn a_new_adapter_loads_without_waiting_and_delivers_the_rest_of_the_turn() {
        let dir = TempDir::new("acp-resume");
        let server = node(&dir, "100");
        let mut first = adapter(&dir, &server);
        let session = first.new_session(1);
        prompt_request(&mut first, 2, &session);
        let head = chunk_text(&first.recv());
        drop(first);
        let mut acp = adapter(&dir, &server);
        let (response, updates) = loaded(&mut acp, 1, &session);
        assert_eq!(response["result"], Value::Null, "{response}");
        let replayed = updates[1].1.clone();
        let output = |id| goal(&server, id)["output"].as_str().unwrap().to_owned();
        assert!(output(1).starts_with(&replayed), "{replayed}");
        assert!(replayed.starts_with(&head), "{replayed}");
        assert!(acp.recv_within(Duration::from_millis(300)).is_none());
        let (response, chunks) = acp.prompt(2, &session, &["next"]);
        assert_eq!(response["result"]["stopReason"], "end_turn", "{response}");
        let expected = format!("{}{}", &output(1)[replayed.len()..], output(2));
        assert_eq!(chunks.concat(), expected);
        assert_eq!(session_of(&server, 1)["turns"].as_array().unwrap().len(), 2);
        assert_eq!(server.request_status("GET", "/goals/3", "").0, 404);
        assert!(acp.finish().success());
    }

    #[test]
    fn a_cancelled_goal_is_not_streamed_again_after_a_load() {
        let dir = TempDir::new("acp-load-cancelled");
        let server = node(&dir, "100");
        let mut first = adapter(&dir, &server);
        let session = first.new_session(1);
        prompt_request(&mut first, 2, &session);
        first.recv();
        drop(first);
        server.request("DELETE", "/goals/1", "");
        let mut acp = adapter(&dir, &server);
        let (response, _) = loaded(&mut acp, 1, &session);
        assert_eq!(response["result"], Value::Null, "{response}");
        let (response, chunks) = acp.prompt(2, &session, &["next"]);
        assert_eq!(response["result"]["stopReason"], "end_turn", "{response}");
        assert_eq!(
            chunks.concat(),
            goal(&server, 2)["output"].as_str().unwrap()
        );
        assert!(acp.finish().success());
    }

    #[test]
    fn a_failed_goal_is_not_streamed_again_after_a_load() {
        let dir = TempDir::new("acp-load-failed");
        let port = fake_node("failed", &Arc::new(AtomicBool::new(true)));
        let mut acp = Acp::start(&dir.0, &[url(port)]);
        let (response, _) = loaded(&mut acp, 1, "2");
        assert_eq!(response["result"], Value::Null, "{response}");
        let (response, _) = acp.prompt(2, "2", &["next"]);
        assert_eq!(response["result"]["stopReason"], "end_turn", "{response}");
        assert!(acp.finish().success());
    }

    #[test]
    fn loading_a_session_with_a_prompt_pending_is_refused_and_the_prompt_finishes() {
        let dir = TempDir::new("acp-load-busy");
        let server = node(&dir, "500");
        let mut acp = adapter(&dir, &server);
        let session = acp.new_session(1);
        prompt_request(&mut acp, 2, &session);
        let first = acp.recv();
        let params = json!({"sessionId": session, "cwd": "/", "mcpServers": []});
        acp.request(3, "session/load", params);
        let (refused, mut updates) = acp.read_until(3);
        assert_eq!(refused["error"]["code"], -32002, "{refused}");
        let (response, rest) = acp.read_until(2);
        assert_eq!(response["result"]["stopReason"], "end_turn", "{response}");
        updates.extend(rest);
        let mut all = vec![first];
        all.extend(updates);
        let text: String = all.iter().map(chunk_text).collect();
        assert_eq!(text, goal(&server, 1)["output"].as_str().unwrap());
        assert!(acp.finish().success());
    }

    #[test]
    fn loading_an_unknown_or_malformed_session_id_answers_invalid_params() {
        let dir = TempDir::new("acp-load-bad");
        let server = node(&dir, "500");
        let mut acp = adapter(&dir, &server);
        let ids = [
            json!("99"),
            json!("abc"),
            json!("../goals/1"),
            json!(5),
            Value::Null,
        ];
        let mut n = 1;
        // Goal 1 must exist so that a path escape would have something to read.
        let session = acp.new_session(n);
        acp.prompt(n + 1, &session, &["x"]);
        n += 2;
        for id in ids {
            let params = json!({"sessionId": id, "cwd": "/", "mcpServers": []});
            let (response, updates) = acp.call(n, "session/load", params);
            assert_eq!(error_code(&response), -32602, "{id}: {response}");
            assert!(updates.is_empty(), "{id}: {updates:?}");
            n += 1;
        }
        let (response, _) = acp.call(n, "session/load", json!({"cwd": "/"}));
        assert_eq!(error_code(&response), -32602, "{response}");
        assert_normal_turn(&mut acp, &server, n + 1, 2);
        assert!(acp.finish().success());
    }

    #[test]
    fn loading_through_an_unreachable_node_answers_unavailable_and_loads_nothing() {
        let dir = TempDir::new("acp-load-down");
        let server = node(&dir, "500");
        let mut first = adapter(&dir, &server);
        let session = first.new_session(1);
        first.prompt(2, &session, &["x"]);
        let mut acp = Acp::start_with(&dir.0, &[url(server.port)], SHORT);
        drop(server);
        let (response, updates) = loaded(&mut acp, 1, &session);
        assert_cluster_unavailable(&response);
        assert!(updates.is_empty(), "{updates:?}");
        let (response, _) = acp.prompt(2, &session, &["y"]);
        assert_eq!(error_code(&response), -32602, "{response}");
        assert!(acp.finish().success());
    }

    #[test]
    fn a_goal_with_no_output_replays_without_an_empty_agent_chunk() {
        let dir = TempDir::new("acp-load-empty");
        let server = node(&dir, "1");
        let session: Value =
            serde_json::from_str(&server.request("POST", "/sessions", "")).unwrap();
        let session = session["id"].as_u64().unwrap();
        let body = r#"{"turn":0,"prompt":"p1"}"#;
        server.request("POST", &format!("/sessions/{session}/turns"), body);
        server.request("DELETE", "/goals/1", "");
        assert_eq!(goal(&server, 1)["output"], "");
        let mut acp = adapter(&dir, &server);
        let (response, updates) = loaded(&mut acp, 1, &session.to_string());
        assert_eq!(response["result"], Value::Null, "{response}");
        assert_eq!(updates, [pair("user_message_chunk", "p1")]);
        assert!(acp.finish().success());
    }

    #[test]
    fn without_a_node_url_it_exits_with_a_failure() {
        let dir = TempDir::new("acp-usage");
        let mut acp = Acp::start(&dir.0, &[]);
        assert_eq!(acp.finish().code(), Some(1));
        assert!(acp.stderr_text().contains("usage"), "{}", acp.stderr_text());
        assert!(acp.rest_of_stdout().is_empty());
    }

    fn error_code(response: &Value) -> i64 {
        response["error"]["code"].as_i64().unwrap()
    }

    /// A normal turn on a fresh session ends in end_turn with the goal output as chunks.
    fn assert_normal_turn(acp: &mut Acp, server: &Server, id: u64, goal_id: u64) {
        let session = acp.new_session(id);
        let (response, chunks) = acp.prompt(id + 1, &session, &["again"]);
        assert_eq!(response["result"]["stopReason"], "end_turn");
        assert_eq!(
            chunks.concat(),
            goal(server, goal_id)["output"].as_str().unwrap()
        );
    }

    #[test]
    fn bad_input_gets_its_code_and_the_next_turn_still_completes() {
        let dir = TempDir::new("acp-bad");
        let server = node(&dir, "500");
        let mut acp = adapter(&dir, &server);
        acp.send("{not json");
        let response = acp.recv();
        assert_eq!(error_code(&response), -32700);
        assert_eq!(response["id"], Value::Null);
        let (response, _) = acp.call(7, "no/such", json!({}));
        assert_eq!(error_code(&response), -32601);
        let (response, _) = acp.call(
            8,
            "session/prompt",
            json!({"sessionId": "nope", "prompt": []}),
        );
        assert_eq!(error_code(&response), -32602);
        assert_normal_turn(&mut acp, &server, 9, 1);
    }

    #[test]
    fn invalid_utf8_answers_a_parse_error_and_the_process_lives() {
        let dir = TempDir::new("acp-utf8");
        let server = node(&dir, "500");
        let mut acp = adapter(&dir, &server);
        acp.send_bytes(b"{\"jsonrpc\":\"2.0\",\"method\":\"\xff\xfe\"}");
        assert_eq!(error_code(&acp.recv()), -32700);
        let (init, _) = acp.call(1, "initialize", json!({"protocolVersion": 1}));
        assert_eq!(init["result"]["protocolVersion"], 1);
    }

    #[test]
    fn values_that_are_not_requests_answer_invalid_request() {
        let dir = TempDir::new("acp-invalid");
        let server = node(&dir, "500");
        let mut acp = adapter(&dir, &server);
        for line in [
            "42",
            "[1,2]",
            r#"{"jsonrpc":"2.0","id":5}"#,
            r#"{"id":6,"method":7}"#,
        ] {
            acp.send(line);
            let response = acp.recv();
            assert_eq!(error_code(&response), -32600, "{line}");
        }
        acp.send(r#"{"jsonrpc":"2.0","id":9}"#);
        assert_eq!(acp.recv()["id"], 9);
    }

    #[test]
    fn notifications_are_never_answered() {
        let dir = TempDir::new("acp-notify");
        let server = node(&dir, "500");
        let mut acp = adapter(&dir, &server);
        acp.send(r#"{"jsonrpc":"2.0","method":"no/such"}"#);
        acp.send(r#"{"jsonrpc":"2.0","method":"session/prompt","params":{"sessionId":"x"}}"#);
        acp.request(3, "initialize", json!({"protocolVersion": 1}));
        let next = acp.recv();
        assert_eq!(next["id"], 3);
        assert!(next["result"].is_object());
    }

    #[test]
    fn a_prompt_without_a_prompt_array_answers_invalid_params() {
        let dir = TempDir::new("acp-params");
        let server = node(&dir, "500");
        let mut acp = adapter(&dir, &server);
        let session = acp.new_session(1);
        for (id, params) in [
            (2, json!({"sessionId": session})),
            (3, json!({"sessionId": session, "prompt": "hi"})),
            (4, json!({"prompt": []})),
        ] {
            let (response, _) = acp.call(id, "session/prompt", params);
            assert_eq!(error_code(&response), -32602, "{response}");
        }
    }

    #[test]
    fn a_node_that_dies_mid_turn_answers_an_error_and_the_adapter_lives() {
        let dir = TempDir::new("acp-die");
        let server = node(&dir, "200");
        let mut acp = Acp::start_with(&dir.0, &[url(server.port)], SHORT);
        let session = acp.new_session(1);
        acp.request(
            2,
            "session/prompt",
            json!({"sessionId": session, "prompt": [{"type": "text", "text": "x"}]}),
        );
        assert_eq!(acp.recv()["method"], "session/update");
        drop(server);
        loop {
            let message = acp.recv();
            if message["id"] == 2 {
                assert_cluster_unavailable(&message);
                break;
            }
        }
        let (init, _) = acp.call(3, "initialize", json!({"protocolVersion": 1}));
        assert_eq!(init["result"]["protocolVersion"], 1);
        assert!(acp.stderr_text().contains("GET /goals/1"));
    }

    #[test]
    fn an_unreachable_node_leaves_the_adapter_answering() {
        let dir = TempDir::new("acp-down2");
        let port = crate::common::free_port();
        let mut acp = Acp::start_with(&dir.0, &[url(port)], SHORT);
        let started = Instant::now();
        acp.request(1, "session/new", json!({"cwd": "/", "mcpServers": []}));
        let init = acp.call(3, "initialize", json!({"protocolVersion": 1})).0;
        assert_eq!(init["result"]["protocolVersion"], 1);
        let response = acp.recv();
        assert_eq!(response["id"], 1);
        assert_cluster_unavailable(&response);
        assert!(response["result"]["sessionId"].is_null());
        assert!(started.elapsed() >= Duration::from_millis(900));
    }

    #[test]
    fn a_node_killed_mid_turn_and_restarted_within_the_timeout_finishes_the_turn() {
        let dir = TempDir::new("acp-restart");
        let db = dir.0.join("rs3.db");
        let rate = [("MODEL.TOKENS_PER_SECOND", "100")];
        let server = Server::start(&dir.0, Some(&db), &rate);
        let port = server.port;
        let mut acp = Acp::start_with(&dir.0, &[url(port)], &[("ACP_UNAVAILABLE_TIMEOUT", "30")]);
        let session = acp.new_session(1);
        acp.request(
            2,
            "session/prompt",
            json!({"sessionId": session, "prompt": [{"type": "text", "text": "x"}]}),
        );
        let mut chunks = Vec::new();
        let first = acp.recv();
        chunks.push(first["params"]["update"]["content"]["text"].clone());
        drop(server);
        let server = Server::start_on(port, &dir.0, Some(&db), &rate);
        let response = loop {
            let message = acp.recv();
            if message["id"] == 2 {
                break message;
            }
            chunks.push(message["params"]["update"]["content"]["text"].clone());
        };
        assert_eq!(response["result"]["stopReason"], "end_turn", "{response}");
        let text: String = chunks.iter().map(|c| c.as_str().unwrap()).collect();
        assert_eq!(text, goal(&server, 1)["output"].as_str().unwrap());
    }

    #[test]
    fn a_node_that_never_answers_is_given_up_on_at_the_timeout() {
        let dir = TempDir::new("acp-hung");
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        let mut acp = Acp::start_with(&dir.0, &[url(port)], SHORT);
        let started = Instant::now();
        let (response, _) = acp.call(1, "session/new", json!({"cwd": "/", "mcpServers": []}));
        assert_cluster_unavailable(&response);
        assert!(
            started.elapsed() < Duration::from_secs(4),
            "{:?}",
            started.elapsed()
        );
        drop(listener);
    }

    #[test]
    fn an_unparsable_unavailable_timeout_exits_naming_the_variable() {
        let dir = TempDir::new("acp-badtimeout");
        let mut acp = Acp::start_with(
            &dir.0,
            &[url(crate::common::free_port())],
            &[("ACP_UNAVAILABLE_TIMEOUT", "abc")],
        );
        assert_eq!(acp.finish().code(), Some(1));
        assert!(acp.stderr_text().contains("ACP_UNAVAILABLE_TIMEOUT"));
        assert!(acp.rest_of_stdout().is_empty());
    }

    #[test]
    fn a_turn_is_the_session_turn_zero_with_the_prompt_text() {
        let dir = TempDir::new("acp-direct");
        let server = node(&dir, "500");
        server.request("POST", "/sessions", "");
        server.request(
            "POST",
            "/sessions/1/turns",
            r#"{"turn":0,"prompt":"hello"}"#,
        );
        let deadline = Instant::now() + Duration::from_secs(30);
        while goal(&server, 1)["status"] != "done" {
            assert!(Instant::now() < deadline, "goal 1 never finished");
            std::thread::sleep(Duration::from_millis(20));
        }
        let direct = goal(&server, 1)["output"].as_str().unwrap().to_owned();
        assert_eq!(output_for("acp-seeded", vec![text("hello")]), direct);
    }

    fn session_of(server: &Server, id: u64) -> Value {
        serde_json::from_str(&server.request("GET", &format!("/sessions/{id}"), "")).unwrap()
    }

    #[test]
    fn two_prompts_in_one_session_are_turns_zero_and_one() {
        let dir = TempDir::new("acp-two");
        let server = node(&dir, "500");
        let mut acp = adapter(&dir, &server);
        let session = acp.new_session(1);
        for id in [2, 3] {
            let (response, _) = acp.prompt(id, &session, &["same"]);
            assert_eq!(response["result"]["stopReason"], "end_turn");
        }
        let turns = session_of(&server, 1)["turns"].clone();
        assert_eq!(
            turns,
            json!([{"prompt": "same", "goal": 1}, {"prompt": "same", "goal": 2}])
        );
    }

    fn prompt_request(acp: &mut Acp, id: u64, session: &str) {
        let params = json!({"sessionId": session, "prompt": [text("hi")]});
        acp.request(id, "session/prompt", params);
    }

    #[test]
    fn a_second_prompt_on_a_busy_session_is_refused_and_the_first_turn_finishes() {
        let dir = TempDir::new("acp-busy");
        let server = node(&dir, "500");
        let mut acp = adapter(&dir, &server);
        let session = acp.new_session(1);
        prompt_request(&mut acp, 2, &session);
        let first = acp.recv();
        assert_eq!(first["method"], "session/update", "{first}");
        prompt_request(&mut acp, 3, &session);
        let (refused, mut updates) = acp.read_until(3);
        assert_eq!(refused["error"]["code"], -32002, "{refused}");
        assert_eq!(refused["error"]["message"], "turn_in_flight", "{refused}");
        assert_eq!(
            refused["error"]["data"]["error"], "turn_in_flight",
            "{refused}"
        );
        let (response, rest) = acp.read_until(2);
        assert_eq!(response["result"]["stopReason"], "end_turn", "{response}");
        updates.extend(rest);
        let mut chunks = vec![first];
        chunks.extend(updates);
        let chunks: String = chunks
            .iter()
            .map(|u| u["params"]["update"]["content"]["text"].as_str().unwrap())
            .collect();
        assert_eq!(chunks, goal(&server, 1)["output"].as_str().unwrap());
        assert_eq!(server.request_status("GET", "/goals/2", "").0, 404);
        assert_eq!(session_of(&server, 1)["turns"].as_array().unwrap().len(), 1);
    }

    #[test]
    fn a_prompt_sent_on_receiving_a_response_is_not_refused() {
        let dir = TempDir::new("acp-back-to-back");
        let server = node(&dir, "500");
        let mut acp = adapter(&dir, &server);
        let session = acp.new_session(1);
        for id in 2..6 {
            let (response, _) = acp.prompt(id, &session, &["x"]);
            assert_eq!(response["result"]["stopReason"], "end_turn", "{response}");
        }
    }

    #[test]
    fn two_adapters_on_one_node_get_different_session_ids() {
        let dir = TempDir::new("acp-pair");
        let server = node(&dir, "500");
        let mut first = adapter(&dir, &server);
        let mut second = adapter(&dir, &server);
        assert_ne!(first.new_session(1), second.new_session(1));
    }

    #[test]
    fn a_dead_first_url_is_skipped_for_a_live_one() {
        let dir = TempDir::new("acp-failover");
        let server = node(&dir, "500");
        let dead = crate::common::free_port();
        let urls = [
            format!("http://127.0.0.1:{dead}"),
            format!("http://127.0.0.1:{}", server.port),
        ];
        let mut acp = Acp::start(&dir.0, &urls);
        let session = acp.new_session(1);
        let (response, chunks) = acp.prompt(2, &session, &["x"]);
        assert_eq!(response["result"]["stopReason"], "end_turn");
        assert_eq!(
            chunks.concat(),
            goal(&server, 1)["output"].as_str().unwrap()
        );
        let log = acp.stderr_text();
        assert_eq!(log.lines().count(), 1, "{log}");
    }

    /// A node that answers 503 no_quorum to its first `refusals` requests, then creates session 7.
    fn node_losing_quorum(refusals: usize) -> u16 {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        std::thread::spawn(move || {
            for (n, mut stream) in listener.incoming().flatten().enumerate() {
                let _ = stream.read(&mut [0u8; 4096]);
                let (status, body) = if n < refusals {
                    ("503 Service Unavailable", r#"{"error":"no_quorum"}"#)
                } else {
                    ("200 OK", r#"{"id":7}"#)
                };
                let head = format!("HTTP/1.1 {status}\r\nContent-Length: {}\r\n", body.len());
                let _ = write!(stream, "{head}Connection: close\r\n\r\n{body}");
            }
        });
        port
    }

    #[test]
    fn a_node_answering_503_no_quorum_is_retried_until_it_recovers() {
        let dir = TempDir::new("acp-no-quorum");
        let mut acp = Acp::start(&dir.0, &[url(node_losing_quorum(2))]);
        let (response, _) = acp.call(1, "session/new", json!({"cwd": "/", "mcpServers": []}));
        assert_eq!(response["result"]["sessionId"], "7", "{response}");
        let log = acp.stderr_text();
        assert_eq!(log.lines().count(), 2, "{log}");
        assert!(log.contains("503"), "{log}");
    }

    #[test]
    fn a_node_without_a_database_answers_goals_unavailable_which_is_final() {
        let dir = TempDir::new("acp-503");
        let server = Server::start(&dir.0, None, &[]);
        let mut acp = Acp::start(&dir.0, &[url(server.port)]);
        let (response, _) = acp.call(1, "session/new", json!({"cwd": "/", "mcpServers": []}));
        assert_eq!(error_code(&response), -32603);
        let log = acp.stderr_text();
        assert_eq!(log.lines().count(), 1, "{log}");
        assert!(log.contains("503"), "{log}");
    }

    #[test]
    fn a_final_status_from_a_node_is_not_retried() {
        let (dir, other) = (TempDir::new("acp-final"), TempDir::new("acp-final-b"));
        let (first, second) = (node(&dir, "500"), node(&other, "500"));
        let urls = [
            format!("http://127.0.0.1:{}", first.port),
            format!("http://127.0.0.1:{}", second.port),
        ];
        let mut acp = Acp::start(&dir.0, &urls);
        let session = acp.new_session(1);
        drop(first);
        let (response, _) = acp.prompt(2, &session, &["x"]);
        // A 404 on a turn POST is answered as an unknown script.
        assert_eq!(error_code(&response), -32003);
        // The dead node is skipped once and logged; the live node's 404 is final, so not retried.
        let log = acp.stderr_text();
        assert_eq!(log.lines().count(), 1, "{log}");
    }

    #[test]
    fn a_dropped_turn_response_is_resent_and_makes_one_goal() {
        let dir = TempDir::new("acp-lost");
        let server = node(&dir, "500");
        let proxy = Proxy::start(server.port);
        let mut acp = Acp::start(&dir.0, &[format!("http://127.0.0.1:{}", proxy.port)]);
        let session = acp.new_session(1);
        proxy.drop_next(1);
        let (response, _) = acp.prompt(2, &session, &["x"]);
        assert_eq!(response["result"]["stopReason"], "end_turn");
        assert_eq!(proxy.dropped(), 1);
        let (status, _) = server.request_status("GET", "/goals/2", "");
        assert_eq!(status, 404);
    }

    #[test]
    fn when_every_response_is_dropped_the_prompt_fails_and_the_same_prompt_later_succeeds() {
        let dir = TempDir::new("acp-lost-all");
        let server = node(&dir, "500");
        let proxy = Proxy::start(server.port);
        let mut acp = Acp::start_with(&dir.0, &[url(proxy.port)], SHORT);
        let session = acp.new_session(1);
        proxy.drop_next(1000);
        let (response, _) = acp.prompt(2, &session, &["x"]);
        assert_cluster_unavailable(&response);
        assert!(proxy.dropped() >= 1);
        proxy.drop_next(0);
        let (response, _) = acp.prompt(3, &session, &["x"]);
        assert_eq!(response["result"]["stopReason"], "end_turn");
        let turns = session_of(&server, 1)["turns"].clone();
        assert_eq!(turns, json!([{"prompt": "x", "goal": 1}]));
    }

    /// Start a prompt, read its first chunk, kill the node and return the -32001 answer
    /// together with every chunk received so far.
    fn prompt_cut_off_by_a_dead_node(acp: &mut Acp, session: &str, server: Server) -> Vec<String> {
        acp.request(
            2,
            "session/prompt",
            json!({"sessionId": session, "prompt": [{"type": "text", "text": "x"}]}),
        );
        let mut chunks = vec![chunk_text(&acp.recv())];
        drop(server);
        loop {
            let message = acp.recv();
            if message["id"] == 2 {
                assert_cluster_unavailable(&message);
                return chunks;
            }
            chunks.push(chunk_text(&message));
        }
    }

    fn chunk_text(update: &Value) -> String {
        update["params"]["update"]["content"]["text"]
            .as_str()
            .unwrap()
            .to_owned()
    }

    #[test]
    fn the_unfinished_turn_is_delivered_by_the_next_prompt_after_the_node_returns() {
        let dir = TempDir::new("acp-unfinished");
        let db = dir.0.join("rs3.db");
        let rate = [("MODEL.TOKENS_PER_SECOND", "100")];
        let server = Server::start(&dir.0, Some(&db), &rate);
        let port = server.port;
        let mut acp = Acp::start_with(&dir.0, &[url(port)], SHORT);
        let session = acp.new_session(1);
        let mut received = prompt_cut_off_by_a_dead_node(&mut acp, &session, server);
        let (response, chunks) = acp.prompt(3, &session, &["y"]);
        assert_cluster_unavailable(&response);
        assert!(chunks.is_empty(), "{chunks:?}");
        let server = Server::start_on(port, &dir.0, Some(&db), &rate);
        let (response, chunks) = acp.prompt(4, &session, &["y"]);
        assert_eq!(response["result"]["stopReason"], "end_turn", "{response}");
        received.extend(chunks);
        let (first, second) = (goal(&server, 1), goal(&server, 2));
        let expected = format!(
            "{}{}",
            first["output"].as_str().unwrap(),
            second["output"].as_str().unwrap()
        );
        assert_eq!(received.concat(), expected);
        assert_eq!(server.request_status("GET", "/goals/3", "").0, 404);
        let turns = session_of(&server, 1)["turns"].clone();
        assert_eq!(
            turns,
            json!([{"prompt": "x", "goal": 1}, {"prompt": "y", "goal": 2}])
        );
    }

    /// A prompt cut off by a dead node, its node back at `rate`, and goal 1 still unfinished.
    fn unfinished_turn_on_a_restarted_node(dir: &TempDir, rate: &str) -> (Acp, String, Server) {
        let db = dir.0.join("rs3.db");
        let rate = [("MODEL.TOKENS_PER_SECOND", rate)];
        let server = Server::start(&dir.0, Some(&db), &rate);
        let port = server.port;
        let mut acp = Acp::start_with(&dir.0, &[url(port)], SHORT);
        let session = acp.new_session(1);
        prompt_cut_off_by_a_dead_node(&mut acp, &session, server);
        (
            acp,
            session,
            Server::start_on(port, &dir.0, Some(&db), &rate),
        )
    }

    #[test]
    fn a_cancelled_unfinished_turn_does_not_swallow_the_next_prompt() {
        let dir = TempDir::new("acp-unfinished-cancel");
        let (mut acp, session, server) = unfinished_turn_on_a_restarted_node(&dir, "20");
        server.request("DELETE", "/goals/1", "");
        let (response, chunks) = acp.prompt(3, &session, &["y"]);
        assert_eq!(response["result"]["stopReason"], "end_turn", "{response}");
        let own = goal(&server, 2);
        assert_eq!(own["status"], "done", "{own}");
        assert!(chunks.concat().ends_with(own["output"].as_str().unwrap()));
    }

    #[test]
    fn cancelling_while_an_unfinished_turn_is_delivered_starts_no_new_turn() {
        let dir = TempDir::new("acp-unfinished-own-cancel");
        let (mut acp, session, server) = unfinished_turn_on_a_restarted_node(&dir, "20");
        acp.request(
            3,
            "session/prompt",
            json!({"sessionId": session, "prompt": [{"type": "text", "text": "y"}]}),
        );
        acp.cancel(&session);
        let (response, _) = acp.read_until(3);
        assert_eq!(response["result"]["stopReason"], "cancelled", "{response}");
        assert_eq!(goal(&server, 1)["status"], "cancelled");
        assert_eq!(goal(&server, 2)["error"], "not_found");
    }

    #[test]
    fn a_finished_turn_leaves_nothing_for_the_next_prompt_to_replay() {
        let dir = TempDir::new("acp-clean");
        let server = node(&dir, "500");
        let mut acp = adapter(&dir, &server);
        let session = acp.new_session(1);
        for (id, goal_id) in [(2, 1), (3, 2)] {
            let (response, chunks) = acp.prompt(id, &session, &["same"]);
            assert_eq!(response["result"]["stopReason"], "end_turn");
            let output = goal(&server, goal_id)["output"]
                .as_str()
                .unwrap()
                .to_owned();
            assert_eq!(chunks.concat(), output);
        }
    }

    /// Chunk texts of `updates`, joined.
    fn joined(updates: &[Value]) -> String {
        updates.iter().map(chunk_text).collect()
    }

    fn cancel_note(acp: &mut Acp, session: &str) {
        acp.cancel(session);
    }

    #[test]
    fn cancel_mid_turn_answers_cancelled_and_the_next_prompt_runs_on_a_new_goal() {
        let dir = TempDir::new("acp-cancel-mid");
        let server = node(&dir, "100");
        let mut acp = adapter(&dir, &server);
        let session = acp.new_session(1);
        prompt_request(&mut acp, 2, &session);
        let first = acp.recv();
        assert_eq!(first["method"], "session/update", "{first}");
        cancel_note(&mut acp, &session);
        let (response, rest) = acp.read_until(2);
        assert_eq!(response["result"]["stopReason"], "cancelled", "{response}");
        let cancelled = goal(&server, 1);
        assert_eq!(cancelled["status"], "cancelled");
        let text = format!("{}{}", chunk_text(&first), joined(&rest));
        assert_eq!(text, cancelled["output"].as_str().unwrap());
        let (response, chunks) = acp.prompt(3, &session, &["next"]);
        assert_eq!(response["result"]["stopReason"], "end_turn", "{response}");
        assert_eq!(
            chunks.concat(),
            goal(&server, 2)["output"].as_str().unwrap()
        );
        assert_eq!(goal(&server, 1)["status"], "cancelled");
        assert!(acp.finish().success());
    }

    #[test]
    fn cancel_right_after_the_prompt_answers_cancelled() {
        let dir = TempDir::new("acp-cancel-early");
        let server = node(&dir, "100");
        let mut acp = adapter(&dir, &server);
        let session = acp.new_session(1);
        prompt_request(&mut acp, 2, &session);
        cancel_note(&mut acp, &session);
        let (response, _) = acp.read_until(2);
        assert_eq!(response["result"]["stopReason"], "cancelled", "{response}");
        assert_eq!(goal(&server, 1)["status"], "cancelled");
        assert!(acp.finish().success());
    }

    #[test]
    fn cancel_on_an_idle_session_does_not_stick() {
        let dir = TempDir::new("acp-cancel-idle");
        let server = node(&dir, "500");
        let mut acp = adapter(&dir, &server);
        let session = acp.new_session(1);
        cancel_note(&mut acp, &session);
        let (response, _) = acp.prompt(2, &session, &["hi"]);
        assert_eq!(response["result"]["stopReason"], "end_turn", "{response}");
        assert!(acp.finish().success());
    }

    #[test]
    fn cancel_for_an_unknown_session_writes_nothing() {
        let dir = TempDir::new("acp-cancel-unknown");
        let server = node(&dir, "500");
        let mut acp = adapter(&dir, &server);
        cancel_note(&mut acp, "99");
        assert!(acp.recv_within(Duration::from_millis(300)).is_none());
        let session = acp.new_session(1);
        let (response, _) = acp.prompt(2, &session, &["hi"]);
        assert_eq!(response["result"]["stopReason"], "end_turn", "{response}");
        assert!(acp.finish().success());
    }

    #[test]
    fn session_cancel_with_an_id_is_method_not_found() {
        let dir = TempDir::new("acp-cancel-request");
        let server = node(&dir, "500");
        let mut acp = adapter(&dir, &server);
        let session = acp.new_session(1);
        let (response, _) = acp.call(2, "session/cancel", json!({"sessionId": session}));
        assert_eq!(error_code(&response), -32601, "{response}");
        assert!(acp.finish().success());
    }

    /// A stand-in node on which session 1 has goal 1, which reports `status` with no output once
    /// `open` is set. Returns its port.
    fn fake_node(status: &'static str, open: &Arc<AtomicBool>) -> u16 {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        let open = Arc::clone(open);
        std::thread::spawn(move || {
            for mut stream in listener.incoming().flatten() {
                let open = Arc::clone(&open);
                std::thread::spawn(move || {
                    let mut data = Vec::new();
                    let mut buf = [0u8; 4096];
                    while !data.windows(4).any(|w| w == b"\r\n\r\n") {
                        match stream.read(&mut buf) {
                            Ok(n) if n > 0 => data.extend_from_slice(&buf[..n]),
                            _ => return,
                        }
                    }
                    let head = String::from_utf8_lossy(&data).into_owned();
                    let body = if head.starts_with("POST /sessions/2/") {
                        "{\"goal\":2}".to_owned()
                    } else if head.starts_with("POST /sessions/") {
                        "{\"goal\":1}".to_owned()
                    } else if head.starts_with("GET /sessions/") {
                        "{\"turns\":[{\"goal\":1,\"prompt\":\"p\"}]}".to_owned()
                    } else if head.starts_with("GET /goals/2") {
                        "{\"status\":\"done\",\"output\":\"\"}".to_owned()
                    } else if head.starts_with("POST /sessions") {
                        "{\"id\":1}".to_owned()
                    } else {
                        let deadline = Instant::now() + Duration::from_secs(10);
                        while !open.load(Ordering::SeqCst) && Instant::now() < deadline {
                            std::thread::sleep(Duration::from_millis(10));
                        }
                        let (output, error) = match status {
                            "failed" => ("partial", "\"boom\""),
                            _ => ("", "null"),
                        };
                        format!(
                            "{{\"status\":\"{status}\",\"output\":\"{output}\",\"error\":{error}}}"
                        )
                    };
                    let reply = format!(
                        "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                        body.len()
                    );
                    let _ = stream.write_all(reply.as_bytes());
                });
            }
        });
        port
    }

    /// Read until the responses to `ids` have all arrived. Returns the responses by id
    /// and every chunk as (arrival position, session id, text).
    fn read_both(acp: &mut Acp, ids: [u64; 2]) -> (Vec<Value>, Vec<(usize, String, String)>) {
        let mut responses = Vec::new();
        let mut chunks = Vec::new();
        for position in 0.. {
            if responses.len() == ids.len() {
                break;
            }
            let message = acp.recv();
            if ids.contains(&message["id"].as_u64().unwrap_or(0)) {
                responses.push(message);
            } else {
                let session = message["params"]["sessionId"].as_str().unwrap().to_owned();
                chunks.push((position, session, chunk_text(&message)));
            }
        }
        (responses, chunks)
    }

    fn response_for(responses: &[Value], id: u64) -> &Value {
        responses.iter().find(|r| r["id"] == id).unwrap()
    }

    /// The goal id of the first turn of session `n` (as numbered by the node).
    fn goal_of_session(server: &Server, n: u64) -> u64 {
        session_of(server, n)["turns"][0]["goal"].as_u64().unwrap()
    }

    fn session_text(chunks: &[(usize, String, String)], session: &str) -> String {
        chunks
            .iter()
            .filter(|c| c.1 == session)
            .map(|c| c.2.as_str())
            .collect()
    }

    fn positions(chunks: &[(usize, String, String)], session: &str) -> (usize, usize) {
        let all: Vec<usize> = chunks
            .iter()
            .filter(|c| c.1 == session)
            .map(|c| c.0)
            .collect();
        (*all.first().unwrap(), *all.last().unwrap())
    }

    fn two_prompts(acp: &mut Acp) -> (String, String) {
        let a = acp.new_session(1);
        let b = acp.new_session(2);
        for (id, session, words) in [(3, &a, "alpha"), (4, &b, "bravo")] {
            let params = json!({"sessionId": session, "prompt": [text(words)]});
            acp.request(id, "session/prompt", params);
        }
        (a, b)
    }

    #[test]
    fn two_sessions_stream_in_parallel_each_byte_identical_to_its_goal() {
        let dir = TempDir::new("acp-parallel");
        let server = node(&dir, "500");
        let mut acp = adapter(&dir, &server);
        let (a, b) = two_prompts(&mut acp);
        let (responses, chunks) = read_both(&mut acp, [3, 4]);
        for id in [3, 4] {
            let response = response_for(&responses, id);
            assert_eq!(response["result"]["stopReason"], "end_turn", "{response}");
        }
        let (a_first, a_last) = positions(&chunks, &a);
        let (b_first, b_last) = positions(&chunks, &b);
        assert!(
            a_first < b_last && b_first < a_last,
            "streams did not overlap"
        );
        let a_goal = goal(&server, goal_of_session(&server, 1));
        let b_goal = goal(&server, goal_of_session(&server, 2));
        assert_eq!(
            session_text(&chunks, &a),
            a_goal["output"].as_str().unwrap()
        );
        assert_eq!(
            session_text(&chunks, &b),
            b_goal["output"].as_str().unwrap()
        );
        assert_ne!(a_goal["output"], b_goal["output"]);
    }

    #[test]
    fn cancelling_one_of_two_running_sessions_leaves_the_other_to_finish() {
        let dir = TempDir::new("acp-parallel-cancel");
        let server = node(&dir, "100");
        let mut acp = adapter(&dir, &server);
        let (a, b) = two_prompts(&mut acp);
        let mut chunks = Vec::new();
        while !(chunks.iter().any(|c: &(usize, String, String)| c.1 == a)
            && chunks.iter().any(|c| c.1 == b))
        {
            let message = acp.recv();
            assert_eq!(message["method"], "session/update", "{message}");
            let session = message["params"]["sessionId"].as_str().unwrap().to_owned();
            chunks.push((0, session, chunk_text(&message)));
        }
        acp.cancel(&a);
        let (responses, rest) = read_both(&mut acp, [3, 4]);
        chunks.extend(rest);
        assert_eq!(
            response_for(&responses, 3)["result"]["stopReason"],
            "cancelled"
        );
        assert_eq!(
            response_for(&responses, 4)["result"]["stopReason"],
            "end_turn"
        );
        let a_goal = goal(&server, goal_of_session(&server, 1));
        let b_goal = goal(&server, goal_of_session(&server, 2));
        assert_eq!(a_goal["status"], "cancelled");
        assert_eq!(b_goal["status"], "done");
        assert_eq!(
            session_text(&chunks, &a),
            a_goal["output"].as_str().unwrap()
        );
        assert_eq!(
            session_text(&chunks, &b),
            b_goal["output"].as_str().unwrap()
        );
    }

    #[test]
    fn a_failed_goal_streams_its_text_then_ends_the_turn_with_its_code() {
        let dir = TempDir::new("acp-failed");
        let port = fake_node("failed", &Arc::new(AtomicBool::new(true)));
        let mut acp = Acp::start(&dir.0, &[url(port)]);
        let session = acp.new_session(1);
        let (response, chunks) = acp.prompt(2, &session, &["hi"]);
        assert_eq!(chunks, ["partial"]);
        assert_eq!(response["error"]["code"], -32000, "{response}");
        assert_eq!(response["error"]["message"], "boom", "{response}");
        assert_eq!(response["error"]["data"]["error"], "boom", "{response}");
        assert!(acp.finish().success());
    }

    const THROWS: &str = "throw new Error('no')";
    const WRITES: &str = "Deno.core.ops.op_rs3_insert(JSON.stringify({ hit: 'yes' })); 'ran'";

    fn store_scripts(server: &Server) {
        let seed = json!({"bad": THROWS, "good": WRITES}).to_string();
        server.request("POST", "/insert", &seed);
    }

    fn assert_advertised(advert: &Value, session: &str) {
        assert_eq!(advert["method"], "session/update", "{advert}");
        assert_eq!(advert["params"]["sessionId"], session, "{advert}");
        let update = &advert["params"]["update"];
        assert_eq!(update["sessionUpdate"], "available_commands_update");
        let run = &update["availableCommands"][0];
        assert_eq!(run["name"], "run", "{advert}");
        assert_eq!(run["input"]["hint"], "<key>", "{advert}");
    }

    #[test]
    fn run_is_advertised_after_a_new_session_and_after_a_load() {
        let dir = TempDir::new("acp-advert");
        let server = node(&dir, "500");
        let mut acp = adapter(&dir, &server);
        let session = acp.new_session(1);
        let advert = acp.advertised();
        assert_advertised(&advert, &session);
        let (response, _) = acp.load(2, &session, Duration::from_secs(10));
        assert_eq!(response["result"], Value::Null, "{response}");
        let advert = acp.advertised();
        assert_advertised(&advert, &session);
    }

    #[test]
    fn a_run_prompt_runs_the_stored_script_once_and_streams_its_output() {
        let dir = TempDir::new("acp-run");
        let server = node(&dir, "500");
        store_scripts(&server);
        let mut acp = adapter(&dir, &server);
        let session = acp.new_session(1);
        let (response, chunks) = acp.prompt(2, &session, &["/run good"]);
        assert_eq!(response["result"]["stopReason"], "end_turn", "{response}");
        assert_eq!(
            chunks.concat(),
            goal(&server, 1)["output"].as_str().unwrap()
        );
        assert_eq!(server.request("GET", "/key/hit", ""), "\"yes\"");
        assert!(acp.finish().success());
    }

    #[test]
    fn a_throwing_script_answers_its_code_and_the_next_prompt_still_runs() {
        let dir = TempDir::new("acp-run-throws");
        let server = node(&dir, "500");
        store_scripts(&server);
        let mut acp = adapter(&dir, &server);
        let session = acp.new_session(1);
        let (response, _) = acp.prompt(2, &session, &["/run bad"]);
        assert_eq!(error_code(&response), -32000, "{response}");
        assert_eq!(response["error"]["message"], "js_error", "{response}");
        assert_eq!(response["error"]["data"]["error"], "js_error", "{response}");
        let (response, _) = acp.prompt(3, &session, &["next"]);
        assert_eq!(response["result"]["stopReason"], "end_turn", "{response}");
        assert!(acp.finish().success());
    }

    #[test]
    fn an_unknown_script_key_answers_unknown_script_and_records_no_turn() {
        let dir = TempDir::new("acp-run-unknown");
        let server = node(&dir, "500");
        let mut acp = adapter(&dir, &server);
        let session = acp.new_session(1);
        let (response, _) = acp.prompt(2, &session, &["/run nothing"]);
        assert_eq!(error_code(&response), -32003, "{response}");
        assert_eq!(response["error"]["message"], "unknown_script", "{response}");
        assert_eq!(
            response["error"]["data"]["error"], "unknown_script",
            "{response}"
        );
        let shown = server.request("GET", &format!("/sessions/{session}"), "");
        assert_eq!(
            serde_json::from_str::<Value>(&shown).unwrap(),
            json!({"turns": []})
        );
        let (response, _) = acp.prompt(3, &session, &["next"]);
        assert_eq!(response["result"]["stopReason"], "end_turn", "{response}");
        assert!(acp.finish().success());
    }

    #[test]
    fn a_goal_that_ends_after_the_cancel_arrived_answers_cancelled() {
        let dir = TempDir::new("acp-cancel-late");
        let open = Arc::new(AtomicBool::new(false));
        let port = fake_node("done", &open);
        let mut acp = Acp::start(&dir.0, &[url(port)]);
        let session = acp.new_session(1);
        prompt_request(&mut acp, 2, &session);
        acp.cancel(&session);
        std::thread::sleep(Duration::from_millis(300));
        open.store(true, Ordering::SeqCst);
        let (response, _) = acp.read_until(2);
        assert_eq!(response["result"]["stopReason"], "cancelled", "{response}");
        assert!(acp.finish().success());
    }

    #[test]
    fn other_first_arguments_still_start_the_server() {
        let dir = TempDir::new("acp-serve");
        let port = crate::common::free_port();
        let mut command = crate::common::rs3(port, &dir.0, None);
        command.arg("serve");
        let _server = Server::spawn(command, port, Duration::from_secs(30));
    }

    fn post_turn(server: &Server, session: &str, turn: u64, prompt: &str) {
        let body = json!({"turn": turn, "prompt": prompt}).to_string();
        let (status, reply) =
            server.request_status("POST", &format!("/sessions/{session}/turns"), &body);
        assert!(status < 300, "{status} {reply}");
    }

    fn wait_done(server: &Server, id: u64) {
        let deadline = Instant::now() + Duration::from_secs(20);
        while goal(server, id)["status"] != "done" {
            assert!(Instant::now() < deadline, "goal {id} never finished");
            std::thread::sleep(Duration::from_millis(50));
        }
    }

    #[test]
    fn a_prompt_on_a_taken_turn_streams_the_committed_turn_then_its_own() {
        let dir = TempDir::new("acp-conflict");
        let server = node(&dir, "500");
        let mut acp = adapter(&dir, &server);
        let session = acp.new_session(1);
        post_turn(&server, &session, 0, "other");
        wait_done(&server, 1);
        let (response, chunks) = acp.prompt(2, &session, &["mine"]);
        assert_eq!(response["result"]["stopReason"], "end_turn", "{response}");
        let out = |id| goal(&server, id)["output"].as_str().unwrap().to_owned();
        assert_eq!(chunks.concat(), format!("{}{}", out(1), out(2)));
        let turns = session_of(&server, 1);
        let prompts: Vec<&str> = turns["turns"]
            .as_array()
            .unwrap()
            .iter()
            .map(|t| t["prompt"].as_str().unwrap())
            .collect();
        assert_eq!(prompts, ["other", "mine"]);
        assert!(acp.finish().success());
    }

    #[test]
    fn two_taken_turns_are_streamed_in_order_before_the_prompt_adds_one_goal() {
        let dir = TempDir::new("acp-conflict-two");
        let server = node(&dir, "500");
        let mut acp = adapter(&dir, &server);
        let session = acp.new_session(1);
        post_turn(&server, &session, 0, "a");
        post_turn(&server, &session, 1, "b");
        wait_done(&server, 1);
        wait_done(&server, 2);
        let (response, chunks) = acp.prompt(2, &session, &["mine"]);
        assert_eq!(response["result"]["stopReason"], "end_turn", "{response}");
        let out = |id| goal(&server, id)["output"].as_str().unwrap().to_owned();
        assert_eq!(chunks.concat(), format!("{}{}{}", out(1), out(2), out(3)));
        assert_eq!(session_of(&server, 1)["turns"].as_array().unwrap().len(), 3);
        assert_eq!(server.request_status("GET", "/goals/4", "").0, 404);
        assert!(acp.finish().success());
    }

    #[test]
    fn a_taken_turn_cut_off_by_a_dead_node_is_not_repeated_by_the_next_prompt() {
        let dir = TempDir::new("acp-conflict-cut");
        let db = dir.0.join("rs3.db");
        let rate = [("MODEL.TOKENS_PER_SECOND", "100")];
        let server = Server::start(&dir.0, Some(&db), &rate);
        let port = server.port;
        let mut acp = Acp::start_with(&dir.0, &[url(port)], SHORT);
        let session = acp.new_session(1);
        post_turn(&server, &session, 0, "other");
        let mut received = prompt_cut_off_by_a_dead_node(&mut acp, &session, server);
        let server = Server::start_on(port, &dir.0, Some(&db), &rate);
        let (response, chunks) = acp.prompt(3, &session, &["y"]);
        assert_eq!(response["result"]["stopReason"], "end_turn", "{response}");
        received.extend(chunks);
        let out = |id| goal(&server, id)["output"].as_str().unwrap().to_owned();
        assert_eq!(received.concat(), format!("{}{}", out(1), out(2)));
        assert_eq!(session_of(&server, 1)["turns"].as_array().unwrap().len(), 2);
    }
}
