//! Coding goals through the real binary, with canned model replies from MODEL.REPLIES.
#[cfg(test)]
mod common;

#[cfg(test)]
mod tests {
    use std::time::{Duration, Instant};

    use serde_json::{Value, json};

    use crate::common::{Server, TempDir};

    const PASSING: &str = "function add(a: number, b: number): number { return a + b; }\n\
        test(\"adds\", () => { expect(add(1, 2)).toEqual(3); });";
    const TYPE_ERROR: &str = "const n: number = \"s\";\n\
        test(\"t\", () => { expect(1).toEqual(1); });";
    const FAILING_TEST: &str = "function add(a: number, b: number): number { return a - b; }\n\
        test(\"adds\", () => { expect(add(1, 2)).toEqual(3); });";
    const SPIN: &str = "while (true) {}";

    fn reply(source: &str) -> String {
        json!({"explanation": "x", "files": {"a.ts": source}}).to_string()
    }

    fn start(dir: &TempDir, name: &str, replies: &[String]) -> Server {
        let file = dir.0.join(name);
        std::fs::write(&file, serde_json::to_string(replies).unwrap()).unwrap();
        start_with(dir, &file)
    }

    fn start_with(dir: &TempDir, file: &std::path::Path) -> Server {
        start_env(dir, file, &[])
    }

    fn start_env(dir: &TempDir, file: &std::path::Path, extra: &[(&str, &str)]) -> Server {
        let db = dir.0.join("rs3.redb");
        let mut env = vec![("MODEL.REPLIES", file.to_str().unwrap())];
        env.extend_from_slice(extra);
        Server::start(&dir.0, Some(&db), &env)
    }

    fn create(server: &Server, body: &Value) -> u64 {
        let (status, text) = server.request_status("POST", "/goals", &body.to_string());
        assert_eq!(status, 201, "{text}");
        serde_json::from_str::<Value>(&text).unwrap()["id"]
            .as_u64()
            .unwrap()
    }

    fn code(task: &str, max_iterations: u32) -> Value {
        json!({"kind": "code", "task": task, "spec": "add two numbers", "max_iterations": max_iterations})
    }

    fn get(server: &Server, id: u64) -> Value {
        serde_json::from_str(&server.request("GET", &format!("/goals/{id}"), "")).unwrap()
    }

    fn wait_until(server: &Server, id: u64, ready: impl Fn(&Value) -> bool) -> Value {
        let deadline = Instant::now() + Duration::from_secs(60);
        loop {
            let goal = get(server, id);
            if ready(&goal) {
                return goal;
            }
            assert!(
                Instant::now() < deadline,
                "goal {id} never got there: {goal}"
            );
            std::thread::sleep(Duration::from_millis(20));
        }
    }

    fn finished(server: &Server, id: u64) -> Value {
        wait_until(server, id, |g| g["status"] == "done")
    }

    fn text(v: &Value) -> String {
        v.as_str().unwrap().to_owned()
    }

    #[test]
    fn a_type_error_is_fed_back_and_the_fixed_reply_succeeds() {
        let dir = TempDir::new("code-fix");
        let server = start(&dir, "r.json", &[reply(TYPE_ERROR), reply(PASSING)]);
        let id = create(&server, &code("write add", 3));
        let goal = finished(&server, id);
        assert_eq!(goal["outcome"], "success", "{goal}");
        let iterations = goal["iterations"].as_array().unwrap();
        assert_eq!(iterations.len(), 2, "{goal}");
        assert!(!iterations[0]["diagnostics"].as_array().unwrap().is_empty());
        assert!(iterations[0]["tests"].as_array().unwrap().is_empty());
        assert!(text(&iterations[0]["feedback"]).contains("TS2322:"));
        assert!(text(&iterations[1]["prompt"]).contains("TS2322:"));
        assert!(iterations[1]["diagnostics"].as_array().unwrap().is_empty());
        assert_eq!(iterations[1]["tests"][0]["name"], "adds");
        assert_eq!(iterations[1]["tests"][0]["passed"], true);
        assert_eq!(iterations[1]["reply"], reply(PASSING));
        assert_eq!(iterations[1]["files"]["a.ts"], PASSING);
    }

    const CLOCK_AND_DICE: &str =
        "console.log(Date.now(), String(new Date().getTime()), Math.random());";

    fn logs_on_fresh_node(name: &str, seed: u64) -> Value {
        let dir = TempDir::new(name);
        let server = start(&dir, "r.json", &[reply(CLOCK_AND_DICE)]);
        let mut body = code("log", 1);
        body["seed"] = json!(seed);
        let goal = finished(&server, create(&server, &body));
        goal["iterations"][0]["logs"].clone()
    }

    #[test]
    fn a_guest_sees_the_same_clock_and_dice_on_every_node() {
        let first = logs_on_fresh_node("code-det-a", 5);
        assert!(text(&first[0]).starts_with("log: 0 \"0\" 0."), "{first}");
        assert_eq!(logs_on_fresh_node("code-det-b", 5), first);
        assert_ne!(logs_on_fresh_node("code-det-c", 6), first);
    }

    const THROWER: &str = "test(\"one\", () => {});\nthrow new Error(\"boom\");";

    /// One goal on a fresh node; returns its iterations.
    fn iterations_on_fresh_node(name: &str, replies: &[String], max: u32, recycle: &str) -> Value {
        let dir = TempDir::new(name);
        let file = dir.0.join("r.json");
        std::fs::write(&file, serde_json::to_string(replies).unwrap()).unwrap();
        let server = start_env(&dir, &file, &[("CHECKER.RECYCLE_AFTER", recycle)]);
        let mut body = code("t", max);
        body["seed"] = json!(7);
        body["budget"] = json!(3000);
        let id = create(&server, &body);
        let goal = finished(&server, id);
        assert_eq!(get(&server, id)["status"], "done");
        goal["iterations"].clone()
    }

    #[test]
    fn guests_give_identical_results_on_two_fresh_nodes() {
        let guests = [SPIN, THROWER, CLOCK_AND_DICE];
        for guest in guests {
            let a = iterations_on_fresh_node("code-two-a", &[reply(guest)], 1, "50");
            let b = iterations_on_fresh_node("code-two-b", &[reply(guest)], 1, "50");
            assert_eq!(a, b, "{guest}");
            if guest == SPIN {
                assert!(a[0]["tests"].as_array().unwrap().is_empty(), "{a}");
                assert!(a[0]["logs"].as_array().unwrap().is_empty(), "{a}");
                assert!(text(&a[0]["feedback"]).contains("deadline"), "{a}");
            }
        }
    }

    #[test]
    fn checks_are_identical_across_recycling() {
        let others = [FAILING_TEST, FAILING_TEST];
        let mut replies = vec![];
        for i in 0..9 {
            replies.push(reply(if i % 3 == 0 {
                TYPE_ERROR
            } else {
                others[i % 2]
            }));
        }
        let recycled = iterations_on_fresh_node("code-rec-a", &replies, 9, "2");
        let default = iterations_on_fresh_node("code-rec-b", &replies, 9, "50");
        let diags = |its: &Value| -> Vec<Value> {
            its.as_array()
                .unwrap()
                .iter()
                .map(|i| i["diagnostics"].clone())
                .collect()
        };
        let d = diags(&recycled);
        assert_eq!(d.len(), 9);
        assert!(!d[0].as_array().unwrap().is_empty());
        assert_eq!(d[0], d[3]);
        assert_eq!(d[0], d[6]);
        assert_eq!(d, diags(&default));
    }

    #[test]
    fn a_trapping_reply_is_reported_and_the_next_reply_still_checks() {
        let trap = format!(
            "const x = {}1{};\ntest(\"t\", () => {{}});",
            "(".repeat(1_200),
            ")".repeat(1_200)
        );
        let replies = [reply(&trap), reply(PASSING)];
        let its = iterations_on_fresh_node("code-trap-a", &replies, 3, "50");
        assert_eq!(its[0]["diagnostics"][0]["file"], "(program)", "{its}");
        assert!(
            text(&its[0]["feedback"]).contains("checker trapped"),
            "{its}"
        );
        assert!(its[0]["tests"].as_array().unwrap().is_empty());
        let fresh = iterations_on_fresh_node("code-trap-b", &[reply(PASSING)], 3, "50");
        assert_eq!(its[1]["diagnostics"], fresh[0]["diagnostics"]);
        assert_eq!(its[1]["tests"][0]["passed"], true, "{its}");
    }

    #[test]
    fn a_failing_test_is_fed_back_and_the_iterations_run_out() {
        let dir = TempDir::new("code-fail");
        let server = start(&dir, "r.json", &[reply(FAILING_TEST), reply(PASSING)]);
        let id = create(&server, &code("write add", 1));
        let goal = finished(&server, id);
        assert_eq!(goal["outcome"], "max_iterations", "{goal}");
        let iterations = goal["iterations"].as_array().unwrap();
        assert_eq!(iterations.len(), 1);
        assert_eq!(iterations[0]["tests"][0]["passed"], false);
        let feedback = text(&iterations[0]["feedback"]);
        assert!(feedback.contains("FAILED test \"adds\""), "{feedback}");
    }

    #[test]
    fn one_malformed_reply_gets_a_corrective_retry() {
        let dir = TempDir::new("code-retry");
        let server = start(&dir, "r.json", &["oops".into(), reply(PASSING)]);
        let id = create(&server, &code("write add", 1));
        let goal = finished(&server, id);
        assert_eq!(goal["outcome"], "success", "{goal}");
        let iterations = goal["iterations"].as_array().unwrap();
        assert_eq!(iterations.len(), 1);
        assert_eq!(iterations[0]["attempts"], 2);
        assert!(text(&iterations[0]["prompt"]).contains("respond with ONLY valid JSON"));
    }

    #[test]
    fn two_malformed_replies_end_llm_malformed() {
        let dir = TempDir::new("code-malformed");
        let server = start(
            &dir,
            "r.json",
            &["oops".into(), "again".into(), reply(PASSING)],
        );
        let id = create(&server, &code("write add", 3));
        let goal = finished(&server, id);
        assert_eq!(goal["outcome"], "llm_malformed", "{goal}");
    }

    #[test]
    fn a_rule_breaking_reply_is_fed_back_not_terminal() {
        let dir = TempDir::new("code-violation");
        let server = start(
            &dir,
            "r.json",
            &[reply("import x from 'y';"), reply(PASSING)],
        );
        let id = create(&server, &code("write add", 3));
        let goal = finished(&server, id);
        assert_eq!(goal["outcome"], "success", "{goal}");
        assert!(text(&goal["iterations"][0]["feedback"]).contains("import"));
    }

    #[test]
    fn exhausted_replies_end_llm_error() {
        let dir = TempDir::new("code-exhausted");
        let server = start(&dir, "r.json", &[reply(TYPE_ERROR)]);
        let id = create(&server, &code("write add", 3));
        let goal = finished(&server, id);
        assert_eq!(goal["outcome"], "llm_error", "{goal}");
        assert_eq!(goal["iterations"].as_array().unwrap().len(), 1);
    }

    #[test]
    fn a_reply_cut_at_the_budget_is_malformed_even_on_the_retry() {
        let dir = TempDir::new("code-deadline");
        let server = start(&dir, "r.json", &[reply(PASSING), reply(PASSING)]);
        let body = json!({"kind": "code", "task": "t", "max_iterations": 1, "budget": 20});
        let id = create(&server, &body);
        let goal = finished(&server, id);
        assert_eq!(goal["outcome"], "llm_malformed", "{goal}");
        assert_eq!(goal["iterations"][0]["attempts"], 2);
        // The reply was cut at the budget, not the whole reply.
        assert_eq!(text(&goal["iterations"][0]["reply"]).len(), 20);
    }

    #[test]
    fn a_running_goal_reports_no_outcome_yet() {
        let dir = TempDir::new("code-running");
        let server = start(&dir, "r.json", &[reply(SPIN)]);
        let id = create(&server, &code("spin", 2));
        let goal = wait_until(&server, id, |g| g["status"] == "running");
        assert_eq!(goal["outcome"], Value::Null);
    }

    #[test]
    fn bad_code_goals_are_refused_with_a_json_error() {
        let dir = TempDir::new("code-bad");
        let server = start(&dir, "r.json", &[reply(PASSING)]);
        for body in [
            json!({"kind": "code", "max_iterations": 2}),
            json!({"kind": "code", "task": "", "max_iterations": 2}),
            json!({"kind": "code", "task": "t", "max_iterations": 0}),
            json!({"kind": "code", "task": "t", "max_iterations": 2, "budget": 0}),
            json!({"kind": "other", "task": "t"}),
        ] {
            let (status, text) = server.request_status("POST", "/goals", &body.to_string());
            let error: Value = serde_json::from_str(&text).unwrap();
            assert!(status == 400 || status == 422, "{body}: {status} {text}");
            assert!(
                error["error"].is_string() && error["message"].is_string(),
                "{text}"
            );
        }
    }

    #[test]
    fn a_text_goal_on_the_same_node_still_runs() {
        let dir = TempDir::new("code-text");
        let server = start(&dir, "r.json", &[reply(PASSING)]);
        let body = r#"{"priority":0,"budget":16,"seed":1,"prompt":"p"}"#;
        let created: Value = serde_json::from_str(&server.request("POST", "/goals", body)).unwrap();
        let goal = finished(&server, created["id"].as_u64().unwrap());
        assert_eq!(goal["reason"], "budget");
        assert!(goal.get("outcome").is_none());
    }

    #[test]
    fn a_killed_goal_resumes_only_on_the_replies_it_started_with() {
        let dir = TempDir::new("code-resume");
        let spin = start(&dir, "spin.json", &[reply(SPIN)]);
        let id = create(&spin, &code("spin", 2));
        wait_until(&spin, id, |g| g["status"] == "running");
        drop(spin);
        // Other replies: the goal must not run.
        let other = dir.0.join("other.json");
        std::fs::write(&other, serde_json::to_string(&[reply(PASSING)]).unwrap()).unwrap();
        let elsewhere = start_with(&dir, &other);
        std::thread::sleep(Duration::from_millis(1500));
        let goal = get(&elsewhere, id);
        assert_ne!(goal["status"], "done", "{goal}");
        drop(elsewhere);
        // The same replies: the goal resumes and ends once they run out.
        let again = start_with(&dir, &dir.0.join("spin.json"));
        let goal = finished(&again, id);
        assert_eq!(goal["outcome"], "llm_error", "{goal}");
        assert_eq!(goal["iterations"][0]["tests"], json!([]));
        assert!(goal["iterations"][0]["feedback"].is_string());
    }

    #[test]
    fn a_malformed_reply_that_used_the_whole_budget_still_gets_its_corrective_retry() {
        let junk = "x".repeat(400);
        let run = |name: &str, replies: &[String]| {
            let dir = TempDir::new(name);
            let server = start(&dir, "r.json", replies);
            let mut body = code("t", 1);
            body["budget"] = json!(400);
            finished(&server, create(&server, &body))
        };
        let healed = run("code-retry-a", &[junk.clone(), reply(PASSING)]);
        assert_eq!(healed["outcome"], "success", "{healed}");
        let failed = run("code-retry-b", &[junk.clone(), junk]);
        assert_eq!(failed["outcome"], "llm_malformed", "{failed}");
    }

    #[test]
    fn the_model_is_told_each_guest_log_with_its_level() {
        let src = "console.warn(\"a\", 1);\ntest(\"t\", () => { expect(1).toEqual(2); });";
        let dir = TempDir::new("code-warn");
        let server = start(&dir, "r.json", &[reply(src)]);
        let goal = finished(&server, create(&server, &code("t", 1)));
        let feedback = text(&goal["iterations"][0]["feedback"]);
        assert!(feedback.contains("guest warn: \"a\" 1"), "{feedback}");
        assert!(
            feedback.find("FAILED test").unwrap() < feedback.find("guest warn").unwrap(),
            "{feedback}"
        );
    }

    #[test]
    fn a_budget_beyond_the_model_context_is_refused() {
        let dir = TempDir::new("code-big-budget");
        let server = start(&dir, "r.json", &[reply(PASSING)]);
        let mut body = code("t", 1);
        body["budget"] = json!(4097);
        let (status, text) = server.request_status("POST", "/goals", &body.to_string());
        assert_eq!(status, 422, "{text}");
        assert!(text.contains("invalid_input"), "{text}");
        body["budget"] = json!(4096);
        let (status, text) = server.request_status("POST", "/goals", &body.to_string());
        assert_eq!(status, 201, "{text}");
    }
}
