//! Goal routes: `POST /goals` creates a generation goal, or a script goal that
//! runs the script stored under a key (pinned at creation), and `GET /goals/{id}`
//! reports its status and committed output, and `DELETE /goals/{id}` cancels it
//! durably and answers the same way. All answer 503 `goals_unavailable`
//! when the server runs without a database.
use actix_web::{HttpResponse, delete, get, post, web};
use serde::Deserialize;

use super::error::ApiError;
use super::kv::blocking_failed;
use crate::goals::{GoalSpec, GoalStatus, Goals};
use crate::journal::JournalError;
use crate::state::AppState;
use crate::store::StoreError;

/// Register the goal routes. The shared `AppState` is registered by `http::configure`.
pub fn configure(cfg: &mut web::ServiceConfig) {
    cfg.service(create).service(get_goal).service(cancel_goal);
}

/// What a client sends to create a goal: generation fields, or a script key.
#[derive(Deserialize)]
#[serde(untagged)]
enum NewGoal {
    Generation(Generation),
    Script(ScriptGoal),
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Generation {
    priority: i64,
    budget: i64,
    seed: u64,
    prompt: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ScriptGoal {
    priority: i64,
    script: String,
}

fn goals(state: &AppState) -> Result<Goals, ApiError> {
    state.goals.clone().ok_or(ApiError::GoalsUnavailable)
}

/// `POST /goals` creates a goal and answers 201 `{"id":n}` once it is durable.
/// Invalid UTF-8 or JSON answers 400 `malformed_json`; JSON of the wrong shape
/// or a budget below 1 answers 422 `invalid_input`.
#[post("/goals")]
async fn create(state: web::Data<AppState>, body: web::Bytes) -> Result<HttpResponse, ApiError> {
    let goals = goals(&state)?;
    let text = std::str::from_utf8(&body)
        .map_err(|e| StoreError::MalformedJson(format!("body is not valid UTF-8: {e}")))?;
    let value: serde_json::Value =
        serde_json::from_str(text).map_err(|e| StoreError::MalformedJson(e.to_string()))?;
    let new: NewGoal = serde_json::from_value(value)
        .map_err(|e| StoreError::InvalidInput(format!("invalid goal: {e}")))?;
    let id = match new {
        NewGoal::Generation(g) => {
            let budget = u64::try_from(g.budget)
                .ok()
                .filter(|b| *b >= 1)
                .ok_or_else(|| StoreError::InvalidInput("budget must be at least 1".into()))?;
            let spec = GoalSpec {
                priority: g.priority,
                budget,
                seed: g.seed,
                prompt: g.prompt,
            };
            web::block(move || goals.create(spec)).await
        }
        NewGoal::Script(g) => {
            let source = state.store.get(&g.script)?;
            web::block(move || goals.create_script(g.priority, g.script, source)).await
        }
    }
    .map_err(blocking_failed)??;
    Ok(HttpResponse::Created().json(serde_json::json!({ "id": id })))
}

/// Run `act` on goal `path` off the async threads and answer 200
/// `{status, output, error}`; an id that is not a goal answers 404 `not_found`.
async fn goal_status(
    state: web::Data<AppState>,
    path: web::Path<String>,
    act: fn(&Goals, u64) -> Result<Option<GoalStatus>, JournalError>,
) -> Result<HttpResponse, ApiError> {
    let goals = goals(&state)?;
    let raw = path.into_inner();
    let id: u64 = raw
        .parse()
        .map_err(|_| ApiError::GoalNotFound(raw.clone()))?;
    let status = web::block(move || act(&goals, id))
        .await
        .map_err(blocking_failed)??
        .ok_or(ApiError::GoalNotFound(raw))?;
    Ok(HttpResponse::Ok().json(serde_json::json!({
        "status": status.status,
        "output": status.output,
        "error": status.error,
    })))
}

/// `GET /goals/{id}` answers 200 `{status, output, error}`, or 404 `not_found`.
#[get("/goals/{id}")]
async fn get_goal(
    state: web::Data<AppState>,
    path: web::Path<String>,
) -> Result<HttpResponse, ApiError> {
    goal_status(state, path, Goals::status).await
}

/// `DELETE /goals/{id}` durably cancels the goal and answers like GET; a goal
/// that already ended keeps its status, and the committed output stays readable.
#[delete("/goals/{id}")]
async fn cancel_goal(
    state: web::Data<AppState>,
    path: web::Path<String>,
) -> Result<HttpResponse, ApiError> {
    goal_status(state, path, Goals::cancel).await
}

#[cfg(test)]
mod tests {
    use std::num::{NonZeroU32, NonZeroUsize};
    use std::time::{Duration, Instant};

    use actix_web::{App, dev::ServiceResponse, http::StatusCode, test as atest, web};
    use redb::{Database, ReadableTable, TableDefinition};
    use serde_json::{Value, json};

    use crate::goals::Goals;
    use crate::http::tests::assert_json_error;
    use crate::state::{AppState, test_state};
    use crate::storage::{TempDir, open};
    use crate::store::Store;

    const HEADS: TableDefinition<u64, &[u8]> = TableDefinition::new("journal_heads");
    const RECORDS: TableDefinition<(u64, u64), &[u8]> = TableDefinition::new("journal_records");

    struct Fixture {
        _dir: TempDir,
        db: std::sync::Arc<Database>,
        state: AppState,
    }

    /// A durable state; `workers` of `None` leaves goals queued.
    fn fixture(rate: u32, workers: Option<usize>) -> Fixture {
        let dir = TempDir::new();
        let db = open(&dir.path().join("db")).unwrap();
        let goals = Goals::open(db.clone(), NonZeroU32::new(rate).unwrap()).unwrap();
        let store = Store::durable(db.clone()).unwrap();
        goals.enable_scripts(store.clone(), crate::js::test_executor());
        if let Some(n) = workers {
            goals.start_workers(NonZeroUsize::new(n).unwrap());
        }
        let base = test_state();
        let state = AppState {
            goals: Some(goals),
            ..AppState::new(store, (*base.settings).clone(), base.js.clone())
        };
        Fixture {
            _dir: dir,
            db,
            state,
        }
    }

    /// One request against a fresh service built on `state`.
    async fn call(state: &AppState, req: atest::TestRequest) -> ServiceResponse {
        let app =
            atest::init_service(App::new().configure(crate::http::configure(state.clone()))).await;
        atest::call_service(&app, req.to_request()).await
    }

    fn goal_body(budget: i64) -> Value {
        json!({"priority": 0, "budget": budget, "seed": 7, "prompt": "hi"})
    }

    fn get_request(id: impl std::fmt::Display) -> atest::TestRequest {
        atest::TestRequest::get().uri(&format!("/goals/{id}"))
    }

    fn delete_request(id: impl std::fmt::Display) -> atest::TestRequest {
        atest::TestRequest::delete().uri(&format!("/goals/{id}"))
    }

    async fn delete(state: &AppState, id: u64) -> Value {
        let resp = call(state, delete_request(id)).await;
        assert_eq!(resp.status(), StatusCode::OK);
        atest::read_body_json(resp).await
    }

    async fn create(state: &AppState, body: &Value) -> u64 {
        let req = atest::TestRequest::post()
            .uri("/goals")
            .set_payload(body.to_string());
        let resp = call(state, req).await;
        assert_eq!(resp.status(), StatusCode::CREATED);
        let body: Value = atest::read_body_json(resp).await;
        body["id"].as_u64().unwrap()
    }

    async fn get(state: &AppState, id: u64) -> Value {
        let resp = call(state, get_request(id)).await;
        assert_eq!(resp.status(), StatusCode::OK);
        atest::read_body_json(resp).await
    }

    async fn wait_for(state: &AppState, id: u64, status: &str) -> Value {
        let deadline = Instant::now() + Duration::from_secs(20);
        loop {
            let goal = get(state, id).await;
            if goal["status"] == status {
                return goal;
            }
            assert!(
                Instant::now() < deadline,
                "goal {id} never reached {status}: {goal}"
            );
            actix_web::rt::time::sleep(Duration::from_millis(5)).await;
        }
    }

    fn tokens(goal: &Value) -> usize {
        goal["output"].as_str().unwrap().split_whitespace().count()
    }

    const VALID: &str = r#"{"priority":0,"budget":1,"seed":1,"prompt":"p"}"#;

    fn post(body: impl Into<web::Bytes>) -> atest::TestRequest {
        atest::TestRequest::post().uri("/goals").set_payload(body)
    }

    async fn internal_error_body(resp: ServiceResponse) {
        assert_eq!(resp.status(), StatusCode::INTERNAL_SERVER_ERROR);
        assert_eq!(
            atest::read_body(resp).await,
            web::Bytes::from_static(br#"{"error":"internal","message":"internal error"}"#)
        );
    }

    #[actix_web::test]
    async fn a_created_goal_finishes_with_exactly_its_budget() {
        let f = fixture(1_000_000, Some(2));
        let id = create(&f.state, &goal_body(21)).await;
        let goal = wait_for(&f.state, id, "done").await;
        assert_eq!(tokens(&goal), 21);
        assert_eq!(goal["error"], Value::Null);
    }

    #[actix_web::test]
    async fn one_worker_finishes_two_goals() {
        let f = fixture(1_000_000, Some(1));
        let a = create(&f.state, &goal_body(10)).await;
        let b = create(&f.state, &goal_body(12)).await;
        assert_eq!(tokens(&wait_for(&f.state, a, "done").await), 10);
        assert_eq!(tokens(&wait_for(&f.state, b, "done").await), 12);
    }

    #[actix_web::test]
    async fn a_goal_is_queued_without_workers() {
        let f = fixture(1_000_000, None);
        let id = create(&f.state, &goal_body(5)).await;
        assert_eq!(
            get(&f.state, id).await,
            json!({"status": "queued", "output": "", "error": null})
        );
    }

    #[actix_web::test]
    async fn a_slow_goal_is_seen_running() {
        let f = fixture(10, Some(1));
        let id = create(&f.state, &goal_body(16)).await;
        wait_for(&f.state, id, "running").await;
        wait_for(&f.state, id, "done").await;
    }

    #[actix_web::test]
    async fn bad_bodies_are_rejected() {
        let f = fixture(1_000_000, None);
        let resp = call(&f.state, post("not json")).await;
        assert_json_error(resp, StatusCode::BAD_REQUEST, "malformed_json").await;
        let resp = call(&f.state, post(web::Bytes::from_static(&[0xff, 0xfe]))).await;
        assert_json_error(resp, StatusCode::BAD_REQUEST, "malformed_json").await;
        for body in [
            r#"{"priority":0,"budget":0,"seed":1,"prompt":"p"}"#,
            r#"{"priority":0,"budget":-3,"seed":1,"prompt":"p"}"#,
            r#"{"priority":0,"seed":1,"prompt":"p"}"#,
            r#"{"priority":0,"budget":"5","seed":1,"prompt":"p"}"#,
            r#"[1,2]"#,
        ] {
            let resp = call(&f.state, post(body)).await;
            assert_json_error(resp, StatusCode::UNPROCESSABLE_ENTITY, "invalid_input").await;
        }
        // None of them created a goal.
        let resp = call(&f.state, get_request(1)).await;
        assert_json_error(resp, StatusCode::NOT_FOUND, "not_found").await;
    }

    #[actix_web::test]
    async fn deleting_a_queued_goal_cancels_it() {
        let f = fixture(1_000_000, None);
        let id = create(&f.state, &goal_body(3)).await;
        let want = json!({"status": "cancelled", "output": "", "error": null});
        assert_eq!(delete(&f.state, id).await, want);
        assert_eq!(get(&f.state, id).await, want);
        assert_eq!(delete(&f.state, id).await, want);
    }

    #[actix_web::test]
    async fn deleting_a_running_goal_keeps_its_committed_output() {
        let f = fixture(80, Some(1));
        let id = create(&f.state, &goal_body(100)).await;
        wait_for(&f.state, id, "running").await;
        let cancelled = delete(&f.state, id).await;
        assert_eq!(cancelled["status"], "cancelled");
        assert!(tokens(&cancelled) < 100);
        actix_web::rt::time::sleep(Duration::from_millis(300)).await;
        assert_eq!(get(&f.state, id).await, cancelled);
    }

    #[actix_web::test]
    async fn deleting_a_done_goal_leaves_it_done() {
        let f = fixture(1_000_000, Some(1));
        let id = create(&f.state, &goal_body(2)).await;
        let done = wait_for(&f.state, id, "done").await;
        assert_eq!(delete(&f.state, id).await, done);
    }

    #[actix_web::test]
    async fn deleting_a_script_goal_answers_cancelled_with_null_output() {
        let f = fixture(1_000_000, None);
        put(&f.state, "job", "1").await;
        let id = create(&f.state, &script_body("job")).await;
        assert_eq!(
            delete(&f.state, id).await,
            json!({"status": "cancelled", "output": null, "error": null})
        );
    }

    #[actix_web::test]
    async fn unknown_and_malformed_ids_are_json_404s() {
        let f = fixture(1_000_000, None);
        for id in ["42", "abc", "99999999999999999999999"] {
            for req in [get_request(id), delete_request(id)] {
                let resp = call(&f.state, req).await;
                let body = assert_json_error(resp, StatusCode::NOT_FOUND, "not_found").await;
                assert_eq!(body["message"], format!("goal not found: {id}"));
            }
        }
    }

    #[actix_web::test]
    async fn without_a_database_all_routes_answer_503() {
        let state = test_state();
        for req in [post(VALID), get_request(1), delete_request(1)] {
            let resp = call(&state, req).await;
            assert_json_error(resp, StatusCode::SERVICE_UNAVAILABLE, "goals_unavailable").await;
        }
    }

    #[actix_web::test]
    async fn a_broken_journal_answers_500_without_host_details() {
        let f = fixture(1_000_000, None);
        let txn = f.db.begin_write().unwrap();
        txn.delete_table(HEADS).unwrap();
        txn.open_table(TableDefinition::<&str, &str>::new("journal_heads"))
            .unwrap();
        txn.commit().unwrap();
        for req in [post(VALID), get_request(1), delete_request(1)] {
            internal_error_body(call(&f.state, req).await).await;
        }
    }

    #[actix_web::test]
    async fn a_goal_with_a_corrupt_stream_is_failed_with_no_output() {
        let f = fixture(1_000_000, None);
        let id = create(&f.state, &goal_body(5)).await;
        let txn = f.db.begin_write().unwrap();
        {
            let mut table = txn.open_table(RECORDS).unwrap();
            let mut bytes = table.get((id, 0)).unwrap().unwrap().value().to_vec();
            let last = bytes.len() - 1;
            bytes[last] ^= 1;
            table.insert((id, 0), bytes.as_slice()).unwrap();
        }
        txn.commit().unwrap();
        assert_eq!(
            get(&f.state, id).await,
            json!({"status": "failed", "output": null, "error": "journal_corrupt"})
        );
    }

    #[actix_web::test]
    async fn the_configured_rate_paces_the_tokens() {
        let slow = fixture(100, Some(1));
        let start = Instant::now();
        let id = create(&slow.state, &goal_body(16)).await;
        wait_for(&slow.state, id, "done").await;
        assert!(
            start.elapsed() >= Duration::from_millis(150),
            "16 tokens at 100/s took {:?}",
            start.elapsed()
        );

        let fast = fixture(100_000, Some(1));
        let start = Instant::now();
        let id = create(&fast.state, &goal_body(16)).await;
        wait_for(&fast.state, id, "done").await;
        assert!(start.elapsed() < Duration::from_secs(5));
    }

    async fn put(state: &AppState, key: &str, value: &str) {
        let req = atest::TestRequest::post()
            .uri("/insert")
            .set_payload(json!({ key: value }).to_string());
        assert_eq!(call(state, req).await.status(), StatusCode::OK);
    }

    fn script_body(key: &str) -> Value {
        json!({"priority": 0, "script": key})
    }

    #[actix_web::test]
    async fn a_script_goal_runs_the_stored_script_and_reports_its_output() {
        let f = fixture(1_000_000, Some(1));
        put(
            &f.state,
            "job",
            r#"Deno.core.ops.op_rs3_insert('{"a":"x"}'); Deno.core.ops.op_rs3_get('a') + '!'"#,
        )
        .await;
        let id = create(&f.state, &script_body("job")).await;
        let goal = wait_for(&f.state, id, "done").await;
        assert_eq!(
            goal,
            json!({"status": "done", "output": "x!", "error": null})
        );
        assert_eq!(f.state.store.get("a").unwrap(), "x");
    }

    #[actix_web::test]
    async fn a_missing_script_key_is_404_and_creates_no_goal() {
        let f = fixture(1_000_000, None);
        let resp = call(&f.state, post(script_body("nope").to_string())).await;
        assert_json_error(resp, StatusCode::NOT_FOUND, "not_found").await;
        let resp = call(&f.state, get_request(1)).await;
        assert_json_error(resp, StatusCode::NOT_FOUND, "not_found").await;
    }

    #[actix_web::test]
    async fn bodies_fitting_neither_goal_shape_are_rejected() {
        let f = fixture(1_000_000, None);
        put(&f.state, "job", "1").await;
        for body in [
            r#"{"priority":0,"budget":1,"seed":1,"prompt":"p","script":"job"}"#,
            r#"{"priority":0,"budget":1,"script":"job"}"#,
            r#"{"priority":0,"script":5}"#,
            r#"{"script":"job"}"#,
            r#"{"priority":0,"script":"job","extra":1}"#,
        ] {
            let resp = call(&f.state, post(body)).await;
            assert_json_error(resp, StatusCode::UNPROCESSABLE_ENTITY, "invalid_input").await;
        }
        let resp = call(&f.state, get_request(1)).await;
        assert_json_error(resp, StatusCode::NOT_FOUND, "not_found").await;
    }

    #[actix_web::test]
    async fn script_goals_answer_503_without_a_database() {
        let resp = call(&test_state(), post(script_body("job").to_string())).await;
        assert_json_error(resp, StatusCode::SERVICE_UNAVAILABLE, "goals_unavailable").await;
    }

    #[actix_web::test]
    async fn changing_the_script_key_after_creation_changes_nothing() {
        let f = fixture(1_000_000, None);
        put(&f.state, "job", "'old'").await;
        let id = create(&f.state, &script_body("job")).await;
        put(&f.state, "job", "'new'").await;
        f.state
            .goals
            .as_ref()
            .unwrap()
            .start_workers(NonZeroUsize::new(1).unwrap());
        assert_eq!(wait_for(&f.state, id, "done").await["output"], "old");
    }

    #[actix_web::test]
    async fn a_failing_script_reports_failed_with_js_error_and_no_output() {
        let f = fixture(1_000_000, Some(1));
        put(&f.state, "job", "throw new Error('boom')").await;
        let id = create(&f.state, &script_body("job")).await;
        assert_eq!(
            wait_for(&f.state, id, "failed").await,
            json!({"status": "failed", "output": null, "error": "js_error"})
        );
    }
}
