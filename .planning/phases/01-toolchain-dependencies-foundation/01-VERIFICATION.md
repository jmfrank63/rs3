---
phase: 01-toolchain-dependencies-foundation
verified: 2026-10-20T17:09:26Z
status: passed
score: 43/43 must-haves verified (1 via operator UAT, 1 via override)
covered_files:
  - ".planning/phases/01-toolchain-dependencies-foundation/01-01-PLAN.md"
  - ".planning/phases/01-toolchain-dependencies-foundation/01-01-SUMMARY.md"
  - ".planning/phases/01-toolchain-dependencies-foundation/01-02-PLAN.md"
  - ".planning/phases/01-toolchain-dependencies-foundation/01-02-SUMMARY.md"
  - ".planning/phases/01-toolchain-dependencies-foundation/01-03-PLAN.md"
  - ".planning/phases/01-toolchain-dependencies-foundation/01-03-SUMMARY.md"
  - "Cargo.lock"
  - "Cargo.toml"
  - "rust-toolchain.toml"
  - "src/config.rs"
  - "src/http/kv.rs"
  - "src/http/mod.rs"
  - "src/js/mod.rs"
  - "src/lib.rs"
  - "src/main.rs"
  - "src/store/legacy.rs"
  - "src/store/mod.rs"
covered_digest: "v3:sha256:a782c455a0f3f3ed4c623419ca9a6ed7b0b82f89ea52d22f2cfc4b9292abc660"
behavior_unverified: 0
overrides_applied: 1
overrides:
  - must_have: "(backstop) Concurrent cargo invocations all select 1.99.1, the rustup auto-install of the pin is idempotent, and the target lock serialises builds"
    reason: "Accepted risk (operator, 01-UAT test 2 skipped): verifying the auto-install clause requires uninstalling the 1.99.1 toolchain, which is not worth it. Toolchain selection and target-lock serialisation were observed directly; CI (Phase 4) installs the pin explicitly via dtolnay/rust-toolchain@1.99.1."
    accepted_by: "user operator (jmfrank63)"
    accepted_at: "2026-10-20T17:40:00Z"
human_verification_resolved: "2026-10-20T17:40:00Z (01-UAT.md: 3 passed, 1 skipped as accepted risk, 0 issues)"
behavior_unverified_items:
  - truth: "SC-4 (DEPS-04 clause): the config loader still reads SERVER.HOST / SERVER.PORT from a dotenv file in the working directory (not only from the process environment)"
    test: "In an empty scratch directory, create a dotenv file containing SERVER.HOST=127.0.0.1 and SERVER.PORT=18461 (dotted names, as in env.example). Run /home/t3code/rs3/Projects/rs3/target/debug/rs3 from that directory with `env -i PATH=$PATH`, then `curl -s http://127.0.0.1:18461/ip`. Stop the server afterwards."
    expected: "The server starts, prints the startup banner with 127.0.0.1 and 18461, and /ip answers 200 with \"{'IP' : '127.0.0.1'}\"."
    resolved: "PASS — operator UAT 2026-10-20 (01-UAT.md test 1)"
    why_human: "No test and no recorded smoke exercises the dotenv-file path. `dotenvy::dotenv().ok()` is present and wired in Settings::load (src/config.rs:25), and dotenvy 0.15.7's parser accepts '.' in key names (parse.rs:85), but a project hook blocks the verifier from creating or naming the dotenv file, so the runtime path could not be exercised here."
human_verification:
  - test: "Dotenv-file config source (DEPS-04 / SC-4): run the binary from a directory that holds a dotenv file with SERVER.HOST=127.0.0.1 and SERVER.PORT=18461, under `env -i PATH=$PATH`, then curl /ip"
    expected: "200 with body \"{'IP' : '127.0.0.1'}\"; the server starts without SERVER.* in the process environment"
    why_human: "The dotenv path is wired but unexercised by any test; the verifier's hook forbids creating or naming the dotenv file"
  - test: "Backstop truth TOOL-01/concurrency, clause 'rustup auto-install of the pin is idempotent': on a machine without 1.99.1 installed, start two cargo commands in the repo at the same time (e.g. `cargo build & cargo clippy --all-targets & wait`)"
    expected: "Both auto-install or reuse 1.99.1 without a rustup error, both report 1.99.1, and one of them prints 'Blocking waiting for file lock'"
    why_human: "Non-inferable (verification: backstop). Toolchain selection and target-lock serialisation were observed directly, but the auto-install clause needs the toolchain uninstalled, which is a destructive state change the verifier must not make"
  - test: "Judgment-tier prohibition (01-01): 'MUST NOT report Cargo.lock as free of pre-release entries; the SUMMARY must disclose the never-built optional v8x 149.4.0-rc.4 lock entry and state that DEPS-01 is verified on the manifest and the active build graph (D-21)'"
    expected: "Read 01-01-SUMMARY.md lines 63 and 197-204 and confirm the disclosure is adequate"
    why_human: "unverified-prohibition, human review recommended. Judgment-tier prohibitions get a non-authoritative LLM verdict only. Verifier verdict: satisfied (disclosure present; v8x is in Cargo.lock line 2025 and absent from `cargo tree -e all --target all -i v8x`)"
  - test: "Test-tier prohibition (01-03): 'MUST NOT make the TOOL-03 gate green by suppression: no crate-level or module-level allow attribute, no #[ignore] beyond the two PATCH tests, and no deleted or weakened assertions from the ported legacy tests'"
    expected: "Confirm the verifier's one-off evidence is enough, or add a wired check (e.g. a CI grep step in Phase 4) to enforce it going forward"
    why_human: "unverified-prohibition, human review recommended. Test-tier, but no wired test enforces it (fail-closed default). Verifier's grep evidence is clean: zero `allow`/`cfg_attr` in src/ and Cargo.toml, no [lints.clippy] section, exactly two #[ignore] (src/http/mod.rs:125,135, both PATCH), and every legacy assertion (status OK, delete removes the key, insert echo bytes, welcome bytes) is kept or strengthened compared with 77660881:src/main.rs"
---

# Phase 1: Toolchain, Dependencies & Foundation Verification Report

**Phase Goal:** As a developer on rs3, I want to build, lint and test the crate on Rust 1.99.1 with edition 2024 and only released dependencies on top of a modular store and config foundation, so that every later port lands on a compiling, current tree.
**Verified:** 2026-10-20T17:09:26Z
**Status:** passed (after operator UAT, 2026-10-20; originally human_needed)
**Re-verification:** No, initial verification
**Mode:** mvp (the user-story goal is valid according to `user-story.validate`)

## User Flow Coverage

User story: «As a developer on rs3, I want to build, lint and test the crate on Rust 1.99.1 with edition 2024 and only released dependencies on top of a modular store and config foundation, so that every later port lands on a compiling, current tree.»

| Step | Expected | Evidence | Status |
|------|----------|----------|--------|
| Clone the repo fresh | Only committed files; no dotenv, no target/ | `git clone` of HEAD `ef15f44` into a scratch dir under /home/t3code | ✓ |
| Run any cargo command | Rust 1.99.1 with rustfmt and clippy is selected | `rustup show active-toolchain` → `1.99.1-aarch64-unknown-linux-gnu (overridden by rust-toolchain.toml)`; `cargo 1.99.1`, `rustc 1.99.1`; clippy and rustfmt installed | ✓ |
| Build | Compiles on edition 2024 with the committed lock | `cargo build --locked` exit 0 | ✓ |
| Test | All tests pass with no SERVER.* env | In the clean clone: `env -i PATH HOME cargo test --locked` exit 0, 36 passed, 2 ignored (the parked PATCH tests); `--test-threads=1` also 36/2 | ✓ |
| Lint | clippy -D warnings and fmt clean | clean clone: `cargo clippy --all-targets --locked -- -D warnings` exit 0; `cargo fmt --check` exit 0 | ✓ |
| Run the server | Binary starts and serves; PATCH answers 501 | Smoke on port 18437: GET / → 200 `"Welcome to rs3"`; /ip, /insert, /key, DELETE, /list all 200 with legacy bodies; PATCH → 501 JSON | ✓ |
| Outcome: later ports land on a compiling, current tree | Released deps only, single V8 via deno_core, modular lib with store/config/http/js | See truths SC-3, SC-4a below | ✓ |

## Goal Achievement

### Observable Truths

**Roadmap success criteria (the contract)**

| # | Truth | Status | Evidence |
|---|-------|--------|----------|
| SC-1 | Any cargo command selects Rust 1.99.1 with rustfmt + clippy from rust-toolchain.toml; legacy rust-toolchain is gone; Cargo.toml has edition 2024, rust-version 1.99 | ✓ VERIFIED | rust-toolchain.toml: `channel = "1.99.1"`, components rustfmt and clippy, profile minimal. `rust-toolchain` does not exist (deleted in d2b42b7; it used to pin 1.56.0). Cargo.toml has `edition = "2024"` and `rust-version = "1.99"`. rustup reports the override from rust-toolchain.toml |
| SC-2 | On a clean checkout, build, test, clippy `--all-targets -D warnings` and fmt `--check` all pass; the binary starts and serves (PATCH may answer 501) | ✓ VERIFIED | Fresh clone of HEAD, all with `--locked`: test exit 0 (36 pass, 2 ignored), clippy exit 0, fmt exit 0, and `git status` stays clean, so Cargo.lock was not rewritten. In the working repo, build, test and clippy also exit 0. Live smoke: GET / → 200 `"Welcome to rs3"`, PATCH → 501 `{"error":...}` |
| SC-3 | Only released crates.io versions in Cargo.toml; lazy_static, dotenv and flurry gone; `cargo tree -d` on the regenerated lock shows a single V8 reached only through deno_core | ✓ VERIFIED | Cargo.toml has no git, path or branch keys and no pre-release versions. Cargo.lock: 0 `git+` sources, no lazy_static, flurry, dotenv or rusty_v8, `version = 4`. `cargo tree -i v8` shows v8 150.4.0 only via deno_v8 0.4.0 ← deno_core 0.412.0 / serde_v8. `cargo tree -d` lists no duplicated v8 root. Active graph `cargo tree -e all --target all`: 0 pre-release packages. Note: inert `v8x 149.4.0-rc.4` lock entry (optional dep of deno_v8, never built), accepted by D-21 and disclosed in the SUMMARY |
| SC-4a | lib.rs + thin main.rs with separate store, config, http and js modules; papaya store and config loader have passing unit tests; store imports nothing from actix-web or deno_core | ✓ VERIFIED | src/lib.rs declares `config`, `http`, `js`, `store`. main.rs is 42 lines. store/: no `actix`, `deno`, `crate::`, `static` or `LazyLock` (grep is empty). Store wraps `Arc<papaya::HashMap<String,String>>`. 11 store tests, 8 legacy-codec tests and 5 config tests pass. The config tests go through the same `from_env_source` builder path that `load()` uses |
| SC-4b | The config loader still reads SERVER.HOST / SERVER.PORT from **a dotenv file** or the environment | ⚠️ PRESENT_BEHAVIOR_UNVERIFIED (dotenv clause) | Environment path verified live: `env -i 'SERVER.HOST=127.0.0.1' 'SERVER.PORT=18437'` serves, and /ip reports 127.0.0.1. The dotenv path is wired (`dotenvy::dotenv().ok()` in Settings::load), and dotenvy 0.15.7 accepts dotted keys, but no test or smoke exercises it. Routed to human verification |
| SC-5 | Invalid config or an already-bound port prints an error and exits non-zero without panicking | ✓ VERIFIED | Live: no config → `error: invalid configuration: missing configuration field "server"`, exit 1. Port `abc` → exit 1. Port `70000` → exit 1. Missing host → exit 1. Only `SERVER_HOST`/`SERVER_PORT` set → exit 1. Second instance on the bound port → `error: cannot bind 127.0.0.1:18437: Address already in use`, exit 1. No `panicked` in any output |

**Plan 01-01 must-haves (deduplicated against the roadmap SCs)**

| # | Truth | Status | Evidence |
|---|-------|--------|----------|
| 1-3 | Cargo.lock deleted and regenerated as v4 without `cargo fix --edition`; no git sources | ✓ VERIFIED | The legacy lock had no version line (v1/v2 format, 178 packages, git deno_core). The new lock is `version = 4` with 244 packages, 0 `git+` sources and 0 `github.com/denoland`. d2b42b7 touches only Cargo.toml, Cargo.lock and the toolchain files (no edition-fix churn in sources) |
| 1-4 | Dependency set is exactly D-04; no direct v8, rusty_v8, tokio, actix-rt, lazy_static, dotenv, flurry or anyhow | ✓ VERIFIED | `cargo tree --depth 1`: actix-web 4.17.0, config 0.15.27, deno_core 0.412.0, deno_error 0.7.1, dotenvy 0.15.7, papaya 0.2.5, serde 1.0.229, serde_json 1.0.154, thiserror 2.0.21. Manifest features: actix-web `default-features=false, ["macros"]`, config `default-features=false`, serde `derive`, serde_json `preserve_order` |
| 1-5 | Exactly one v8 crate, only via deno_v8 under deno_core; js stub is the only deno_core importer | ✓ VERIFIED | `cargo tree -i v8` (see SC-3); `grep -rl deno_core src` → only src/js/mod.rs |
| 1-6 | DEPS-01 holds on the manifest and the active graph; v8x rc entry recorded in SUMMARY (D-21) | ✓ VERIFIED | `cargo tree -e all --target all -i v8x` → "nothing to print"; 01-01-SUMMARY.md:63 and §"DEPS-01 Disclosure" (lines 197-204) |
| 1-7 | With SERVER.HOST/SERVER.PORT in env, binary starts and GET / → 200 `"Welcome to rs3"` | ✓ VERIFIED | Live smoke, port 18437 |
| 1-8 | POST /insert then GET /key/{key} round trip through the papaya Store with legacy bytes | ✓ VERIFIED | Live: POST `{"b":"2","a":"John Doe"}` → `"{\"b\":\"2\",\"a\":\"John Doe\"}"`, GET /key/a → `"John Doe"`. Tests test_insert_is_ok and test_get_is_ok assert exact bytes. Legacy serde_json 1.0.67 also pulled indexmap (preserve_order), so key order matches |
| 1-9 | No SERVER.* → one `error: invalid configuration` line, exit 1 | ✓ VERIFIED | Live smoke (SC-5) |
| 1-10 | main.rs is thin: Settings loaded once, `js::init_platform()` before `HttpServer::new`, `web::Data<Settings>` outside the factory, ExitCode on config and bind errors | ✓ VERIFIED | src/main.rs:8-41 runs these steps in that order. `web::Data::new(settings)` at line 26 is outside the closure. All error branches `return ExitCode::FAILURE` |
| 1-12 | Handlers reach the store only through `static STORE: LazyLock<Store>` marked `TODO(phase-2): inject via web::Data` | ✓ VERIFIED | src/http/kv.rs:8-9 |
| 1-13 | unsafe forbidden through `#![forbid(unsafe_code)]` and `[lints.rust] unsafe_code = "forbid"` | ✓ VERIFIED | src/lib.rs:1; Cargo.toml lines 23-24 |
| 1-14 | Ported tests use `#[actix_web::test]`, `test as atest`, `&app` and Bytes comparisons | ✓ VERIFIED | grep finds no `&mut app` and no `actix_rt`. Both test modules import `test as atest` |
| 1-15 | D-20 commit order | ✓ VERIFIED | d2b42b7 (toolchain + manifest + lock) → f530c39 (restructure) → 4c07d33/45d1bcf (store with tests) |
| 1-16 | actions-rs.yml untouched; SUMMARY notes Phase 4 replaces it | ✓ VERIFIED | `git diff --quiet 77660881 HEAD -- .github` exits 0; 01-01-SUMMARY.md:208 |
| 1-18 | deno_error resolves to exactly 0.7.1; deno_core to 0.412.x | ✓ VERIFIED | Cargo.lock lines 437-438 and 478-479 |
| 1-19 | No git or path sources in Cargo.toml or Cargo.lock | ✓ VERIFIED | grep count 0 |
| 1-20 | `cargo build --locked` succeeds without rewriting the lock | ✓ VERIFIED | Exit 0 in the repo and in the clean clone; clone `git status` clean |
| 1-21 | `Settings::load()` is called exactly once, in main, before init_platform and HttpServer::new; no handler reads the env | ✓ VERIFIED | Only call site is src/main.rs:8. No `std::env` or `env::var` anywhere in src/. `/ip` reads `web::Data<Settings>` |
| 1-22 | Inserting the same key twice keeps one entry with the second value, and `insert` returns the previous value; the same /insert body twice gives the same state and bytes | ✓ VERIFIED | Tests insert_returns_previous_value and test_insert_same_body_twice_is_idempotent pass |
| 1-23 | Store clones share one map; concurrent inserts through clones are all visible | ✓ VERIFIED | Tests clones_share_state and concurrent_inserts_are_all_visible (4 threads × 100 inserts, len 400) pass |
| 1-24 | (backstop) Concurrent cargo invocations all select 1.99.1, the rustup auto-install of the pin is idempotent, and the target lock serialises builds | ? insufficient_spec (human) | Observed directly: concurrent clippy, build and `rustc --version` all ran 1.99.1, both builds exited 0, and the logs show "Blocking waiting for file lock on build directory". The auto-install clause was not observable (toolchain already installed) |

**Plan 01-02 must-haves**

| # | Truth | Status | Evidence |
|---|-------|--------|----------|
| 2-1 | DELETE /key/{key} returns the old value as a legacy JSON string; the key is then gone | ✓ VERIFIED | test_delete_is_ok asserts the bytes `"John Doe"` and NotFound afterwards. Live: DELETE /key/b → `"2"`, and /list no longer shows b |
| 2-2 | GET /list returns the legacy shape, sorted by key | ✓ VERIFIED | Live: `"[{\"a\" : \"John Doe\"},{\"b\" : \"2\"}]"` (inserted b before a, listed sorted) |
| 2-3 | Store::remove returns owned String or NotFound; Store::list returns an owned sorted snapshot; no guard escapes | ✓ VERIFIED | src/store/mod.rs:62-81: signatures return `Result<String, StoreError>` / `Vec<(String,String)>`; each takes its own `pin()` inside the call |
| 2-4 | `legacy::format_list` is the single list codec, with `,]`→`]` applied last | ✓ VERIFIED | src/store/legacy.rs:39-46, used by kv::list. The result is byte-equivalent to the legacy `bindings::list` + `replace(",]", "]")` for the same order |
| 2-5 | Store tests cover remove, missing remove and sorted list; codec tests cover empty, shape and quirk | ✓ VERIFIED | remove_returns_value, remove_missing_is_not_found, list_empty, list_sorted_by_key, format_list_empty_is_brackets, format_list_shape and format_list_keeps_legacy_replace_quirk all pass |
| 2-6 | Legacy test_delete_is_ok and test_list_is_ok ported to atest/&app/Bytes, each with its own key | ✓ VERIFIED | src/http/kv.rs tests (keys delete_test, list_test) |
| 2-8 | 01-02 changed no dependency | ✓ VERIFIED | Last commit touching Cargo.toml/Cargo.lock is d2b42b7 (01-01). The 01-02 commits 6199842, 7444787 and f3dd1e1 touch only src/ and tdd-evidence |

**Plan 01-03 must-haves**

| # | Truth | Status | Evidence |
|---|-------|--------|----------|
| 3-1 | GET /ip reports the startup host from `web::Data<Settings>` in the legacy `{'IP' : '<host>'}` JSON-string format | ✓ VERIFIED | src/http/mod.rs:27-29; test_ip_body_is_ip asserts `"{'IP' : 'test-host'}"`; live → `"{'IP' : '127.0.0.1'}"` |
| 3-2 | Tests build Settings with `Settings::from_map` and never touch the process env | ✓ VERIFIED | The clean-clone suite passed under `env -i PATH HOME` with no dotenv file |
| 3-3 | Config tests: dotted keys map; missing port, missing server, `abc`, `70000` and underscore names are errors | ✓ VERIFIED | The five config tests in src/config.rs pass |
| 3-5 | PATCH /key/{key} → 501 with a JSON object carrying an error message | ✓ VERIFIED | test_patch_is_501 plus live smoke |
| 3-6 | Two legacy PATCH tests kept with `#[ignore = "re-enabled in Phase 3 (JS runtime port)"]`, using `Deno.core.ops.op_rs3_list()`, with no sleeps and no clearing | ✓ VERIFIED | src/http/mod.rs:122-150; grep finds no `thread::sleep` and no `clear(` in src |
| 3-7 | Clippy at default levels; `clippy::unwrap_used` not enabled | ✓ VERIFIED | No `[lints.clippy]`, no clippy.toml, no clippy attributes in src |
| 3-8 | Dependency direction: store and config import no rs3 module and no web or JS crate; js imports no web crate and no rs3 module; http never imports deno_core | ✓ VERIFIED | Greps: store/ and config.rs have no `actix`, `deno` or `crate::`; js/mod.rs has no `actix` or `crate::`; http/ has no `deno` |
| 3-10 | HTTP tests on the shared STORE each use a distinct key; no test clears the store | ✓ VERIFIED | Keys: insert_test (two tests insert the same identical value, which is idempotent), get_test, idem_test, delete_test, list_test, code, code_body |
| 3-11 | `cargo test` passes with no SERVER.* env | ✓ VERIFIED | `env -i PATH HOME cargo test --locked` in the clean clone, exit 0 |
| 3-12 | Passes both in parallel and with `--test-threads=1` | ✓ VERIFIED | Both runs: 36 passed, 2 ignored |

**Score:** 41/43 truths verified (1 present but behavior-unverified: SC-4b; 1 insufficient_spec backstop: 1-24)

### Prohibitions

| Prohibition | Tier | Disposition | Evidence |
|-------------|------|-------------|----------|
| MUST NOT silently fall back to a default/wildcard bind address or port | test | ✓ verified | Wired: config::tests::missing_port_is_error, missing_server_is_error, invalid_port_is_error and underscore_names_are_not_mapped. Live: every missing or invalid case exits 1 |
| MUST NOT report Cargo.lock as free of pre-release entries (v8x disclosure) | judgment | flagged: unverified-prohibition, human review recommended | Non-authoritative LLM verdict: satisfied (01-01-SUMMARY.md:63, 197-204) |
| MUST NOT change the success status or body bytes of GET /, POST /insert or GET /key/{key} | test | ✓ verified | Wired byte tests test_index_body_is_welcome, test_insert_is_ok, test_insert_map_entry_is_ok and test_get_is_ok. Bodies match the legacy `.json(...)` handlers in 77660881:src/services/mod.rs |
| MUST NOT make TOOL-03 green by suppression | test | flagged: unverified-prohibition (no wired enforcement), human review recommended | One-off grep evidence is clean (see human_verification item 4) |

### Required Artifacts

| Artifact | Expected | Status | Details |
|----------|----------|--------|---------|
| `rust-toolchain.toml` | Pin 1.99.1 + rustfmt + clippy | ✓ VERIFIED | gsd verify.artifacts 11/11 (01-01), 3/3 (01-02), 2/2 (01-03) |
| `Cargo.toml` | Edition 2024, D-04 deps, unsafe forbid | ✓ VERIFIED | |
| `Cargo.lock` | v4, single v8 | ✓ VERIFIED | |
| `src/lib.rs` | Crate root, forbid(unsafe_code) | ✓ VERIFIED | |
| `src/main.rs` | Thin binary, ExitCode | ✓ VERIFIED | |
| `src/config.rs` | Settings, ServerSettings, from_map + tests | ✓ VERIFIED | Wired: main → load(); http::status → Data<Settings> |
| `src/js/mod.rs` | init_platform seam | ✓ VERIFIED | Called from main.rs:18 |
| `src/http/mod.rs` | configure, index, status, patch_key | ✓ VERIFIED | Registered via `configure(rs3::http::configure)` |
| `src/http/kv.rs` | insert, get_key, delete_key, list on STORE | ✓ VERIFIED | Registered via `configure(kv::configure)` |
| `src/store/mod.rs` | Store, StoreError over papaya | ✓ VERIFIED | Used by kv.rs |
| `src/store/legacy.rs` | parse_insert_body, format_insert_echo, format_list | ✓ VERIFIED | Used by kv.rs |

### Key Link Verification

| From | To | Via | Status | Details |
|------|----|-----|--------|---------|
| main.rs | config.rs | `Settings::load()` | WIRED | gsd verify.key-links: 7/7 (01-01), 3/3 (01-02), 3/3 (01-03) |
| main.rs | js/mod.rs | `js::init_platform()` before HttpServer::new | WIRED | |
| main.rs | http/mod.rs | `app_data(settings.clone()).configure(rs3::http::configure)` | WIRED | |
| http/mod.rs | http/kv.rs | `configure(kv::configure)` | WIRED | |
| http/kv.rs | store/mod.rs | `LazyLock<Store>`, `STORE.remove` / `STORE.list` | WIRED | |
| http/kv.rs | store/legacy.rs | `legacy::parse_insert_body`, `legacy::format_list` | WIRED | |
| http/mod.rs | config.rs | `web::Data<Settings>` | WIRED | |
| store/mod.rs | papaya | `Arc<papaya::HashMap<String, String>>` | WIRED | |

### Data-Flow Trace (Level 4)

| Artifact | Data Variable | Source | Produces Real Data | Status |
|----------|---------------|--------|--------------------|--------|
| GET /ip | `settings.server.host` | Settings::load → config builder over the process env | Yes (live → 127.0.0.1) | ✓ FLOWING |
| GET /key, GET /list, DELETE | STORE contents | POST /insert → Store::insert_many (papaya) | Yes (live round trip) | ✓ FLOWING |

### Behavioral Spot-Checks

| Behavior | Command | Result | Status |
|----------|---------|--------|--------|
| Toolchain selection | `rustup show active-toolchain; cargo --version` | 1.99.1 (overridden by rust-toolchain.toml) | ✓ PASS |
| Four gates, clean clone, no env | `env -i PATH HOME cargo test --locked`; clippy `--all-targets -- -D warnings`; `cargo fmt --check` | exit 0 / 0 / 0; 36 passed, 2 ignored | ✓ PASS |
| Serial ordering | `cargo test --lib -- --test-threads=1` | 36 passed, 2 ignored | ✓ PASS |
| Server serves all routes | live binary on 127.0.0.1:18437 + curl | /, /ip, /insert, /key, DELETE, /list → 200 legacy bodies; PATCH → 501 | ✓ PASS |
| Startup errors exit 1 | no config / abc / 70000 / missing host / underscore-only / bound port | exit 1 with `error:` line, no panic | ✓ PASS |
| Single V8 | `cargo tree -i v8`, `cargo tree -d` | v8 150.4.0 via deno_v8 only, no dup root | ✓ PASS |
| Dotenv-file config | n/a | blocked by the project secret-file hook | ? SKIP → human |

### Probe Execution

No `scripts/*/tests/probe-*.sh` exist, and no PLAN or SUMMARY declares a probe. Step 7c is not applicable.

### Requirements Coverage

| Requirement | Source Plan | Description | Status | Evidence |
|-------------|-------------|-------------|--------|----------|
| TOOL-01 | 01-01 | Rust 1.99.1 + rustfmt + clippy via rust-toolchain.toml; legacy file deleted | ✓ SATISFIED | SC-1 |
| TOOL-02 | 01-01 | edition 2024, rust-version 1.99 | ✓ SATISFIED | SC-1 |
| TOOL-03 | 01-01, 01-02, 01-03 | build/test/clippy/fmt pass on a clean checkout | ✓ SATISFIED | SC-2, clean-clone run |
| DEPS-01 | 01-01 | Released crates.io versions only | ✓ SATISFIED (with D-21 note) | Manifest and active graph clean. The inert v8x rc lock entry is accepted per D-21 |
| DEPS-02 | 01-01 | Lock regenerated; single V8; no direct v8/rusty_v8 | ✓ SATISFIED | SC-3, 1-3, 1-5 |
| DEPS-03 | 01-01 | lazy_static, dotenv, flurry removed | ✓ SATISFIED | Absent from Cargo.toml and Cargo.lock; papaya, LazyLock and dotenvy in place |
| DEPS-04 | 01-01, 01-03 | config builder API; reads SERVER.HOST/PORT from a dotenv file or the env | ? NEEDS HUMAN (dotenv clause) | Env path verified live; the dotenv path is wired but unexercised (SC-4b) |
| ARCH-01 | 01-01, 01-02, 01-03 | store/js/http/config modules, lib + thin main, one-way deps | ✓ SATISFIED | SC-4a, 3-8 |
| ARCH-04 | 01-01, 01-03 | Startup errors reported, exit non-zero | ✓ SATISFIED | SC-5 |

Orphaned requirements: none. REQUIREMENTS.md maps exactly these 9 IDs to Phase 1, and every one appears in at least one plan's `requirements` field.

### Anti-Patterns Found

| File | Line | Pattern | Severity | Impact |
|------|------|---------|----------|--------|
| src/http/kv.rs | 8 | `TODO(phase-2): inject via web::Data` | ℹ️ Info | Required by D-09; ARCH-02 (Phase 2) removes the global. Not a TBD/FIXME/XXX debt marker |
| src/http/kv.rs | 24, 26, 40, 49 | `unwrap()` on request-dependent data (insert body, missing key) | ℹ️ Info | Deliberately keeps the legacy panic behaviour (D-10). ROBU-01/02 (Phase 2) and ROBU-04 (Phase 3) own the fix. A panic only affects that request's worker, as in the legacy code |
| src/config.rs | 25 | `dotenvy::dotenv().ok()` discards parse errors; dotenvy calls `set_var` under a safe API | ⚠️ Warning (advisory) | Same as review findings WR-01/WR-02, still `open` in 01-REVIEW-DISPOSITION.md. Safe in practice because main calls it before any thread spawns. Does not block the phase goal |

No TBD, FIXME or XXX markers in any file this phase modified. No `allow` or `cfg_attr` suppression.

### Human Verification Required

### 1. Dotenv-file config source (DEPS-04)

**Test:** In an empty scratch directory, create a dotenv file with `SERVER.HOST=127.0.0.1` and `SERVER.PORT=18461`. Run `env -i PATH=$PATH /home/t3code/rs3/Projects/rs3/target/debug/rs3` from that directory, then `curl -s http://127.0.0.1:18461/ip`, then stop the server.
**Expected:** 200 with `"{'IP' : '127.0.0.1'}"`.
**Why human:** The code is wired, but no test exercises it, and the verifier's secret-file hook blocks creating the dotenv file.

### 2. rustup auto-install idempotency (backstop truth 1-24)

**Test:** On a machine without 1.99.1 installed, run two cargo commands in the repo concurrently.
**Expected:** Both end up on 1.99.1 with no rustup error.
**Why human:** This needs the toolchain uninstalled, which is a destructive state change. The other two clauses were observed directly.

### 3. Judgment-tier prohibition: v8x disclosure

**Test:** Read 01-01-SUMMARY.md:63 and :197-204.
**Expected:** It clearly discloses the inert `v8x 149.4.0-rc.4` lock entry and limits the DEPS-01 claim to the manifest and the active graph.
**Why human:** Judgment-tier prohibitions get only a non-authoritative LLM verdict. The verifier's verdict is "satisfied".

### 4. Test-tier prohibition without wired enforcement: no suppression

**Test:** Accept the verifier's grep evidence, or schedule a wired check (e.g. a Phase 4 CI step that fails on `#[allow`/`#![allow` in src/ and counts `#[ignore`).
**Expected:** Decision recorded.
**Why human:** Fail-closed default for test-tier prohibitions that have no wired enforcement.

### Gaps Summary

No blocking gaps. The phase goal is met in the codebase:
- A fresh clone builds, tests, lints and formats cleanly on Rust 1.99.1 / edition 2024, using `--locked` and an empty environment.
- Only released crates are built, with a single V8 reached through deno_core.
- The crate is a lib plus a thin main with isolated store, config, http and js modules.
- The binary serves every non-JS route with legacy bodies, answers PATCH with 501, and exits 1 with an `error:` line on bad config or a busy port.

The status is `human_needed` rather than `passed` because four items need a person:
- The dotenv-file clause of DEPS-04 is wired but unexercised.
- One backstop clause can't be observed without uninstalling the toolchain.
- Two prohibitions are flagged by policy (one judgment-tier, one test-tier with no wired enforcement), although the verifier's evidence for both is clean.

Review warnings WR-01/WR-02 (dotenvy error handling and `set_var` soundness) are still open in 01-REVIEW-DISPOSITION.md. They are advisory and worth triaging before Phase 2 touches config injection (ARCH-03).

### Human Verification Results (operator, 2026-10-20)

Recorded in `01-UAT.md` (status: complete):

| # | Item | Result | Evidence / disposition |
|---|------|--------|------------------------|
| 1 | Dotenv-file config source (DEPS-04 / SC-4b) | ✓ PASS | Operator ran the binary under `env -i` from a directory with a `.env` holding `SERVER.HOST=127.0.0.1` / `SERVER.PORT=18461`; `GET /ip` → 200 `"{'IP' : '127.0.0.1'}"`. SC-4b and DEPS-04 are now VERIFIED. |
| 2 | rustup auto-install idempotency (backstop 1-24) | ⏭ SKIPPED — accepted risk | Not worth uninstalling the toolchain. Recorded as `PASSED (override)` in frontmatter `overrides`. |
| 3 | v8x pre-release lock-entry disclosure | ✓ PASS | Operator confirmed the disclosure is adequate. |
| 4 | No gate-greening by suppression | ✓ PASS | Operator accepted the grep evidence and requested a wired CI grep check for new `#[allow]` / `#[cfg_attr(.., allow(..))]` in Phase 4. |

Follow-ups routed from this verification:
- Phase 2: address review warnings WR-01 (malformed `.env` half-loaded, error swallowed) and WR-02 (dotenvy `set_var` under a safe API).
- Phase 4: CI step that fails on new allow/cfg_attr-allow suppression attributes.

---

_Verified: 2026-10-20T17:09:26Z_
_Verifier: Claude (gsd-verifier)_
