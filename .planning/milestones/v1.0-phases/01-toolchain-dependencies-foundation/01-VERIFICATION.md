---
phase: 01-toolchain-dependencies-foundation
verified: 2026-10-21T11:53:34Z
status: passed
score: 45/45 must-haves verified (33 verified directly, 12 via override: 11 superseded by later phases, 1 accepted risk)
covered_files:
  - ".planning/phases/01-toolchain-dependencies-foundation/01-01-PLAN.md"
  - ".planning/phases/01-toolchain-dependencies-foundation/01-01-SUMMARY.md"
  - ".planning/phases/01-toolchain-dependencies-foundation/01-02-PLAN.md"
  - ".planning/phases/01-toolchain-dependencies-foundation/01-02-SUMMARY.md"
  - ".planning/phases/01-toolchain-dependencies-foundation/01-03-PLAN.md"
  - ".planning/phases/01-toolchain-dependencies-foundation/01-03-SUMMARY.md"
  - "Cargo.lock"
  - "Cargo.toml"
  - "clippy.toml"
  - "rust-toolchain.toml"
  - "scripts/ci-guards.sh"
  - "src/config.rs"
  - "src/http/kv.rs"
  - "src/http/mod.rs"
  - "src/js/mod.rs"
  - "src/lib.rs"
  - "src/main.rs"
  - "src/state.rs"
  - "src/store/legacy.rs"
  - "src/store/mod.rs"
covered_digest: "v3:sha256:4b7ea73eceaaec08c0f925a5a9a9164e543f33f0e2b6b1bf75cdfa77972402c9"
behavior_unverified: 0
overrides_applied: 12
overrides:
  - must_have: "(backstop) Concurrent cargo invocations all select 1.99.1, the rustup auto-install of the pin is idempotent, and the target lock serialises builds"
    reason: "Accepted risk (operator, 01-UAT test 2 skipped): verifying the auto-install clause requires uninstalling the 1.99.1 toolchain, which is not worth it. Toolchain selection and target-lock serialisation were observed directly; CI (Phase 4) installs the pin explicitly via dtolnay/rust-toolchain@1.99.1."
    accepted_by: "user operator (jmfrank63)"
    accepted_at: "2026-10-20T17:40:00Z"
  - must_have: "The dependency set is exactly the D-04 set ... with no direct v8, rusty_v8, tokio, actix-rt, lazy_static, dotenv, flurry or anyhow (D-04, DEPS-03)"
    reason: "Superseded only for the 'no direct tokio' clause by Phase 3 decision D-09 (03-CONTEXT.md:48: a direct tokio dependency with limited features is acceptable). Cargo.toml now has tokio 1.54.0 [rt, sync, time] (+ macros as dev-dep), the same tokio deno_core and actix-web already resolve. Every other clause still holds: D-04 crates at the planned versions, no direct v8/rusty_v8/actix-rt/lazy_static/dotenv/flurry/anyhow."
    accepted_by: "superseded by Phase 3 recorded decision D-09 (03-CONTEXT.md)"
    accepted_at: "2026-10-21T11:53:34Z"
  - must_have: "main.rs is thin: it loads Settings once, calls rs3::js::init_platform() on the main thread before HttpServer::new, creates web::Data<Settings> outside the factory closure, and maps config and bind errors to ExitCode::FAILURE"
    reason: "Superseded for the 'web::Data<Settings>' clause by Phase 2 D-10/D-11 (02-CONTEXT.md:42-43): main builds one AppState{store, settings, js} outside the HttpServer::new closure and rs3::http::configure(state) creates the single web::Data<AppState>. The intent (state created once, outside the factory) holds; Settings::load once, init_platform before HttpServer::new and ExitCode::FAILURE on config/bind errors all hold in src/main.rs."
    accepted_by: "superseded by Phase 2 recorded decisions D-10/D-11 (02-CONTEXT.md)"
    accepted_at: "2026-10-21T11:53:34Z"
  - must_have: "Handlers reach the store only through the temporary static STORE LazyLock bridge in http::kv, marked TODO(phase-2): inject via web::Data (D-09)"
    reason: "Intentionally temporary; removed by Phase 2 D-11 (02-CONTEXT.md:43, ARCH-02): the static STORE and its TODO were deleted and handlers use state.store from web::Data<AppState>. Also covers the 01-01/01-02 key links 'LazyLock<Store>', 'STORE.(remove|list)(' and 'configure(rs3::http::configure)', now 'state.store.remove/list' and 'rs3::http::configure(state)' + 'App::new().configure(cfg.clone())'."
    accepted_by: "superseded by Phase 2 recorded decision D-11 (02-CONTEXT.md)"
    accepted_at: "2026-10-21T11:53:34Z"
  - must_have: "legacy::format_list is the single list codec that the future op_rs3_list will share; it keeps the current unescaped shape and applies the legacy ,] to ] replacement as its last step, so /list bodies stay byte-identical to the old handler (D-08)"
    reason: "Superseded by Phase 2 D-06/D-09 (02-CONTEXT.md:36,39): format_list now JSON-escapes keys and values and drops the ',]' quirk (it corrupted values containing ',]'); bytes stay identical to legacy for data needing no escaping. The 'single codec shared with op_rs3_list' clause still holds (src/js/ops.rs:200 calls legacy::format_list)."
    accepted_by: "superseded by Phase 2 recorded decisions D-06/D-09 (02-CONTEXT.md)"
    accepted_at: "2026-10-21T11:53:34Z"
  - must_have: "New store unit tests cover remove, remove of a missing key and the sorted list, and legacy codec tests cover the empty list, the item shape and the replacement quirk (D-18)"
    reason: "Superseded for the 'replacement quirk' test by Phase 2 D-09 (02-CONTEXT.md:39), which replaced format_list_keeps_legacy_replace_quirk with escaping and round-trip tests. Store tests remove_returns_value, remove_missing_is_not_found, list_sorted_by_key remain."
    accepted_by: "superseded by Phase 2 recorded decision D-09 (02-CONTEXT.md)"
    accepted_at: "2026-10-21T11:53:34Z"
  - must_have: "The legacy test_delete_is_ok and test_list_is_ok are ported to #[actix_web::test] with actix_web::test as atest, &app and Bytes comparisons, each using its own key in the shared STORE (D-16)"
    reason: "Superseded for the 'shared STORE' clause by Phase 2 D-12 (02-CONTEXT.md:44): every test builds its own AppState via test_state(), which is stronger isolation. The ported tests still exist with #[actix_web::test], atest, &app and Bytes comparisons (src/http/kv.rs:137,158)."
    accepted_by: "superseded by Phase 2 recorded decision D-12 (02-CONTEXT.md)"
    accepted_at: "2026-10-21T11:53:34Z"
  - must_have: "PATCH /key/{key} answers 501 Not Implemented with a short JSON object carrying an error message until Phase 3 (D-11)"
    reason: "Time-boxed by its own wording ('until Phase 3'). Phase 3 (03-03, commit a4c70a3; 03-CONTEXT.md:56 D-12) made PATCH run the stored script on the rs3-js pool; ROADMAP SC-2 explicitly allowed the temporary 501."
    accepted_by: "superseded by Phase 3 recorded decision D-12 (03-CONTEXT.md) and plan 03-03"
    accepted_at: "2026-10-21T11:53:34Z"
  - must_have: "The two legacy PATCH tests are kept and marked #[ignore = \"re-enabled in Phase 3 (JS runtime port)\"]; their scripts use Deno.core.ops.op_rs3_list() and they use no sleeps and no store clearing (D-17)"
    reason: "Superseded for the #[ignore] clause by Phase 3 D-12 (03-CONTEXT.md:56): both tests were un-ignored and pass (src/http/mod.rs:643 test_patch_is_ok, :653 test_patch_body_is_listing). They still use Deno.core.ops.op_rs3_list(), no sleeps and no clearing."
    accepted_by: "superseded by Phase 3 recorded decision D-12 (03-CONTEXT.md)"
    accepted_at: "2026-10-21T11:53:34Z"
  - must_have: "Clippy runs at default lint levels under -D warnings; clippy::unwrap_used is not enabled in Phase 1 (D-19)"
    reason: "Scoped to Phase 1 by its wording. Phase 2 D-13 (02-CONTEXT.md:45) turned on [lints.clippy] unwrap_used = deny and expect_used = deny with clippy.toml allowing them in tests; CI guard (c) enforces it."
    accepted_by: "superseded by Phase 2 recorded decision D-13 (02-CONTEXT.md)"
    accepted_at: "2026-10-21T11:53:34Z"
  - must_have: "Dependencies point one way: store and config import no other rs3 module and no web or JS crate, js imports no web crate and no other rs3 module, and http never imports the JS engine crate directly (ARCH-01)"
    reason: "Superseded for the 'js imports no other rs3 module' clause by Phase 3 D-03 (03-CONTEXT.md:40): ops reach the store through ExecCtx{store}, so js imports crate::store, which is exactly the http -> js -> store direction ARCH-01 requires. All other clauses hold: store/config import no rs3 module and no actix/deno; js imports no actix and no crate::http/state/config; http has no deno import."
    accepted_by: "superseded by Phase 3 recorded decision D-03 (03-CONTEXT.md)"
    accepted_at: "2026-10-21T11:53:34Z"
  - must_have: "Edge TOOL-03/adjacency: the HTTP tests that share the temporary STORE each use a distinct key (insert_test, get_test, idem_test, delete_test, code, code_body), so parallel tests never read or delete each other's entry, and no test clears the store"
    reason: "Superseded by Phase 2 D-12 (02-CONTEXT.md:44): the shared STORE no longer exists; each test builds a fresh AppState via test_state(), so cross-test interference is impossible by construction. No test clears a store."
    accepted_by: "superseded by Phase 2 recorded decision D-12 (02-CONTEXT.md)"
    accepted_at: "2026-10-21T11:53:34Z"
re_verification:
  previous_status: passed
  previous_score: 43/43 (1 via operator UAT, 1 via override)
  reason: "Report went stale: covered files changed in Phases 2-6. Re-judged against the final milestone tree (HEAD a4a8069)."
  gaps_closed: []
  gaps_remaining: []
  regressions: []
human_verification_resolved: "01-UAT.md (complete, 2026-10-20): 3 passed, 1 skipped as accepted risk, 0 issues. Not re-raised."
---

# Phase 1: Toolchain, Dependencies & Foundation Verification Report

**Phase Goal:** As a developer on rs3, I want to build, lint and test the crate on Rust 1.99.1 with edition 2024 and only released dependencies on top of a modular store and config foundation, so that every later port lands on a compiling, current tree.
**Verified:** 2026-10-21T11:53:34Z
**Status:** passed
**Re-verification:** Yes. The 2026-10-20 report went stale after later phases changed the covered files. This run re-judges every must-have against the final milestone tree (HEAD a4a8069).
**Mode:** mvp (user-story goal)

Evidence basis. The orchestrator ran the full gate on HEAD a4a8069 and shared the log; this verifier did not re-run cargo build/test/clippy or the smoke. The log at `target/tmp/d17-a4a8069.log` shows:
- ci-guards a-h ok
- check-workflow ok
- test-ci-guards 99 cases, 0 missed
- fmt ok
- clippy `--all-targets --locked -D warnings` ok
- `cargo test --locked`: 216 passed, 0 failed, 0 ignored in lib; the 1 ignored is the deno_core `extension!` doctest
- release build ok
- ci-smoke OK

Remote CI run 39527188207 at 40afeb7 (fresh checkout, ubuntu-26.04 x86_64) was green. Since 40afeb7, `git diff` shows only `license = "MIT"` in Cargo.toml and a comment change in src/js/executor.rs.

This verifier ran only read-only checks: source reads, `git log`/`git diff`/`git show`, `cargo tree --locked --offline`, and gsd `verify.artifacts`/`verify.key-links`.

## User Flow Coverage

| Step | Expected | Evidence | Status |
|------|----------|----------|--------|
| Clone fresh, run any cargo command | Rust 1.99.1 + rustfmt + clippy selected | rust-toolchain.toml `channel = "1.99.1"`, components rustfmt/clippy, profile minimal; legacy `rust-toolchain` absent; CI installs `dtolnay/rust-toolchain@1.99.1` | ✓ |
| Build with committed lock | edition 2024 compiles, lock not rewritten | Gate: `cargo build --release --locked` ok; clippy/test also `--locked`; CI on fresh checkout green | ✓ |
| Test | Suite passes, no SERVER.* env | 216 passed, 0 failed, 0 ignored; CI runner has no SERVER.* env; no test calls `Settings::load()` (only call site src/main.rs:8) | ✓ |
| Lint | clippy -D warnings, fmt clean | Gate: fmt ok, clippy ok | ✓ |
| Run the server | Binary starts and serves | ci-smoke on target/release/rs3: GET / → 200 `"Welcome to rs3"`, insert + PATCH round trips, server stays alive | ✓ |
| Outcome: later ports land on a compiling, current tree | Released deps only, single V8 via deno_core, lib + modules | Phases 2–6 were all built on this foundation and the final tree is green | ✓ |

## Goal Achievement

### Observable Truths: Roadmap Success Criteria (the contract)

| # | Truth | Status | Evidence |
|---|-------|--------|----------|
| SC-1 | Any cargo command selects Rust 1.99.1 with rustfmt + clippy from rust-toolchain.toml; legacy `rust-toolchain` gone; Cargo.toml `edition = "2024"`, `rust-version = "1.99"` | ✓ VERIFIED | rust-toolchain.toml (3 keys as above); `ls rust-toolchain` → no such file; Cargo.toml:5-6 |
| SC-2 | Clean checkout: build, test, clippy `--all-targets -D warnings`, fmt `--check` pass; binary starts and serves | ✓ VERIFIED | Shared gate log (all GATE lines OK, 216 tests) on HEAD; CI run 39527188207 on a fresh checkout with the same commands `--locked` (.github/workflows/ci.yml:40-50); ci-smoke serves GET /, POST /insert, PATCH. The 501 allowance is no longer needed (PATCH works since Phase 3) |
| SC-3 | Only released crates.io versions; lazy_static, dotenv, flurry gone; `cargo tree -d` shows a single V8 only via deno_core | ✓ VERIFIED | Cargo.toml has no git/path/branch keys and no pre-release versions. Cargo.lock has `version = 4`, 244 packages, 0 `git+`, and no lazy_static/dotenv/flurry/rusty_v8. `cargo tree -i v8`: v8 150.4.0 ← deno_v8 0.4.0 ← deno_core 0.412.0 / serde_v8. `cargo tree -d` has no duplicated v8 root, and CI guard (e) "single v8" is ok. Prerelease scan of `cargo tree -e all --target all` finds 0. `-i v8x` prints nothing to print, so the inert lock entry is disclosed per D-21 |
| SC-4 | lib.rs + thin main.rs with store, config, http, js modules; papaya store and config loader (builder API + dotenvy, reading SERVER.HOST/PORT from .env or env) have passing unit tests; store imports nothing from actix-web or deno_core | ✓ VERIFIED | Modules: src/lib.rs declares config, http, js, state, store; main.rs is 46 lines. Store: `Arc<papaya::HashMap<String,String>>` with 11 unit tests in src/store/mod.rs; it imports only std (`grep actix\|deno\|crate::` in src/store is empty). Config: built with `Config::builder().add_source(Environment…).build()?.try_deserialize()`; `Settings::load()` = `from_sources(dotenvy::dotenv_iter(), std::env::vars_os())`; 19 tests in src/config.rs. Dotenv path, two halves: the content half is unit-tested through the same `from_sources` code (`dotenv_source_configures_settings`, plus malformed/unreadable/missing/override tests). The live file-lookup half is shown by operator UAT 01-UAT test 1 (old `dotenv()` code) and 02-UAT test 1 (the current `dotenv_iter()` found and parsed a cwd dotenv file in the live binary). Env path: ci-smoke starts the binary with `env -i SERVER.HOST=… SERVER.PORT=…` |
| SC-5 | Invalid config or an already-bound port prints an error and exits non-zero without panicking | ✓ VERIFIED | src/main.rs:8-13 (`error: invalid configuration: {e}` → `ExitCode::FAILURE`), :29-36 (`error: cannot bind {host}:{port}: {e}` → FAILURE), and JS-pool start failure → FAILURE. Phase 6 verifier replayed the 10 documented config error lines byte-for-byte on the final source (06-VERIFICATION.md:67), including `SERVER.PORT=70000 -> exit 1`. Phase 2 verifier checked a live second instance on a held port: rc=1, `error: cannot bind … Address already in use` (02-VERIFICATION.md:76). The bind branch is textually unchanged since then; Phase 3 only inserted the JS-pool start before it (a4c70a3). Config error cases are also unit-tested (missing_port/missing_server/invalid_port/underscore) |

### Observable Truths: Plan 01-01 (deduplicated: truths 1 and 17 restate SC-1 and SC-2)

| # | Truth | Status | Evidence |
|---|-------|--------|----------|
| 1-2 | One package, version 0.2.0, edition 2024, rust-version 1.99, lib + bin, no workspace | ✓ VERIFIED | Cargo.toml:1-7; no `[workspace]`; src/lib.rs + src/main.rs auto-discovered |
| 1-3 | Cargo.lock regenerated as v4 without `cargo fix --edition`; no git sources | ✓ VERIFIED | `version = 4`, 0 `git+`; d2b42b7 (01-01) touched only toolchain, manifest and lock |
| 1-4 | Dependency set is exactly D-04; no direct v8/rusty_v8/tokio/actix-rt/lazy_static/dotenv/flurry/anyhow | PASSED (override) | Superseded for tokio only by Phase 3 D-09. `cargo tree --depth 1`: actix-web 4.17.0, config 0.15.27, deno_core 0.412.0, deno_error 0.7.1, dotenvy 0.15.7, papaya 0.2.5, serde 1.0.229, serde_json 1.0.154, thiserror 2.0.21, tokio 1.54.0. Features as planned (actix-web macros-only, config no defaults, serde derive, serde_json preserve_order). anyhow is only transitive (← deno_core) |
| 1-5 | Exactly one v8, only via deno_v8 under deno_core; js is the only deno_core importer | ✓ VERIFIED | `cargo tree -i v8` (SC-3); `grep deno` in src/http, src/store, src/config.rs, src/state.rs is empty. deno_core is used only under src/js/ |
| 1-6 | DEPS-01 holds on manifest and active graph; v8x rc lock entry disclosed in SUMMARY (D-21) | ✓ VERIFIED | v8x 149.4.0-rc.4 at Cargo.lock:2026-2027, not in the active graph; 01-01-SUMMARY.md:63 and §DEPS-01 Disclosure (197-204). Disclosure adequacy accepted by operator (01-UAT test 3) |
| 1-7 | With SERVER.HOST/PORT in env, binary starts and GET / → 200 `"Welcome to rs3"` | ✓ VERIFIED | ci-smoke asserts exact status and bytes; test_index_body_is_welcome |
| 1-8 | POST /insert then GET /key/{key} round trip through papaya Store, legacy echo and value bytes | ✓ VERIFIED | test_insert_is_ok asserts `"{\"insert_test\":\"John Doe\"}"`; test_get_is_ok asserts `"John Doe"`; test_insert_echo_preserves_body_order |
| 1-9 | No SERVER.* config → one `error: invalid configuration` line, exit 1 | ✓ VERIFIED | main.rs:8-13; Readme.md:87 documents the line; Phase 6 replay (06-VERIFICATION.md:67) |
| 1-10 | main.rs thin: Settings once, init_platform before HttpServer::new, state outside factory, ExitCode on errors | PASSED (override) | Superseded for the `web::Data<Settings>` clause by Phase 2 D-10/D-11. Order in main.rs: load (:8) → init_platform (:16) → JsExecutor::start (:17) → AppState::new (:27) → configure(state) (:28) → HttpServer::new (:29) |
| 1-11 | lib + thin main with store/config/http/js; store imports nothing from actix/deno/rs3 and has no global | ✓ VERIFIED | src/store/mod.rs imports only `std::sync::Arc`; legacy.rs imports serde/serde_json/super only; no `static`/`LazyLock` in src/store |
| 1-12 | Handlers reach the store via temporary `static STORE: LazyLock<Store>` | PASSED (override) | Intentionally removed by Phase 2 D-11 (ARCH-02). No `LazyLock<Store>` anywhere in src |
| 1-13 | unsafe forbidden via `#![forbid(unsafe_code)]` and `[lints.rust] unsafe_code = "forbid"` | ✓ VERIFIED | src/lib.rs:1; Cargo.toml `[lints.rust]` |
| 1-14 | Ported tests use `#[actix_web::test]`, `test as atest`, `&app`, Bytes | ✓ VERIFIED | 77 `#[actix_web::test]`; `test as atest` in http/mod.rs:61 and kv.rs:64; no `&mut app` or `actix_rt` in src |
| 1-15 | D-20 commit order | ✓ VERIFIED | git: d2b42b7 (toolchain + manifest + lock) → f530c39 (restructure) → 4c07d33/45d1bcf (store with tests) |
| 1-16 | actions-rs.yml untouched by this plan; SUMMARY notes Phase 4 replaces it | ✓ VERIFIED | No phase-01 commit touches .github. 01-01-SUMMARY.md:208. Phase 4 then replaced it as planned (4d00cd8) |
| 1-18 | deno_error resolves to exactly 0.7.1, deno_core to 0.412.x | ✓ VERIFIED | Cargo.lock:437-438, 478-479 |
| 1-19 | No git/path/branch in Cargo.toml; zero git+ in Cargo.lock | ✓ VERIFIED | grep counts 0 |
| 1-20 | `cargo build --locked` succeeds without rewriting the lock | ✓ VERIFIED | Gate and CI use `--locked` everywhere; `git status` on HEAD shows no Cargo.lock change |
| 1-21 | `Settings::load()` called exactly once in main, before init_platform and HttpServer::new; no handler reads env | ✓ VERIFIED | Only call site main.rs:8; the only `std::env` use in src is inside `Settings::load` (config.rs:87); `/ip` reads `state.settings` |
| 1-22 | Insert same key twice → one entry with second value, insert returns previous; same /insert body twice idempotent | ✓ VERIFIED | insert_returns_previous_value (store), test_insert_same_body_twice_is_idempotent (kv.rs:172); suite green |
| 1-23 | Store clones share one map; concurrent inserts via clones all visible | ✓ VERIFIED | clones_share_state, concurrent_inserts_are_all_visible (4×100 → len 400); suite green |
| 1-24 | (backstop) Concurrent cargo invocations select 1.99.1; auto-install idempotent; target lock serialises | PASSED (override) | Carried accepted risk (01-UAT test 2 skipped). Selection and target-lock serialisation were observed directly in the 2026-10-20 verification |

### Observable Truths: Plan 01-02 (deduplicated: truth 9 restates SC-2)

| # | Truth | Status | Evidence |
|---|-------|--------|----------|
| 2-1 | DELETE /key/{key} on an existing key returns old value as legacy JSON string; key gone afterwards | ✓ VERIFIED | test_delete_is_ok (kv.rs:137) asserts bytes `"John Doe"` and NotFound afterwards; delete_twice_is_200_then_404 |
| 2-2 | GET /list returns every pair in the legacy list shape, sorted by key | ✓ VERIFIED | Store::list sorts by key; list_golden_sorted, test_list_is_ok. Shape `[{"k" : "v"},…]` kept (escaping added per Phase 2 D-06, byte-identical for data that needs none) |
| 2-3 | Store::remove → owned String or NotFound; Store::list → owned sorted snapshot; no guard escapes | ✓ VERIFIED | src/store/mod.rs remove/list signatures; each takes its own `pin()` inside the call |
| 2-4 | format_list single shared codec, unescaped, `,]` quirk | PASSED (override) | Superseded by Phase 2 D-06/D-09. Still the single codec: kv.rs list and js/ops.rs:200 `op_rs3_list` both call `legacy::format_list` |
| 2-5 | Store tests remove/missing/sorted; codec tests empty/shape/quirk | PASSED (override) | Quirk test replaced per Phase 2 D-09. remove_returns_value, remove_missing_is_not_found, list_empty, list_sorted_by_key present |
| 2-6 | test_delete_is_ok and test_list_is_ok ported (atest, &app, Bytes), own key in shared STORE | PASSED (override) | Shared STORE replaced by per-test AppState (Phase 2 D-12); ported tests present at kv.rs:137,158 |
| 2-7 | store imports nothing from actix/deno/rs3 and holds no global | ✓ VERIFIED | Same evidence as 1-11 |
| 2-8 | 01-02 changed no dependency | ✓ VERIFIED | 01-02 commits 6199842, 7444787, f3dd1e1 touch only src/ and tdd-evidence |

### Observable Truths: Plan 01-03 (deduplicated: truth 4 restates SC-5, truth 9 restates SC-2)

| # | Truth | Status | Evidence |
|---|-------|--------|----------|
| 3-1 | GET /ip reports the startup host from the settings registered once in main, legacy body `{'IP' : '<host>'}` as a JSON string | ✓ VERIFIED | http/mod.rs:36-39 reads `state.settings.server.host` from `web::Data<AppState>` (carrier changed by Phase 2 D-10; behaviour identical); test_ip_body_is_ip asserts `"{'IP' : 'test-host'}"` |
| 3-2 | Tests build Settings with `Settings::from_map`; never touch the process env | ✓ VERIFIED | state.rs `test_state()` uses `Settings::from_map`; config tests use in-memory maps, `dotenvy::from_read_iter` and synthetic env vectors; `load()` is never called from tests |
| 3-3 | Config tests: dotted keys map; missing port, missing server, `abc`, `70000`, `SERVER_HOST` are errors | ✓ VERIFIED | config.rs:137-180 (dotted_keys_parse, missing_port_is_error, missing_server_is_error, invalid_port_is_error [abc + 70000], underscore_names_are_not_mapped) |
| 3-5 | PATCH /key/{key} → 501 until Phase 3 | PASSED (override) | Time-boxed; replaced in Phase 3 (a4c70a3) |
| 3-6 | Two PATCH tests kept, `#[ignore]`d, using `Deno.core.ops.op_rs3_list()`, no sleeps or clearing | PASSED (override) | Un-ignored by Phase 3 D-12; both present and passing with `op_rs3_list` (mod.rs:640-668) |
| 3-7 | Clippy at default levels; unwrap_used not enabled in Phase 1 | PASSED (override) | Phase 2 D-13 raised it to deny (Cargo.toml `[lints.clippy]`, clippy.toml test allowances) |
| 3-8 | One-way module dependencies | PASSED (override) | js → store per Phase 3 D-03 (matches ARCH-01's http → js → store). No reverse edges: `grep crate::http\|crate::state\|crate::config` in src/js and src/store is empty; config imports no rs3 module; http has no deno import |
| 3-10 | Shared-STORE tests use distinct keys; no test clears the store | PASSED (override) | Per-test AppState (Phase 2 D-12) makes this structural |
| 3-11 | `cargo test` passes with no SERVER.* env | ✓ VERIFIED | No test reads the environment (above); CI runner (no SERVER.*) green at 40afeb7 |
| 3-12 | Suite passes in parallel and serially (`--test-threads=1`); no order dependence | ✓ VERIFIED | Parallel: shared gate, 216/0. Serial runs on record: Phase 3 verifier ran the full suite with `--test-threads=1` under `env -i` (03-VERIFICATION.md:82); 05-03-SUMMARY.md:210 shows 212 passed serially; the Phase 5 verifier re-ran the TEST-04 set serially after the last behavioural src fix (05-VERIFICATION.md:200, verified 01:20Z after dfd1120 01:09Z). Since then, only a comment in src/js/executor.rs changed (e3e3485). Order independence is also structural, because each test calls `test_state()` and gets a fresh store |

**Score:** 45/45 truths verified: 33 directly, plus 12 PASSED (override). Of the overrides, 11 were superseded by recorded Phase 2/3 decisions and 1 is an operator-accepted risk. 0 present-but-behavior-unverified.

### Prohibitions

| Prohibition | Tier | Disposition | Evidence |
|-------------|------|-------------|----------|
| MUST NOT fall back to a default/wildcard bind address or default port when SERVER.HOST/PORT is missing or invalid | test | ✓ verified (wired) | config::tests missing_port_is_error, missing_server_is_error, invalid_port_is_error, underscore_names_are_not_mapped. `ServerSettings` has no `#[serde(default)]`. The module doc says "there are no defaults" |
| MUST NOT report Cargo.lock as free of pre-release entries (v8x disclosure, D-21) | judgment | ✓ resolved by operator | 01-UAT test 3 pass (operator confirmed the disclosure is adequate). Not re-raised |
| MUST NOT silently change the success status or body bytes of GET /, POST /insert, GET /key/{key} in Phase 1 | test | ✓ verified (wired) | Golden byte tests still pass on the final tree: test_index_body_is_welcome, test_insert_is_ok, test_insert_map_entry_is_ok, test_get_is_ok; ci-smoke asserts GET / bytes |
| MUST NOT make the TOOL-03 gate green by suppression | test | ✓ verified (now wired) | scripts/ci-guards.sh guard (a) fails on any `#[allow`/`#![allow`/`#[expect`/`cfg_attr(..allow..)`. Guard (b) fails on any `#[ignore]` or ignored doc block. Guard (c) requires unwrap_used/expect_used at deny with no `allow`. Guard (h) forbids rustflags/cap-lints config. All ok on HEAD and in CI. scripts/test-ci-guards.sh proves the guards catch violations (99 cases, 0 missed). 0 `#[ignore]` remain in src. 01-UAT test 4 accepted the original grep evidence |

### Required Artifacts

| Artifact | Expected | Status | Details |
|----------|----------|--------|---------|
| `rust-toolchain.toml` | Pin 1.99.1 + rustfmt + clippy, minimal | ✓ VERIFIED | gsd verify.artifacts 01-01: 10/11 (only kv.rs `static STORE` pattern missing, superseded) |
| `Cargo.toml` | Edition 2024, D-04 deps, unsafe forbid | ✓ VERIFIED | Plus Phase 3 tokio and Phase 2 clippy lints (superseded clauses) |
| `Cargo.lock` | v4, single v8 | ✓ VERIFIED | |
| `src/lib.rs` | Crate root, forbid(unsafe_code) | ✓ VERIFIED | Also declares `state` (Phase 2) |
| `src/main.rs` | Thin binary, ExitCode | ✓ VERIFIED | 46 lines |
| `src/config.rs` | Settings, ServerSettings, from_map + tests | ✓ VERIFIED | gsd 01-03: 2/2 |
| `src/js/mod.rs` | `JsRuntime::init_platform(None)` seam | ✓ VERIFIED | Now hosts the full JS layer (Phase 3/5) |
| `src/http/mod.rs` | configure, index, status, patch_key | ✓ VERIFIED | |
| `src/http/kv.rs` | insert/get/delete/list | ✓ VERIFIED (override for `static STORE` pattern) | Store via `web::Data<AppState>` (Phase 2 D-11); gsd 01-02: 3/3 |
| `src/store/mod.rs` | Store, StoreError over papaya; `pub fn list(&self) -> Vec<(String, String)>` | ✓ VERIFIED | |
| `src/store/legacy.rs` | parse_insert_body, format_insert_echo, format_list | ✓ VERIFIED | legacy.rs:36,134,150 |

### Key Link Verification

| From | To | Via | Status | Details |
|------|----|-----|--------|---------|
| main.rs | config.rs | `Settings::load()` | WIRED | main.rs:8 |
| main.rs | js/mod.rs | `rs3::js::init_platform()` before HttpServer::new | WIRED | main.rs:16 |
| main.rs | http/mod.rs | `configure(rs3::http::configure)` / `app_data(settings.clone())` | WIRED (superseded form) | gsd reports the literal patterns missing. Current wiring per Phase 2 D-10: `let cfg = rs3::http::configure(state);` + `App::new().configure(cfg.clone())` (main.rs:28-29), and configure registers `web::Data<AppState>` once |
| http/mod.rs | http/kv.rs | `configure(kv::configure)` | WIRED | mod.rs:22 (`.configure(kv::configure)`) |
| http/kv.rs | store/mod.rs | `LazyLock<Store>`, `STORE.(remove\|list)(` | WIRED (superseded form) | `state.store.insert_all/get/remove/list` (kv.rs:32,42,51,57), per Phase 2 D-11 |
| http/kv.rs | store/legacy.rs | `legacy::parse_insert_body`, `legacy::format_list` | WIRED | kv.rs:30,57 |
| http/mod.rs | config.rs | `web::Data<Settings>` | WIRED (superseded form) | `web::Data<AppState>` → `state.settings.server.host` (mod.rs:37-38; host read at :38); AppState.settings is `Arc<Settings>` (state.rs) |
| http/kv.rs | route table | `service(delete_key\|list)` | WIRED | kv.rs:10-13 |
| http/mod.rs | route table | `service(status\|patch_key)` | WIRED | mod.rs:19-22 |
| store/mod.rs | papaya | `Arc<papaya::HashMap<String, String>>` | WIRED | store/mod.rs:24 |

### Data-Flow Trace (Level 4)

| Artifact | Data Variable | Source | Produces Real Data | Status |
|----------|---------------|--------|--------------------|--------|
| GET /ip | `state.settings.server.host` | `Settings::load()` (dotenv file + process env) → AppState → web::Data | Yes (test_ip_body_is_ip; Phase 2/6 live runs) | ✓ FLOWING |
| GET /key, DELETE, GET /list | `state.store` contents | POST /insert → `legacy::parse_insert_body` → `Store::insert_all` (papaya) | Yes (golden tests; ci-smoke insert → PATCH 120) | ✓ FLOWING |

### Behavioral Spot-Checks

The orchestrator directed that cargo build/test/clippy, the smoke and the binary not be re-run, because parallel verifiers would contend. Results come from the shared gate log for HEAD a4a8069 plus read-only commands run here.

| Behavior | Command | Result | Status |
|----------|---------|--------|--------|
| Four TOOL-03 gates | shared log `target/tmp/d17-a4a8069.log` | fmt/clippy/test/release all `GATE … OK`; 216 passed, 0 failed | ✓ PASS |
| Binary serves | shared log: `bash scripts/ci-smoke.sh` | `ci-smoke: OK (target/release/rs3, port 39599)` | ✓ PASS |
| Single V8 via deno_core | `cargo tree --locked --offline -i v8` | v8 150.4.0 ← deno_v8 0.4.0 ← deno_core 0.412.0 | ✓ PASS |
| No duplicated v8 | `cargo tree --locked --offline -d \| grep ^v8` | empty | ✓ PASS |
| Active graph free of pre-releases | `cargo tree -e all --target all --prefix none \| grep ' vX.Y.Z-'` | 0 | ✓ PASS |
| v8x not built | `cargo tree -e all --target all -i v8x` | nothing to print | ✓ PASS |
| Direct deps | `cargo tree --depth 1 -e normal,dev` | D-04 set + tokio (Phase 3 D-09) | ✓ PASS |
| Plan artifacts and links | gsd `verify.artifacts` / `verify.key-links` ×3 | 15/16 artifacts, 8/13 links. Every miss is a literal Phase-1 pattern (`LazyLock<Store>`, `STORE.`, `web::Data<Settings>`, `app_data(settings.clone())`, `configure(rs3::http::configure)`) superseded by Phase 2 D-10/D-11 | ✓ PASS (superseded) |

### Probe Execution

No `scripts/*/tests/probe-*.sh` exist, and no Phase 1 PLAN or SUMMARY declares one. Step 7c is not applicable. The repo's CI checks (`scripts/ci-guards.sh`, `scripts/test-ci-guards.sh`, `scripts/ci-smoke.sh`) were run by the orchestrator on HEAD, and all passed (shared log).

### Requirements Coverage

| Requirement | Source Plan | Description | Status | Evidence |
|-------------|-------------|-------------|--------|----------|
| TOOL-01 | 01-01 | Rust 1.99.1 + rustfmt + clippy via rust-toolchain.toml; legacy file deleted | ✓ SATISFIED | SC-1 |
| TOOL-02 | 01-01 | edition 2024, rust-version 1.99 | ✓ SATISFIED | SC-1, 1-2 |
| TOOL-03 | 01-01, 01-02, 01-03 | build/test/clippy/fmt pass on a clean checkout | ✓ SATISFIED | SC-2; shared gate; CI fresh checkout |
| DEPS-01 | 01-01 | Released crates.io versions only | ✓ SATISFIED (D-21 note) | Manifest and active graph clean; inert v8x lock entry disclosed and operator-accepted |
| DEPS-02 | 01-01 | Lock regenerated; single V8; no direct v8/rusty_v8 | ✓ SATISFIED | SC-3, 1-3, 1-5; guard (e) |
| DEPS-03 | 01-01 | lazy_static, dotenv, flurry removed | ✓ SATISFIED | Absent from Cargo.toml and Cargo.lock; injected state, dotenvy and papaya are used instead |
| DEPS-04 | 01-01, 01-03 | config builder API; reads SERVER.HOST/PORT from .env or env | ✓ SATISFIED | SC-4 (builder API; env path live in ci-smoke; dotenv path unit-tested + live lookup via 01-UAT 1 / 02-UAT 1) |
| ARCH-01 | 01-01, 01-02, 01-03 | store/js/http/config modules, lib + thin main, one-way deps (http → js → store) | ✓ SATISFIED | SC-4, 1-11, 3-8 |
| ARCH-04 | 01-01, 01-03 | Startup errors reported, exit non-zero | ✓ SATISFIED | SC-5 |

Orphaned requirements: none. REQUIREMENTS.md:138-148 maps exactly these 9 IDs to Phase 1, all marked Complete, and each appears in at least one plan's `requirements` field.

### Anti-Patterns Found

| File | Line | Pattern | Severity | Impact |
|------|------|---------|----------|--------|
| src/store/legacy.rs | 18, 39, 71 | `XXX` inside `\uXXXX` | ℹ️ Info | False positive: JSON escape notation in doc comments, not a debt marker |
| src/js/runtime.rs, src/js/executor.rs | 71, 760 | `std::thread::sleep` | ℹ️ Info | Both are test-only, under `#[cfg(test)]` (a bounded injected-failure wait and a test). Outside Phase 1 scope |

There are no TBD/FIXME/TODO/HACK markers in any covered file; Phase 1's `TODO(phase-2)` was removed as planned. There are no `#[allow]`/`cfg_attr` suppressions and no `#[ignore]`. Production `unwrap()` is denied by lint, and the `panic!` hits are all in tests.

### Human Verification Required

None pending for Phase 1. The four items from the 2026-10-20 verification were closed by the operator in 01-UAT.md: tests 1, 3 and 4 passed, and test 2 was skipped as an accepted risk, recorded as an override.

Related item owned by another phase, not re-raised here: 02-UAT.md test 2 ("Valid dotenv file with stray keys configures /ip", live binary) is still `[pending]` in Phase 2. It does not block SC-4. The valid-content path is unit-tested through the same `from_sources` code, and 02-UAT test 1 (pass) already showed that the current `dotenv_iter()` finds and reads a cwd dotenv file in the live binary.

### Gaps Summary

There are no gaps. On the final milestone tree, every ROADMAP Phase 1 success criterion holds:
- The crate builds, lints, formats and tests green on Rust 1.99.1 / edition 2024 with `--locked`, locally and in fresh-checkout CI.
- Only released crates are built, with one V8 reached only through deno_core.
- The code is a lib plus a thin main, with store/config/http/js modules whose dependencies point one way.
- The papaya store and the builder-based config loader have passing unit tests, and the loader reads from a dotenv file and the environment.
- Bad config or a busy port exits 1 with an `error:` line.

Eleven plan-level must-haves describe Phase 1 transitional details that later phases replaced on purpose, each by a recorded decision:
- the LazyLock STORE bridge
- `web::Data<Settings>`
- PATCH 501
- the parked `#[ignore]` tests
- the unescaped `/list` quirk
- the shared-store key discipline
- no `unwrap_used` lint
- no direct tokio
- js importing no rs3 module

Each is recorded as PASSED (override), with a citation to 02-CONTEXT.md or 03-CONTEXT.md, not as a gap. In every case the later design is stricter than or equal to the Phase 1 intent.

---

_Verified: 2026-10-21T11:53:34Z_
_Verifier: Claude (gsd-verifier)_
