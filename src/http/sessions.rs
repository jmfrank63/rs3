//! Session routes: `POST /sessions` makes a session, `POST /sessions/{id}/turns` starts a turn
//! (resending it answers the same goal), and `GET /sessions/{id}` lists the turns. All answer 503
//! `goals_unavailable` when the server runs without a database.
use actix_web::{HttpResponse, get, post, web};
use serde::Deserialize;

use super::error::ApiError;
use super::goals::{goals, parse_body};
use super::kv::{blocking_failed, read_barrier};
use crate::sessions::TurnOutcome;
use crate::state::AppState;
use crate::store::StoreError;

/// Register the session routes. The shared `AppState` is registered by `http::configure`.
pub fn configure(cfg: &mut web::ServiceConfig) {
    cfg.service(create).service(get_session).service(start_turn);
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct NewTurn {
    turn: u64,
    prompt: String,
}

/// The session id in the path, or `not_found` when it is not a number.
fn session_id(raw: &str) -> Result<u64, ApiError> {
    raw.parse()
        .map_err(|_| ApiError::SessionNotFound(raw.to_owned()))
}

/// `POST /sessions` answers 201 `{"id":n}` once the session is durable.
#[post("/sessions")]
async fn create(state: web::Data<AppState>) -> Result<HttpResponse, ApiError> {
    let goals = goals(&state)?;
    let id = web::block(move || goals.create_session())
        .await
        .map_err(blocking_failed)??;
    Ok(HttpResponse::Created().json(serde_json::json!({ "id": id })))
}

/// `GET /sessions/{id}` answers 200 `{"turns":[{"prompt","goal"}]}` after the read barrier, or 404.
#[get("/sessions/{id}")]
async fn get_session(
    state: web::Data<AppState>,
    path: web::Path<String>,
) -> Result<HttpResponse, ApiError> {
    let goals = goals(&state)?;
    read_barrier(&state).await?;
    let raw = path.into_inner();
    let id = session_id(&raw)?;
    let turns = web::block(move || goals.session(id))
        .await
        .map_err(blocking_failed)??
        .ok_or(ApiError::SessionNotFound(raw))?;
    Ok(HttpResponse::Ok().json(serde_json::json!({ "turns": turns })))
}

/// `POST /sessions/{id}/turns` with `{turn, prompt}` answers 201 `{"goal":n}`, for a new turn and
/// for a resend of a recorded one alike. A turn that skips ahead or changes its prompt answers 409
/// `turn_conflict`; an unknown session, or a `/run <key>` prompt whose key holds no script, 404; a body of the wrong shape 422 `invalid_input`.
#[post("/sessions/{id}/turns")]
async fn start_turn(
    state: web::Data<AppState>,
    path: web::Path<String>,
    body: web::Bytes,
) -> Result<HttpResponse, ApiError> {
    let goals = goals(&state)?;
    read_barrier(&state).await?;
    let raw = path.into_inner();
    let id = session_id(&raw)?;
    let new: NewTurn = parse_body(&body, "turn")?;
    let new_prompt = new.prompt.clone();
    let outcome = web::block(move || goals.start_turn(id, new.turn, new.prompt))
        .await
        .map_err(blocking_failed)??;
    match outcome {
        TurnOutcome::New(goal) | TurnOutcome::Existing(goal) => {
            Ok(HttpResponse::Created().json(serde_json::json!({ "goal": goal })))
        }
        TurnOutcome::Conflict => Err(ApiError::TurnConflict),
        TurnOutcome::NotFound => Err(ApiError::SessionNotFound(raw)),
        TurnOutcome::UnknownScript => Err(ApiError::ScriptNotFound(new_prompt)),
        TurnOutcome::TooLong(why) => Err(StoreError::InvalidInput(why).into()),
    }
}

#[cfg(test)]
mod tests {
    use std::num::NonZeroU32;

    use actix_web::{App, http::StatusCode, test as atest};
    use serde_json::{Value, json};

    use crate::goals::Goals;
    use crate::http::tests::assert_json_error;
    use crate::state::{AppState, test_state};
    use crate::storage::{TempDir, open};

    async fn call(state: &AppState, req: atest::TestRequest) -> (StatusCode, Value) {
        let app =
            atest::init_service(App::new().configure(crate::http::configure(state.clone()))).await;
        let resp = atest::call_service(&app, req.to_request()).await;
        let status = resp.status();
        (status, atest::read_body_json(resp).await)
    }

    fn turn(session: &str, body: Value) -> atest::TestRequest {
        atest::TestRequest::post()
            .uri(&format!("/sessions/{session}/turns"))
            .set_payload(body.to_string())
    }

    #[actix_web::test]
    async fn a_session_records_its_turns_and_refuses_bad_ones() {
        let dir = TempDir::new();
        let db = open(&dir.path().join("db")).unwrap();
        let goals = Goals::open(db, NonZeroU32::new(1_000_000).unwrap()).unwrap();
        let state = AppState {
            goals: Some(goals),
            ..test_state()
        };

        let (status, body) = call(&state, atest::TestRequest::post().uri("/sessions")).await;
        assert_eq!((status, body), (StatusCode::CREATED, json!({"id": 1})));

        let first = json!({"turn": 0, "prompt": "hi"});
        let (status, body) = call(&state, turn("1", first.clone())).await;
        assert_eq!((status, body), (StatusCode::CREATED, json!({"goal": 1})));
        let (status, body) = call(&state, turn("1", first)).await;
        assert_eq!((status, body), (StatusCode::CREATED, json!({"goal": 1})));

        let (status, body) = call(&state, atest::TestRequest::get().uri("/sessions/1")).await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(body, json!({"turns": [{"prompt": "hi", "goal": 1}]}));

        let skip = turn("1", json!({"turn": 2, "prompt": "hi"}));
        let app =
            atest::init_service(App::new().configure(crate::http::configure(state.clone()))).await;
        let resp = atest::call_service(&app, skip.to_request()).await;
        assert_json_error(resp, StatusCode::CONFLICT, "turn_conflict").await;
        let resp = atest::call_service(
            &app,
            turn("9", json!({"turn": 0, "prompt": "p"})).to_request(),
        )
        .await;
        assert_json_error(resp, StatusCode::NOT_FOUND, "not_found").await;
        let resp = atest::call_service(
            &app,
            atest::TestRequest::get().uri("/sessions/9").to_request(),
        )
        .await;
        assert_json_error(resp, StatusCode::NOT_FOUND, "not_found").await;
    }
}
