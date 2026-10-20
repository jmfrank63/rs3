# Roadmap: rs3

## Overview

rs3 moves from Rust 1.56 / edition 2018 with git and beta dependencies to Rust 1.99.1 /
edition 2024 with only released crates, while keeping every HTTP route and the
JS-executes-against-the-store behaviour. Nothing compiles on a modern toolchain today, so this
is a layer-by-layer rewrite on a fresh `Cargo.toml`, not an incremental upgrade. Phase 1
produces a compiling, linted tree on the final dependency set, with the store and config layers
in place. Phase 2 brings back the six non-JS routes on injected state, with typed errors and
isolated tests. Phase 3 brings back PATCH and the five JS ops on current deno_core. Phase 4 adds
an x86_64 CI gate right after the V8 port, so platform crashes that don't occur on aarch64 show
up early. Phase 5 adds recursion and timeout limits so no script can kill or pin the server.
Phase 6 ships the Docker image and README. TOOL-03 (`cargo build`, `cargo test`,
`cargo clippy --all-targets -- -D warnings`, `cargo fmt --check` all green) is established in
Phase 1 and must still pass at the end of every later phase.

## Phases

**Phase Numbering:**
- Integer phases (1, 2, 3): Planned milestone work
- Decimal phases (2.1, 2.2): Urgent insertions (marked with INSERTED)

Decimal phases appear between their surrounding integers in numeric order.

- [x] **Phase 1: Toolchain, Dependencies & Foundation** - Rust 1.99.1 / edition 2024, released deps only, lib + thin main with store and config layers; tree compiles (PATCH may answer 501) (completed 2026-10-20)
- [ ] **Phase 2: HTTP API Port & Error Handling** - Six non-JS routes on actix-web 4.x with injected state, byte-identical bodies, 404/400/422 errors, isolated tests
- [ ] **Phase 3: JavaScript Runtime Port** - PATCH and the five `op_rs3_*` ops on current deno_core (`#[op2]` + `extension!`), dedicated JS threads, all JS artefacts migrated
- [ ] **Phase 4: CI Gate on x86_64** - GitHub Actions workflow gating fmt/clippy/test on an x86_64 runner; archived actions-rs workflow removed
- [ ] **Phase 5: JS Execution Limits** - PATCH nesting depth limit (422) and execution timeout (504) so no script can crash or pin the server
- [ ] **Phase 6: Docker Image & Documentation** - cargo-chef Dockerfile with slim non-root runtime and `.dockerignore`; README for toolchain, op syntax and behaviour changes

## Phase Details

### Phase 1: Toolchain, Dependencies & Foundation

**Goal:** As a developer on rs3, I want to build, lint and test the crate on Rust 1.99.1 with edition 2024 and only released dependencies on top of a modular store and config foundation, so that every later port lands on a compiling, current tree.
**Mode:** mvp
**Depends on**: Nothing (first phase)
**Requirements**: TOOL-01, TOOL-02, TOOL-03, DEPS-01, DEPS-02, DEPS-03, DEPS-04, ARCH-01, ARCH-04
**Success Criteria** (what must be TRUE):
  1. Any `cargo` command in the repo selects Rust 1.99.1 with `rustfmt` and `clippy` from `rust-toolchain.toml` (the legacy `rust-toolchain` file is gone), and `Cargo.toml` declares `edition = "2024"` and `rust-version = "1.99"`
  2. On a clean checkout `cargo build`, `cargo test`, `cargo clippy --all-targets -- -D warnings` and `cargo fmt --check` all pass, and the binary starts and serves requests (PATCH may temporarily answer 501 until Phase 3)
  3. `Cargo.toml` lists only released crates.io versions (no git, path, beta or rc), `lazy_static`, `dotenv` and `flurry` are gone, and `cargo tree -d` on the freshly regenerated `Cargo.lock` shows a single V8 reached only through `deno_core`
  4. The crate is a `lib.rs` plus a thin `main.rs` with separate `store`, `config`, `http` and `js` modules; the papaya-backed store and the config loader (current `config` builder API + `dotenvy`, still reading `SERVER.HOST` / `SERVER.PORT` from `.env` or the environment) have passing unit tests, and `store` imports nothing from actix-web or deno_core
  5. Starting the binary with invalid configuration or an already-bound port prints an error and exits non-zero instead of panicking

**Plans:** 3/3 plans complete

Plans:
**Wave 1**
- [x] 01-01-PLAN.md: Walking skeleton (wave 1). Toolchain, manifest and lockfile reset; lib + thin main; serve GET / and an insert-to-get round trip through the papaya Store; exit 1 on bad config

**Wave 2** *(blocked on Wave 1 completion)*
- [x] 01-02-PLAN.md: Key-value slice (wave 2). DELETE /key/{key} and GET /list via Store::remove, sorted Store::list and the shared legacy codec, with store unit tests
- [x] 01-03-PLAN.md: Config and service slice (wave 2). GET /ip from injected Settings, Settings::from_map with config tests, ARCH-04 startup smokes, PATCH 501 with parked PATCH tests, full phase gate

**Cross-cutting constraints:**
- cargo build, cargo test, cargo clippy --all-targets -- -D warnings and cargo fmt --check all exit 0 at the end of this plan (TOOL-03).

**Notes**: Delete the legacy `rust-toolchain` file and the old `Cargo.lock` first, because the legacy file takes precedence. Don't run `cargo fix --edition` (nothing compiles). Don't add a direct `v8` dependency. Write `deno_error = "0.7"` by hand. The first V8 build is slow, and `target/` must stay under the project dir (`/tmp` is nearly full). A temporary `LazyLock` bridge is fine here; Phase 2 removes the global.

### Phase 2: HTTP API Port & Error Handling

**Goal:** As a client of the rs3 HTTP API, I want to call every non-JavaScript route and get the same success bodies as before plus clear 4xx errors for missing keys and bad input, so that existing integrations keep working and bad requests can no longer crash the server.
**Mode:** mvp
**Depends on**: Phase 1
**Requirements**: HTTP-01, HTTP-02, HTTP-03, HTTP-04, HTTP-05, HTTP-06, HTTP-08, ROBU-01, ROBU-02, ARCH-02, ARCH-03, TEST-01, TEST-02
**Success Criteria** (what must be TRUE):
  1. `GET /`, `GET /ip`, `POST /insert`, `GET /key/{key}` and `DELETE /key/{key}` return 200 with the same body bytes as before (golden tests carried over from the old suite), and `GET /list` keeps its existing shape with keys and values correctly JSON-escaped (a value containing `"` or `\` round-trips)
  2. `GET`, `DELETE` or `PATCH` on a missing key returns 404 with a JSON error body, also on an empty store (no more `"[]"`); `POST /insert` returns 400 for malformed JSON and 422 for a non-object or non-string values, and nothing is inserted on either error
  3. The store and config are created once in `main` (outside the `HttpServer::new` closure) and injected through `web::Data`: a key inserted by one request is visible to the next on any worker, `GET /ip` reports the host loaded at startup without re-reading the environment, and no global map remains
  4. The HTTP tests use `#[actix_web::test]` and current actix test utilities through the same `configure(state)` helper as `main`; each test builds its own store and config in code with no `thread::sleep` and no `SERVER.*` environment variables, and the suite passes repeatedly under default parallel `cargo test`

**Plans:** 3/3 plans executed

Plans:
**Wave 1**
- [x] 02-01-PLAN.md: Injected state and JSON error contract (wave 1). AppState built once in main and injected via configure(state); ApiError JSON bodies; 404 for GET/DELETE/PATCH on missing keys; banner after bind; per-test state and golden carry-over
- [x] 02-02-PLAN.md: Config loader hardening (wave 1, parallel). load() via dotenv_iter + vars_os with no env mutation; dotenv errors stop startup; server.* allow-list; in-memory unit tests (dotenv-file live checks are operator human-checks)

**Wave 2** *(blocked on Wave 1 completion)*
- [x] 02-03-PLAN.md: Insert validation, escaped /list and lint gate (wave 2). Raw-bytes insert with 400 malformed_json / 422 invalid_input and nothing inserted; infallible insert_all; serde_json-escaped /list in the legacy envelope; unwrap/expect deny lints + clippy.toml; full phase gate

**Cross-cutting constraints:**
- cargo build --locked, cargo test, cargo clippy --all-targets -- -D warnings and cargo fmt --check all exit 0 at the end of every plan (TOOL-03); no lint-suppression attributes anywhere.

**Notes**: Use a raw-body parse for `/insert`, not `web::Json`. `web::Json` rejects requests without a JSON content type and collapses the 400/422 split. ARCH-02 forbids a global map, so Phase 3's ops have to reach this same store through `OpState`.

### Phase 3: JavaScript Runtime Port

**Goal:** As a user storing scripts in rs3, I want to PATCH a key and have its JavaScript run on current deno_core with the five store ops, so that the JS-executes-against-the-store behaviour works again and no script or op error can abort the server.
**Mode:** mvp
**Depends on**: Phase 2
**Requirements**: HTTP-07, JSRT-01, JSRT-02, JSRT-03, JSRT-04, JSRT-05, JSRT-06, ROBU-03, ROBU-04, TEST-03, TEST-05, TEST-06
**Success Criteria** (what must be TRUE):
  1. `PATCH /key/{key}` runs the stored script on current released `deno_core` and returns 200 with the stringified completion value; `fac.js`, `sum.js` and `bin.js` run as test fixtures with their expected results, and every one of the seven routes now has a success test asserting status and preserved body bytes
  2. Scripts can call `Deno.core.ops.op_rs3_list()`, `op_rs3_get(key)`, `op_rs3_insert(json)`, `op_rs3_delete(key)` and `op_rs3_patch(key)` (defined with `#[op2]`, registered through `extension!`) against the same injected store the HTTP handlers use, and a PATCH whose script returns `op_rs3_list()` yields the same content as `GET /list`
  3. A script that throws or has a syntax error makes PATCH return 422 with a JSON body containing the JS error message; an op called on a missing key or with invalid input raises an exception the script can `try/catch`, and the server keeps serving later requests (no `unwrap`/`expect` on request data in handlers, store or ops)
  4. JS runs on dedicated JS threads, not actix worker threads, with the V8 platform initialised once in `main` before any other thread spawns, and the full suite passes under default parallel `cargo test`
  5. Every JS artefact (`requests/*.js`, `requests/*.txt`, `requests/*.http`, `src/example.js`, test scripts) uses `Deno.core.ops.op_rs3_*(...)` with no `Deno.core.ops();` or `opSync` calls left, and the old `rs3_*` to new `op_rs3_*` mapping (including "a missing key throws") is documented

**Plans**: TBD
**Notes**: This is the highest-risk phase. Spike these early:
- a nested `JsRuntime` inside a `#[op2(reentrant)]` `op_rs3_patch`
- the test-side platform strategy (the `unsafe_use_unprotected_platform` feature, enabled only as a dev-dependency)
- the cost of `JsRuntime::new` per request

`web::block` is an acceptable stepping stone, but the phase must end on the bounded, dedicated JS thread pool with large stacks.

### Phase 4: CI Gate on x86_64

**Goal:** As a maintainer of rs3, I want to gate every push with a GitHub Actions workflow on an x86_64 runner that fails on any fmt, clippy or test failure, so that regressions and x86-only V8 crashes are caught before they land.
**Mode:** mvp
**Depends on**: Phase 3
**Requirements**: CI-01, CI-02, CI-03
**Success Criteria** (what must be TRUE):
  1. Pushing `modernize-toolchain` to GitHub runs a workflow on an x86_64 Linux runner using `actions/checkout` v4+, `dtolnay/rust-toolchain` and `Swatinem/rust-cache`, and it finishes green with the V8-backed JS tests run at the default test-thread count
  2. The workflow runs `cargo fmt --check`, `cargo clippy --all-targets --locked -- -D warnings` and `cargo test --locked`, and a failure in any of them fails the build (no `continue-on-error`, no `SERVER.*` env in the workflow)
  3. The archived `actions-rs` workflow is deleted, and the new workflow is the only one in `.github/workflows`

**Plans**: TBD
**Notes**: Verifying this phase requires pushing the branch to `origin` (github.com/jmfrank63/rs3). If the x86 run hits a V8 platform/PKU segfault, apply the documented fallback (`new_unprotected_default_platform` in a `Once`) and re-run. Optional: a `cargo tree -d` step that fails on a duplicate `v8`.

### Phase 5: JS Execution Limits

**Goal:** As a server operator running rs3, I want to rely on a nesting depth limit and an execution timeout for stored scripts, so that a self-patching or never-ending script can neither crash the server nor starve other requests.
**Mode:** mvp
**Depends on**: Phase 4 (builds on Phase 3's JS thread pool; the limits are verified under the CI gate)
**Requirements**: ROBU-05, ROBU-06, TEST-04
**Success Criteria** (what must be TRUE):
  1. A key whose script PATCHes itself, directly or through a cycle (A -> B -> A), returns 422 with a JSON error naming the depth limit and key chain, and the server process stays up
  2. A non-terminating script such as `while(true){}` is terminated after the configured timeout and PATCH returns 504, while other requests, including other PATCHes, keep being answered
  3. The execution timeout comes from settings, so tests set a short timeout in code without environment variables
  4. Every error path has a passing test (404 missing key, 400 malformed insert, 422 bad insert shape, 422 JS exception, 422 depth limit, 504 timeout), both locally and in CI

**Plans**: TBD
**Notes**: Research flag: the native stack needed per nesting level (about 16 MiB per JS thread; tune it with a recursion test), and classifying termination by reason rather than by message string. The heap limit, cancel-on-disconnect and 503 backpressure are v2 (JSHD-*).

### Phase 6: Docker Image & Documentation

**Goal:** As a new developer or deployer of rs3, I want to run it from a slim, non-root Docker image and learn the toolchain, JS op syntax and behaviour changes from the README, so that I can build, run and extend the modernized server without reading the source.
**Mode:** mvp
**Depends on**: Phase 5
**Requirements**: DOCK-01, DOCK-02, DOCK-03, DOCS-01, DOCS-02, DOCS-03
**Success Criteria** (what must be TRUE):
  1. `docker build` succeeds from a current `rust` toolchain builder with cargo-chef dependency caching and `--locked`, whose Debian release matches the runtime image, and `.dockerignore` keeps `target/` and other non-build files out of the build context
  2. `docker run` with `SERVER.HOST=0.0.0.0` starts a slim, non-root container, and `curl` to `GET /` from the host returns `"Welcome to rs3"`
  3. Following only `Readme.md`, a developer can set up Rust 1.99.1 / edition 2024, expect the slow first V8 build, and build, run and test the server
  4. `Readme.md` documents the `Deno.core.ops.op_rs3_*` syntax with the old-to-new op mapping and a working example, the HTTP error status codes, and a "Behaviour changes" section (404 instead of `"[]"`, 400/422 on insert, escaped `/list`)

**Plans**: TBD
**Notes**: Docker is not installed on the dev host, so the image build and the `docker run` + `curl /` smoke test must run where Docker is available (a CI job or the user's machine). Use a builder image that is not `-slim` (the V8 build.rs needs curl). Leave `docker_rs3/`, `aws/` and the `Vagrantfile` alone apart from trivial Rust-version references.

## Progress

**Execution Order:**
Phases execute in numeric order: 1 -> 2 -> 3 -> 4 -> 5 -> 6

| Phase | Plans Complete | Status | Completed |
|-------|----------------|--------|-----------|
| 1. Toolchain, Dependencies & Foundation | 3/3 | Complete    | 2026-10-20 |
| 2. HTTP API Port & Error Handling | 3/3 | In Progress | - |
| 3. JavaScript Runtime Port | 0/TBD | Not started | - |
| 4. CI Gate on x86_64 | 0/TBD | Not started | - |
| 5. JS Execution Limits | 0/TBD | Not started | - |
| 6. Docker Image & Documentation | 0/TBD | Not started | - |
