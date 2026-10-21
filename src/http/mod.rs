//! HTTP layer: the route table and its handlers.
use actix_web::{HttpResponse, Responder, get, patch, web};

use crate::state::AppState;
use error::ApiError;

pub mod error;
pub mod kv;

/// Build the app configuration for one shared `AppState`.
///
/// The `web::Data` is created here exactly once; the returned closure only
/// clones it, so every actix worker (and every test app) built from clones of
/// this closure sees the same store and settings.
pub fn configure(state: AppState) -> impl Fn(&mut web::ServiceConfig) + Clone + Send + 'static {
    let data = web::Data::new(state);
    move |cfg: &mut web::ServiceConfig| {
        cfg.app_data(data.clone())
            .service(index)
            .service(status)
            .service(patch_key)
            .configure(kv::configure);
    }
}

/// `GET /` answers with the JSON welcome string.
#[get("/")]
pub async fn index() -> impl Responder {
    HttpResponse::Ok().json("Welcome to rs3")
}

/// `GET /ip` reports the configured host as the legacy JSON string `"{'IP' : '<host>'}"`.
///
/// The host comes from the settings in the injected `web::Data<AppState>`,
/// loaded once at startup; the handler never re-reads configuration.
#[get("/ip")]
pub async fn status(state: web::Data<AppState>) -> impl Responder {
    HttpResponse::Ok().json(format!("{{'IP' : '{}'}}", state.settings.server.host))
}

/// `PATCH /key/{key}` runs the stored value as JavaScript on the dedicated JS
/// pool; the actix worker only awaits the reply.
///
/// Success is 200 with the JSON string of `String(completion value)`. A
/// missing key is a 404 JSON error, answered before anything is queued.
#[patch("/key/{key}")]
pub async fn patch_key(
    state: web::Data<AppState>,
    path: web::Path<String>,
) -> Result<HttpResponse, ApiError> {
    let key = path.into_inner();
    let source = state.store.get(&key)?;
    let out = state.js.execute(state.store.clone(), key, source).await?;
    Ok(HttpResponse::Ok().json(out))
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use crate::state::test_state;
    use actix_web::{App, dev::ServiceResponse, http::StatusCode, test as atest};

    /// Assert a JSON error response: status, `application/json`, the `error`
    /// code and a string `message`. Returns the parsed body.
    pub(crate) async fn assert_json_error(
        resp: ServiceResponse,
        expected: StatusCode,
        code: &str,
    ) -> serde_json::Value {
        assert_eq!(resp.status(), expected);
        assert_eq!(
            resp.headers()
                .get("content-type")
                .map(|v| v.as_bytes().to_vec()),
            Some(b"application/json".to_vec())
        );
        let body: serde_json::Value =
            serde_json::from_slice(&atest::read_body(resp).await).expect("JSON error body");
        assert_eq!(body["error"], code, "unexpected error code: {body}");
        assert!(body["message"].is_string(), "no string message: {body}");
        body
    }

    fn ip_request() -> atest::TestRequest {
        atest::TestRequest::get()
            .uri("/ip")
            .insert_header(("content-type", "text/plain"))
    }

    #[actix_web::test]
    async fn test_index_is_ok() {
        let app = atest::init_service(App::new().configure(configure(test_state()))).await;
        let req = atest::TestRequest::default()
            .insert_header(("content-type", "text/plain"))
            .to_request();
        let resp = atest::call_service(&app, req).await;
        assert_eq!(resp.status(), StatusCode::OK);
    }

    #[actix_web::test]
    async fn test_index_body_is_welcome() {
        let app = atest::init_service(App::new().configure(configure(test_state()))).await;
        let req = atest::TestRequest::default()
            .insert_header(("content-type", "text/plain"))
            .to_request();
        let resp = atest::call_service(&app, req).await;
        assert_eq!(resp.status(), StatusCode::OK);
        assert_eq!(
            atest::read_body(resp).await,
            r#""Welcome to rs3""#.as_bytes()
        );
    }

    #[actix_web::test]
    async fn test_status_is_ok() {
        let app = atest::init_service(App::new().configure(configure(test_state()))).await;
        let resp = atest::call_service(&app, ip_request().to_request()).await;
        assert_eq!(resp.status(), StatusCode::OK);
    }

    #[actix_web::test]
    async fn test_ip_body_is_ip() {
        let app = atest::init_service(App::new().configure(configure(test_state()))).await;
        let resp = atest::call_service(&app, ip_request().to_request()).await;
        assert_eq!(resp.status(), StatusCode::OK);
        assert_eq!(
            atest::read_body(resp).await,
            r#""{'IP' : 'test-host'}""#.as_bytes()
        );
    }

    #[actix_web::test]
    async fn test_ip_without_settings_is_500() {
        // No AppState registered: actix answers its own text/plain 500.
        let app = atest::init_service(App::new().service(status)).await;
        let resp = atest::call_service(&app, ip_request().to_request()).await;
        assert_eq!(resp.status(), StatusCode::INTERNAL_SERVER_ERROR);
    }

    #[actix_web::test]
    async fn test_patch_missing_is_404_json() {
        let app = atest::init_service(App::new().configure(configure(test_state()))).await;
        let req = atest::TestRequest::patch().uri("/key/nope").to_request();
        let resp = atest::call_service(&app, req).await;
        assert_json_error(resp, StatusCode::NOT_FOUND, "not_found").await;
    }

    #[actix_web::test]
    async fn test_patch_existing_runs_script() {
        let state = test_state();
        state.store.insert("p".into(), "1".into());
        let app = atest::init_service(App::new().configure(configure(state.clone()))).await;
        let req = atest::TestRequest::patch().uri("/key/p").to_request();
        let resp = atest::call_service(&app, req).await;
        assert_eq!(resp.status(), StatusCode::OK);
        assert_eq!(
            atest::read_body(resp).await,
            web::Bytes::from_static(br#""1""#)
        );
        assert_eq!(state.store.get("p"), Ok("1".to_owned()));
    }

    #[actix_web::test]
    async fn test_patch_runs_script_200() {
        let state = test_state();
        state.store.insert("m".into(), "6*20".into());
        let app = atest::init_service(App::new().configure(configure(state))).await;
        let req = atest::TestRequest::patch().uri("/key/m").to_request();
        let resp = atest::call_service(&app, req).await;
        assert_eq!(resp.status(), StatusCode::OK);
        assert_eq!(
            atest::read_body(resp).await,
            web::Bytes::from_static(br#""120""#)
        );
    }

    #[actix_web::test]
    async fn insert_on_one_app_visible_on_another() {
        // Two apps from clones of one configure closure stand in for two actix workers.
        let state = test_state();
        let cfg = configure(state.clone());
        let a = atest::init_service(App::new().configure(cfg.clone())).await;
        let b = atest::init_service(App::new().configure(cfg)).await;
        let echo = atest::call_and_read_body(
            &a,
            atest::TestRequest::post()
                .uri("/insert")
                .set_payload(r#"{"k":"v"}"#)
                .to_request(),
        )
        .await;
        assert_eq!(echo, web::Bytes::from_static(br#""{\"k\":\"v\"}""#));
        let resp =
            atest::call_service(&b, atest::TestRequest::get().uri("/key/k").to_request()).await;
        assert_eq!(resp.status(), StatusCode::OK);
        assert_eq!(
            atest::read_body(resp).await,
            web::Bytes::from_static(br#""v""#)
        );
        assert_eq!(state.store.get("k"), Ok("v".to_owned()));
    }

    /// Seed `pairs` into a fresh test state, PATCH `/key/{key}` and return the response.
    async fn patch_seeded(pairs: &[(&str, &str)], key: &str) -> ServiceResponse {
        let state = test_state();
        for (k, v) in pairs {
            state.store.insert((*k).to_owned(), (*v).to_owned());
        }
        let app = atest::init_service(App::new().configure(configure(state))).await;
        let req = atest::TestRequest::patch()
            .uri(&format!("/key/{key}"))
            .to_request();
        atest::call_service(&app, req).await
    }

    #[actix_web::test]
    async fn test_patch_throw_is_422_js_error() {
        let resp = patch_seeded(&[("t", r#"throw new Error("boom")"#)], "t").await;
        let body = assert_json_error(resp, StatusCode::UNPROCESSABLE_ENTITY, "js_error").await;
        assert_eq!(
            body,
            serde_json::json!({"error": "js_error", "message": "Uncaught Error: boom"})
        );
    }

    /// The `js_depth_limit` body for the default limit of 4 and `chain`.
    fn depth_body(chain: &str) -> serde_json::Value {
        serde_json::json!({
            "error": "js_depth_limit",
            "message": format!("PATCH nesting depth limit (4) exceeded: {chain}")
        })
    }

    #[actix_web::test]
    async fn depth_limit_finite_chain_is_422() {
        // Finite and acyclic (D-11): without a limit this answers 200 "6".
        let pairs = [
            ("k1", "Deno.core.ops.op_rs3_patch('k2')"),
            ("k2", "Deno.core.ops.op_rs3_patch('k3')"),
            ("k3", "Deno.core.ops.op_rs3_patch('k4')"),
            ("k4", "Deno.core.ops.op_rs3_patch('k5')"),
            ("k5", "Deno.core.ops.op_rs3_patch('k6')"),
            ("k6", "6"),
        ];
        let resp = patch_seeded(&pairs, "k1").await;
        let body =
            assert_json_error(resp, StatusCode::UNPROCESSABLE_ENTITY, "js_depth_limit").await;
        assert_eq!(body, depth_body("k1 -> k2 -> k3 -> k4 -> k5"));
    }

    #[actix_web::test]
    async fn test_patch_stringify_exception_is_422_js_error() {
        let src = "({ toString() { throw new Error('x') } })";
        let resp = patch_seeded(&[("t", src)], "t").await;
        let body = assert_json_error(resp, StatusCode::UNPROCESSABLE_ENTITY, "js_error").await;
        assert_eq!(
            body,
            serde_json::json!({"error": "js_error", "message": "Uncaught Error: x"})
        );
        let resp = patch_seeded(&[("n", "Object.create(null)")], "n").await;
        let body = assert_json_error(resp, StatusCode::UNPROCESSABLE_ENTITY, "js_error").await;
        let message = body["message"].as_str().unwrap_or_default();
        assert!(message.starts_with("Uncaught TypeError"), "{body}");
    }

    #[actix_web::test]
    async fn test_patch_symbol_result_is_200() {
        let resp = patch_seeded(&[("s", "Symbol('x')")], "s").await;
        assert_eq!(resp.status(), StatusCode::OK);
        let body = atest::read_body(resp).await;
        assert_eq!(body, web::Bytes::from_static(br#""Symbol(x)""#));
    }

    #[actix_web::test]
    async fn test_patch_syntax_error_is_422_js_error() {
        let resp = patch_seeded(&[("s", "let = ;")], "s").await;
        let body = assert_json_error(resp, StatusCode::UNPROCESSABLE_ENTITY, "js_error").await;
        let message = body["message"].as_str().unwrap_or_default();
        assert!(message.starts_with("Uncaught SyntaxError"), "{body}");
    }

    #[actix_web::test]
    async fn test_patch_non_error_throw_is_422_js_error() {
        let resp = patch_seeded(&[("n", r#"throw "plain""#)], "n").await;
        let body = assert_json_error(resp, StatusCode::UNPROCESSABLE_ENTITY, "js_error").await;
        assert_eq!(body["message"], "Uncaught plain");
    }

    #[actix_web::test]
    async fn test_patch_uncaught_op_error_is_422_js_error() {
        let resp = patch_seeded(&[("g", r#"Deno.core.ops.op_rs3_get("nope")"#)], "g").await;
        let body = assert_json_error(resp, StatusCode::UNPROCESSABLE_ENTITY, "js_error").await;
        assert_eq!(body["message"], "Uncaught Error: key not found: nope");
    }

    #[actix_web::test]
    async fn test_patch_op_panic_is_422_and_server_keeps_serving() {
        let state = test_state();
        state
            .store
            .insert("p".into(), r#"Deno.core.ops.op_panic("x")"#.into());
        state.store.insert("ok".into(), "1+1".into());
        let app = atest::init_service(App::new().configure(configure(state))).await;
        let resp =
            atest::call_service(&app, atest::TestRequest::patch().uri("/key/p").to_request()).await;
        let body = assert_json_error(resp, StatusCode::UNPROCESSABLE_ENTITY, "js_error").await;
        assert_eq!(body["message"], "Uncaught Error: op is disabled");
        let resp = atest::call_service(&app, atest::TestRequest::get().uri("/").to_request()).await;
        assert_eq!(resp.status(), StatusCode::OK);
        assert_eq!(
            atest::read_body(resp).await,
            web::Bytes::from_static(br#""Welcome to rs3""#)
        );
        let resp = atest::call_service(
            &app,
            atest::TestRequest::patch().uri("/key/ok").to_request(),
        )
        .await;
        assert_eq!(resp.status(), StatusCode::OK);
        assert_eq!(
            atest::read_body(resp).await,
            web::Bytes::from_static(br#""2""#)
        );
    }

    #[actix_web::test]
    async fn test_patch_script_error_is_repeatable() {
        let state = test_state();
        state
            .store
            .insert("t".into(), r#"throw new Error("again")"#.into());
        state.store.insert("ok".into(), "40+2".into());
        let app = atest::init_service(App::new().configure(configure(state))).await;
        let mut bodies = Vec::new();
        for _ in 0..2 {
            let resp =
                atest::call_service(&app, atest::TestRequest::patch().uri("/key/t").to_request())
                    .await;
            bodies
                .push(assert_json_error(resp, StatusCode::UNPROCESSABLE_ENTITY, "js_error").await);
        }
        assert_eq!(bodies[0], bodies[1]);
        let resp = atest::call_service(
            &app,
            atest::TestRequest::patch().uri("/key/ok").to_request(),
        )
        .await;
        assert_eq!(resp.status(), StatusCode::OK);
        assert_eq!(
            atest::read_body(resp).await,
            web::Bytes::from_static(br#""42""#)
        );
    }

    #[actix_web::test]
    async fn test_patch_error_message_keeps_non_ascii() {
        let resp = patch_seeded(&[("u", r#"throw new Error("ключ ✓")"#)], "u").await;
        let body = assert_json_error(resp, StatusCode::UNPROCESSABLE_ENTITY, "js_error").await;
        assert_eq!(body["message"], "Uncaught Error: ключ ✓");
    }

    #[actix_web::test]
    async fn test_patch_executor_down_is_500_internal() {
        let state = AppState {
            js: crate::js::executor::JsExecutor::closed(),
            ..test_state()
        };
        state.store.insert("k".into(), "1".into());
        let app = atest::init_service(App::new().configure(configure(state.clone()))).await;
        let resp =
            atest::call_service(&app, atest::TestRequest::patch().uri("/key/k").to_request()).await;
        let body = assert_json_error(resp, StatusCode::INTERNAL_SERVER_ERROR, "internal").await;
        assert_eq!(
            body,
            serde_json::json!({"error": "internal", "message": "internal error"})
        );
        assert_eq!(state.store.get("k"), Ok("1".to_owned()));
    }

    #[actix_web::test]
    async fn test_internal_error_response_is_500_json() {
        use actix_web::ResponseError;
        let resp = ApiError::Internal.error_response();
        assert_eq!(resp.status(), StatusCode::INTERNAL_SERVER_ERROR);
        assert_eq!(
            resp.headers()
                .get("content-type")
                .map(|v| v.as_bytes().to_vec()),
            Some(b"application/json".to_vec())
        );
        let bytes = actix_web::body::to_bytes(resp.into_body())
            .await
            .expect("error body");
        let body: serde_json::Value = serde_json::from_slice(&bytes).expect("JSON error body");
        assert_eq!(
            body,
            serde_json::json!({"error": "internal", "message": "internal error"})
        );
    }

    const LIST_SCRIPT: &str = "Deno.core.ops.op_rs3_list().toString();";

    #[actix_web::test]
    async fn test_patch_is_ok() {
        let state = test_state();
        state.store.insert("code".into(), LIST_SCRIPT.into());
        let app = atest::init_service(App::new().configure(configure(state.clone()))).await;
        let req = atest::TestRequest::patch().uri("/key/code").to_request();
        let resp = atest::call_service(&app, req).await;
        assert_eq!(resp.status(), StatusCode::OK);
    }

    #[actix_web::test]
    async fn test_patch_body_is_listing() {
        let state = test_state();
        state.store.insert("code_body".into(), LIST_SCRIPT.into());
        let app = atest::init_service(App::new().configure(configure(state.clone()))).await;
        let patched = atest::call_and_read_body(
            &app,
            atest::TestRequest::patch()
                .uri("/key/code_body")
                .to_request(),
        )
        .await;
        let listing =
            atest::call_and_read_body(&app, atest::TestRequest::get().uri("/list").to_request())
                .await;
        assert_eq!(patched, listing);
    }

    const FAC_JS: &str = include_str!("../../requests/fac.js");
    const SUM_JS: &str = include_str!("../../requests/sum.js");
    const BIN_JS: &str = include_str!("../../requests/bin.js");
    const STORE_JS: &str = include_str!("../../requests/store.js");
    const TREE_BODY: &[u8] = br#""{\"value\":5,\"left\":{\"value\":3},\"right\":{\"value\":8}}""#;

    /// Send `req` to `app` and return the status code and the body bytes.
    async fn send<S, R>(app: &S, req: R) -> (StatusCode, web::Bytes)
    where
        S: actix_web::dev::Service<R, Response = ServiceResponse, Error = actix_web::Error>,
    {
        let resp = atest::call_service(app, req).await;
        let code = resp.status();
        (code, atest::read_body(resp).await)
    }

    /// A fresh state seeded with `pairs`.
    fn seeded(pairs: &[(&str, &str)]) -> AppState {
        let state = test_state();
        for (k, v) in pairs {
            state.store.insert((*k).to_owned(), (*v).to_owned());
        }
        state
    }

    fn patch_req(key: &str) -> atest::TestRequest {
        atest::TestRequest::patch().uri(&format!("/key/{key}"))
    }

    fn get_req(uri: &str) -> atest::TestRequest {
        atest::TestRequest::get().uri(uri)
    }

    #[actix_web::test]
    async fn fixture_fac_is_120() {
        let state = seeded(&[("num", "5"), ("fac", FAC_JS)]);
        let app = atest::init_service(App::new().configure(configure(state))).await;
        let (code, body) = send(&app, patch_req("fac").to_request()).await;
        assert_eq!(code, StatusCode::OK);
        assert_eq!(body, web::Bytes::from_static(br#""120""#));
    }

    #[actix_web::test]
    async fn fixture_sum_is_10() {
        let state = seeded(&[("numbers", "1 2 3 4"), ("sum", SUM_JS)]);
        let app = atest::init_service(App::new().configure(configure(state))).await;
        let (code, body) = send(&app, patch_req("sum").to_request()).await;
        assert_eq!(code, StatusCode::OK);
        assert_eq!(body, web::Bytes::from_static(br#""10""#));
    }

    #[actix_web::test]
    async fn fixture_bin_is_tree_json() {
        let state = seeded(&[("tree", "5 3 8"), ("bin", BIN_JS)]);
        let app = atest::init_service(App::new().configure(configure(state))).await;
        let (code, body) = send(&app, patch_req("bin").to_request()).await;
        assert_eq!(code, StatusCode::OK);
        assert_eq!(body, web::Bytes::from_static(TREE_BODY));
    }

    #[actix_web::test]
    async fn fixture_store_writes_res() {
        let state = seeded(&[("store", STORE_JS)]);
        let app = atest::init_service(App::new().configure(configure(state))).await;
        let (code, body) = send(&app, patch_req("store").to_request()).await;
        assert_eq!(code, StatusCode::OK);
        assert_eq!(body, web::Bytes::from_static(br#""{\"res\":\"1111\"}""#));
        let (code, body) = send(&app, get_req("/key/res").to_request()).await;
        assert_eq!(code, StatusCode::OK);
        assert_eq!(body, web::Bytes::from_static(br#""1111""#));
    }

    #[actix_web::test]
    async fn fixtures_share_one_store_in_any_order() {
        // sum.js and bin.js both declare `numbers` and `arr` at top level; every
        // PATCH gets a fresh runtime, so they never collide.
        let state = seeded(&[
            ("numbers", "1 2 3 4"),
            ("tree", "5 3 8"),
            ("sum", SUM_JS),
            ("bin", BIN_JS),
        ]);
        let app = atest::init_service(App::new().configure(configure(state))).await;
        let expected: [(&str, &[u8]); 3] =
            [("sum", br#""10""#), ("bin", TREE_BODY), ("sum", br#""10""#)];
        for (key, want) in expected {
            let (code, body) = send(&app, patch_req(key).to_request()).await;
            assert_eq!(code, StatusCode::OK, "PATCH {key}");
            assert_eq!(body, web::Bytes::from_static(want), "PATCH {key}");
        }
    }

    #[actix_web::test]
    async fn test_patch_listing_matches_list_with_escapes_and_order() {
        let state = seeded(&[
            ("b", "2"),
            ("a", "1"),
            ("B", "3"),
            ("q", r#"say "hi" \ bye"#),
            ("code", LIST_SCRIPT),
        ]);
        let app = atest::init_service(App::new().configure(configure(state))).await;
        let (code, patched) = send(&app, patch_req("code").to_request()).await;
        assert_eq!(code, StatusCode::OK);
        let (code, listing) = send(&app, get_req("/list").to_request()).await;
        assert_eq!(code, StatusCode::OK);
        assert_eq!(patched, listing);
        assert!(
            listing.starts_with(br#""[{\"B\" : \"3\"}"#),
            "listing not in byte order: {listing:?}"
        );
        // The listing includes the script key itself.
        let text = std::str::from_utf8(&listing).expect("UTF-8 listing");
        assert!(text.contains(r#"{\"code\" : "#), "{text}");
    }

    #[actix_web::test]
    async fn test_patch_stringifies_completion_values() {
        let cases: [(&str, &str, &[u8]); 5] = [
            ("s1", "let a = 1;", br#""undefined""#),
            ("s2", "({a:1})", br#""[object Object]""#),
            ("s3", "Promise.resolve(1)", br#""[object Promise]""#),
            ("s4", "''", br#""""#),
            ("s5", "", br#""undefined""#),
        ];
        let state = test_state();
        for (k, v, _) in cases {
            state.store.insert(k.to_owned(), v.to_owned());
        }
        let app = atest::init_service(App::new().configure(configure(state))).await;
        for (k, v, want) in cases {
            let (code, body) = send(&app, patch_req(k).to_request()).await;
            assert_eq!(code, StatusCode::OK, "script {v:?}");
            assert_eq!(body, web::Bytes::from_static(want), "script {v:?}");
        }
    }

    #[actix_web::test]
    async fn test_patch_get_delete_on_one_key_hit_their_own_handlers() {
        let state = seeded(&[("k", "6*20")]);
        let app = atest::init_service(App::new().configure(configure(state))).await;
        let (code, body) = send(&app, get_req("/key/k").to_request()).await;
        assert_eq!(code, StatusCode::OK);
        assert_eq!(body, web::Bytes::from_static(br#""6*20""#));
        let (code, body) = send(&app, patch_req("k").to_request()).await;
        assert_eq!(code, StatusCode::OK);
        assert_eq!(body, web::Bytes::from_static(br#""120""#));
        let (code, body) = send(
            &app,
            atest::TestRequest::delete().uri("/key/k").to_request(),
        )
        .await;
        assert_eq!(code, StatusCode::OK);
        assert_eq!(body, web::Bytes::from_static(br#""6*20""#));
        let resp =
            atest::call_service(&app, atest::TestRequest::patch().uri("/key/k").to_request()).await;
        assert_json_error(resp, StatusCode::NOT_FOUND, "not_found").await;
    }
}
