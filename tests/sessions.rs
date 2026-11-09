//! Sessions and turns through the real binary: a resent turn never makes a second goal.
#[cfg(test)]
mod common;

#[cfg(test)]
mod sessions {
    use std::io::{Read, Write};
    use std::net::TcpStream;
    use std::time::{Duration, Instant};

    use serde_json::{Value, json};

    use crate::common::{Server, TempDir};

    const SLOW: &[(&str, &str)] = &[("MODEL.TOKENS_PER_SECOND", "500")];

    fn start(dir: &TempDir) -> Server {
        Server::start(&dir.0, Some(&dir.0.join("rs3.db")), &[])
    }

    fn json_of(body: &str) -> Value {
        serde_json::from_str(body).unwrap()
    }

    fn new_session(server: &Server) -> u64 {
        let (status, body) = server.request_status("POST", "/sessions", "");
        assert_eq!(status, 201, "{body}");
        json_of(&body)["id"].as_u64().unwrap()
    }

    fn turn(server: &Server, session: u64, turn: u64, prompt: &str) -> (u16, Value) {
        let body = json!({ "turn": turn, "prompt": prompt }).to_string();
        let (status, body) =
            server.request_status("POST", &format!("/sessions/{session}/turns"), &body);
        (status, json_of(&body))
    }

    fn goal_of(server: &Server, session: u64, n: u64, prompt: &str) -> u64 {
        let (status, body) = turn(server, session, n, prompt);
        assert_eq!(status, 201, "{body}");
        body["goal"].as_u64().unwrap()
    }

    fn goal(server: &Server, id: u64) -> (u16, Value) {
        let (status, body) = server.request_status("GET", &format!("/goals/{id}"), "");
        (status, json_of(&body))
    }

    fn output_when_done(server: &Server, id: u64) -> String {
        output_within(server, id, 30)
    }

    fn output_within(server: &Server, id: u64, secs: u64) -> String {
        let deadline = Instant::now() + Duration::from_secs(secs);
        loop {
            let (_, g) = goal(server, id);
            if g["status"] == "done" {
                return g["output"].as_str().unwrap().to_owned();
            }
            assert!(Instant::now() < deadline, "goal {id} never finished: {g}");
            std::thread::sleep(Duration::from_millis(20));
        }
    }

    /// The output of turn `n` of session `session` on a fresh server where earlier turns used `earlier`.
    fn fresh_output(name: &str, earlier: &[&str], session: u64, n: u64, prompt: &str) -> String {
        let dir = TempDir::new(name);
        let server = start(&dir);
        for _ in 0..session {
            new_session(&server);
        }
        for (i, p) in earlier.iter().enumerate() {
            goal_of(&server, session, i as u64, p);
        }
        let id = goal_of(&server, session, n, prompt);
        output_when_done(&server, id)
    }

    fn error_code(body: &Value) -> &str {
        body["error"].as_str().unwrap()
    }

    #[test]
    fn every_turn_boundary_is_answered() {
        let dir = TempDir::new("sess-bounds");
        let server = start(&dir);
        let s = new_session(&server);
        assert_eq!(s, 1);

        // Skipping ahead of an empty session is refused.
        let (status, body) = turn(&server, s, 1, "a");
        assert_eq!((status, error_code(&body)), (409, "turn_conflict"));

        assert_eq!(goal_of(&server, s, 0, "a"), 1);
        // turn == len - 1, same prompt: the same goal; a different prompt: refused.
        assert_eq!(goal_of(&server, s, 0, "a"), 1);
        let (status, body) = turn(&server, s, 0, "b");
        assert_eq!((status, error_code(&body)), (409, "turn_conflict"));
        // turn == len + 1.
        let (status, body) = turn(&server, s, 2, "c");
        assert_eq!((status, error_code(&body)), (409, "turn_conflict"));

        assert_eq!(goal_of(&server, s, 1, "c"), 2);
        // An older turn with its recorded prompt still answers.
        assert_eq!(goal_of(&server, s, 0, "a"), 1);
        assert_eq!(goal_of(&server, s, 1, "c"), 2);
        let (status, body) = turn(&server, s, 1, "a");
        assert_eq!((status, error_code(&body)), (409, "turn_conflict"));

        let (status, session) = server.request_status("GET", "/sessions/1", "");
        assert_eq!(status, 200);
        assert_eq!(
            json_of(&session),
            json!({"turns": [{"prompt": "a", "goal": 1}, {"prompt": "c", "goal": 2}]})
        );

        let (status, body) = turn(&server, 99, 0, "a");
        assert_eq!((status, error_code(&body)), (404, "not_found"));
        let (status, body) = server.request_status("GET", "/sessions/99", "");
        assert_eq!((status, error_code(&json_of(&body))), (404, "not_found"));
    }

    #[test]
    fn a_resent_turn_makes_no_second_goal_while_running_or_after() {
        let dir = TempDir::new("sess-resend");
        let server = Server::start(&dir.0, Some(&dir.0.join("rs3.db")), SLOW);
        let s = new_session(&server);
        let id = goal_of(&server, s, 0, "hello");
        assert_eq!(goal_of(&server, s, 0, "hello"), id);
        assert_eq!(goal(&server, 2).0, 404);

        let output = output_when_done(&server, id);
        assert_eq!(goal_of(&server, s, 0, "hello"), id);
        assert_eq!(goal(&server, 2).0, 404);
        assert_eq!(output_when_done(&server, id), output);
        assert_eq!(output, fresh_output("sess-resend-ref", &[], 1, 0, "hello"));
    }

    #[test]
    fn sessions_and_the_next_id_survive_a_restart() {
        let dir = TempDir::new("sess-restart");
        let server = start(&dir);
        let s = new_session(&server);
        goal_of(&server, s, 0, "hi");
        drop(server);

        let server = start(&dir);
        let (status, session) = server.request_status("GET", "/sessions/1", "");
        assert_eq!(status, 200);
        assert_eq!(
            json_of(&session),
            json!({"turns": [{"prompt": "hi", "goal": 1}]})
        );
        assert_eq!(new_session(&server), 2);
        assert_eq!(goal_of(&server, 1, 0, "hi"), 1);
    }

    #[test]
    fn two_sessions_get_different_ids() {
        let dir = TempDir::new("sess-ids");
        let server = start(&dir);
        assert_ne!(new_session(&server), new_session(&server));
    }

    #[test]
    fn the_seed_is_fixed_by_session_and_turn() {
        let first = fresh_output("sess-seed-a", &[], 1, 0, "p");
        assert_eq!(first, fresh_output("sess-seed-b", &[], 1, 0, "p"));
        assert_ne!(first, fresh_output("sess-seed-c", &["p"], 1, 1, "p"));
        assert_ne!(first, fresh_output("sess-seed-d", &[], 2, 0, "p"));
    }

    #[test]
    fn a_turn_does_not_depend_on_the_prompts_before_it() {
        assert_eq!(
            fresh_output("sess-prior-x", &["x"], 1, 1, "q"),
            fresh_output("sess-prior-y", &["y"], 1, 1, "q")
        );
    }

    #[test]
    fn a_goal_with_a_seed_is_the_same_after_turns() {
        let goal_body = r#"{"priority":0,"budget":50,"seed":7,"prompt":"p"}"#;
        let run = |name: &str, turns_first: bool| {
            let dir = TempDir::new(name);
            let server = start(&dir);
            if turns_first {
                let s = new_session(&server);
                goal_of(&server, s, 0, "p");
            }
            let created = json_of(&server.request("POST", "/goals", goal_body));
            output_when_done(&server, created["id"].as_u64().unwrap())
        };
        assert_eq!(run("sess-plain", false), run("sess-turns", true));
    }

    #[test]
    fn bad_requests_are_refused() {
        let dir = TempDir::new("sess-bad");
        let server = start(&dir);
        let s = new_session(&server);
        let path = format!("/sessions/{s}/turns");

        let (status, body) = server.request_status("POST", &path, "{");
        assert_eq!(
            (status, error_code(&json_of(&body))),
            (400, "malformed_json")
        );
        for bad in [
            r#"{"prompt":"p"}"#,
            r#"{"turn":0}"#,
            r#"{"turn":-1,"prompt":"p"}"#,
            r#"{"turn":0,"prompt":"p","budget":5}"#,
            r#"{"turn":0,"prompt":"p","seed":5}"#,
        ] {
            let (status, body) = server.request_status("POST", &path, bad);
            assert_eq!(
                (status, error_code(&json_of(&body))),
                (422, "invalid_input"),
                "{bad}"
            );
        }

        let raw = b"{\"turn\":0,\"prompt\":\"\xff\"}";
        let mut stream = TcpStream::connect(("127.0.0.1", server.port)).unwrap();
        stream
            .set_read_timeout(Some(Duration::from_secs(15)))
            .unwrap();
        write!(
            stream,
            "POST {path} HTTP/1.1\r\nHost: rs3\r\nConnection: close\r\nContent-Length: {}\r\n\r\n",
            raw.len()
        )
        .unwrap();
        stream.write_all(raw).unwrap();
        let mut response = String::new();
        stream.read_to_string(&mut response).unwrap();
        assert!(response.starts_with("HTTP/1.1 400"), "{response}");
        assert!(response.contains("malformed_json"), "{response}");

        for (method, p) in [("GET", "/sessions/x"), ("POST", "/sessions/x/turns")] {
            let (status, body) = server.request_status(method, p, r#"{"turn":0,"prompt":"p"}"#);
            assert_eq!(
                (status, error_code(&json_of(&body))),
                (404, "not_found"),
                "{p}"
            );
        }
        // Nothing above created a turn.
        let (_, session) = server.request_status("GET", "/sessions/1", "");
        assert_eq!(json_of(&session), json!({"turns": []}));
    }

    #[test]
    fn without_a_database_sessions_are_unavailable() {
        let dir = TempDir::new("sess-nodb");
        let server = Server::start(&dir.0, None, &[]);
        for (method, p) in [
            ("POST", "/sessions"),
            ("GET", "/sessions/1"),
            ("POST", "/sessions/1/turns"),
        ] {
            let (status, body) = server.request_status(method, p, r#"{"turn":0,"prompt":"p"}"#);
            assert_eq!(
                (status, error_code(&json_of(&body))),
                (503, "goals_unavailable"),
                "{p}"
            );
        }
    }

    const JOB: &str = r#"const n = Number(Deno.core.ops.op_rs3_get('count'));
        Deno.core.ops.op_rs3_insert(JSON.stringify({ count: String(n + 1) }));
        'ran'"#;

    fn store_job(server: &Server) {
        let seed = json!({"count": "0", "job": JOB}).to_string();
        server.request("POST", "/insert", &seed);
    }

    fn count(server: &Server) -> String {
        json_of(&server.request("GET", "/key/count", ""))
            .as_str()
            .unwrap()
            .to_owned()
    }

    #[test]
    fn script_turn_runs_the_stored_script_once_and_a_resend_answers_the_same_goal() {
        let dir = TempDir::new("sess-script-turn");
        let server = start(&dir);
        store_job(&server);
        let s = new_session(&server);
        let id = goal_of(&server, s, 0, "/run job");
        assert_eq!(output_within(&server, id, 10), "ran");
        assert_eq!(count(&server), "1");

        server.request("DELETE", "/key/job", "");
        assert_eq!(goal_of(&server, s, 0, "/run job"), id);
        std::thread::sleep(Duration::from_millis(200));
        assert_eq!(count(&server), "1");
        let (_, session) = server.request_status("GET", "/sessions/1", "");
        assert_eq!(
            json_of(&session),
            json!({"turns": [{"prompt": "/run job", "goal": id}]})
        );
        let (status, body) = turn(&server, s, 0, "/run other");
        assert_eq!((status, error_code(&body)), (409, "turn_conflict"));
    }

    #[test]
    fn script_turn_with_a_missing_key_is_refused_and_records_nothing() {
        let dir = TempDir::new("sess-script-missing");
        let server = start(&dir);
        let s = new_session(&server);
        for prompt in ["/run nothing", "/run", "/run "] {
            let (status, body) = turn(&server, s, 0, prompt);
            assert_eq!((status, error_code(&body)), (404, "not_found"), "{prompt}");
        }
        let (_, session) = server.request_status("GET", "/sessions/1", "");
        assert_eq!(json_of(&session), json!({"turns": []}));
        let id = goal_of(&server, s, 0, "hello");
        assert_eq!(id, 1);
    }

    #[test]
    fn script_turn_needs_a_space_after_run_else_it_is_a_generation_turn() {
        let dir = TempDir::new("sess-script-near");
        let server = start(&dir);
        store_job(&server);
        let s = new_session(&server);
        let id = goal_of(&server, s, 0, "/runner job");
        assert_ne!(output_when_done(&server, id), "ran");
        assert_eq!(count(&server), "0");
    }
}
