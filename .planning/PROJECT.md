# rs3

## What This Is

rs3 is a small Rust key/value "information object store" (originally a 2021 Birkbeck MSc
Computer Science project). It is an actix-web HTTP server that stores string key/value pairs
in a concurrent in-memory map and can execute a stored value as JavaScript inside an embedded
V8 runtime (`deno_core`), where the JS can call back into the store through registered ops.
Milestone v1.0 (2026-10-21) moved it onto Rust 1.99.1 / edition 2024 with released crates only,
added hard limits for runaway scripts, and replaced the Docker deployment with a native binary
run by an example hardened systemd unit. It is the base for later agentic-coding features
(git integration, VS Code access, TypeScript execution).

## Core Value

rs3 builds, tests and lints cleanly on current stable Rust (edition 2024) with only released
dependencies — while every existing HTTP route and the JS-executes-against-the-store
behaviour keep working with the same semantics.

Held through v1.0. Re-check it at `/gsd-new-milestone`: the next milestone adds features, so the
core value may move from "modernize without breaking" towards the agentic-coding goals.

## Current State

**v1.0 rs3 toolchain modernization: shipped 2026-10-21** on branch `modernize-toolchain`.
The branch is not yet merged into `main` or pushed, and the `v1.0` tag is local only.

- 6 phases and 16 plans. All 47 v1 requirements are checked off. DEPL-03 is partial: the unit's root-only behaviour still needs a real server.
- About 5,600 lines of Rust in 13 files under `src/`, and 216 tests. The full gate is green locally and on x86_64 CI (run 39527188207, 0 annotations).
- The milestone closed with an override. Phases 02 and 06 are `human_needed`, with 02-UAT items 2–3 and 06-UAT items 1–7 open. See STATE.md Deferred Items and `milestones/v1.0-MILESTONE-AUDIT.md`.

## Next Milestone Goals

These are defined with `/gsd-new-milestone`. The candidates are already tracked as v2 requirements in `milestones/v1.0-REQUIREMENTS.md`:

- **Agentic-coding features:**
  - a git-backed store (FUT-01);
  - VS Code / LSP-style access (FUT-02);
  - TypeScript via tsc-rs (FUT-03);
  - serving git repositories over HTTP (FUT-04).
- **JS runtime hardening:**
  - a V8 heap limit (JSHD-01);
  - cancelling a script when the client disconnects (JSHD-02);
  - a bounded queue that answers 503 (JSHD-03);
  - a startup snapshot (JSHD-04).
- **API evolution and ops:**
  - APIV-01..03;
  - structured logging (OPS-01);
  - the `SERVER__HOST` alias (OPS-02);
  - extra CI jobs (OPS-03);
  - release profile tuning (OPS-04).
- **Carry-over from v1.0:**
  - run the remaining human checks (06-UAT on a real server, the 02-UAT dotenv-file checks);
  - merge `modernize-toolchain` into `main`, which brings `LICENSE`;
  - settle the `MemoryMax=` sizing wording.

## Requirements

### Validated

<!-- "existing" items were inferred from the 2021 code and kept working through v1.0. -->

- ✓ `GET /` returns the JSON welcome string `"Welcome to rs3"` — existing
- ✓ `GET /ip` returns the configured host — existing
- ✓ `POST /insert` takes a JSON object and inserts every key/value — existing
- ✓ `GET /list` returns all entries — existing (sorted by key and JSON-escaped since v1.0)
- ✓ `GET /key/{key}` returns the value for a key — existing
- ✓ `DELETE /key/{key}` removes a key and returns the old value — existing
- ✓ `PATCH /key/{key}` executes the stored value as JavaScript in V8 and returns the result — existing
- ✓ JS running in the runtime can call the store via five ops (list, get, insert, delete, patch) — existing (renamed `op_rs3_*` in v1.0)
- ✓ Configuration from `.env` / environment (`SERVER.HOST`, `SERVER.PORT`) — existing
- ✓ `rust-toolchain.toml` pinned to Rust 1.99.1 with `rustfmt` + `clippy`; `edition = "2024"`; `rust-version = "1.99"` — v1.0 (Phase 1)
- ✓ All dependencies on released crates.io versions (no git/beta); `Cargo.lock` regenerated; single V8 via `deno_core` — v1.0 (Phase 1)
- ✓ `dotenv` → `dotenvy`, `flurry` → `papaya`, `lazy_static` removed (the Phase 1 `LazyLock` bridge was replaced by injected state in Phase 2) — v1.0 (Phases 1–2)
- ✓ `config` crate on the 0.15 builder API, still reading dotted `SERVER.HOST` / `SERVER.PORT` from `.env` or the environment — v1.0 (Phase 1)
- ✓ `lib.rs` + thin `main.rs` with separate `store`, `config`, `http`, `js` modules; startup errors exit 1 instead of panicking — v1.0 (Phase 1)
- ✓ `cargo build`, `cargo test`, `cargo clippy --all-targets -- -D warnings`, `cargo fmt --check` all pass (gate held at every later phase) — v1.0 (Phase 1)
- ✓ actix-web on the released 4.17 API (`#[actix_web::main]` / `#[actix_web::test]`, `test::init_service`, `Bytes` bodies) — v1.0 (Phases 1–2)
- ✓ Handlers return JSON errors instead of panicking. A missing key gives 404 `not_found`, a bad insert gives 400 `malformed_json` or 422 `invalid_input` and writes nothing, a script failure gives 422 `js_error`, and a host fault gives 500 `internal`. `unwrap_used` / `expect_used` are denied outside tests — v1.0 (Phases 2–3)
- ✓ No global store. `AppState { store, Arc<Settings> }` is built once in `main`, shared by every route through one `web::Data`, and handed to the ops through per-job `OpState` — v1.0 (Phases 2–3)
- ✓ Tests are deterministic and isolated: each test has its own store and builds its config in code without reading the environment. The only `thread::sleep` calls are two bounded stall fixtures in the timeout tests — v1.0 (Phases 2, 5)
- ✓ JS engine on current released `deno_core` 0.412: five `#[op2]` ops (`op_rs3_list/get/insert/delete/patch`) registered through one `extension!`, called as `Deno.core.ops.op_rs3_*(...)` against the same injected store as the HTTP routes; JS runs on a dedicated `rs3-js-*` pool off the actix workers, V8 initialised once in `main`; script and op errors answer 422 `js_error` instead of aborting — v1.0 (Phase 3)
- ✓ Every JS artefact (`requests/*`, `src/example.js`, tests) migrated to the new op syntax, and the legacy `rs3_*` → `op_rs3_*` mapping documented in `docs/js-ops.md` (and summarised in the README) — v1.0 (Phase 3)
- ✓ CI on GitHub Actions: one `ci` job on an x86_64 `ubuntu-26.04` runner with `actions/checkout@v7`, `dtolnay/rust-toolchain@1.99.1`, `Swatinem/rust-cache@v2`; guards, fmt, clippy `-D warnings`, `cargo test --locked`, release build and release smoke all fail the build (no `continue-on-error`); remote runs 39507141932 and 39527188207 green with 0 annotations — v1.0 (Phase 4)
- ✓ No script can crash or pin the server: a PATCH nesting depth limit (default 4) stops self-patching chains and cycles with 422 `js_depth_limit` and the key chain, and an execution timeout (default 5000 ms, per-worker watchdog) ends runaway scripts with 504 `js_timeout` while other requests keep being answered; both limits are set in code and verified on x86_64 CI (run 39527188207) and by an operator client-disconnect test — v1.0 (Phase 5)
- ✓ Native deployment on a single Linux server: `cargo build --release --locked`, the example hardened unit `deploy/rs3.service` (DynamicUser, an `env(1)` bridge for the dotted variables, `Restart=on-failure` with a start limit, seccomp `@system-service @pkey`, `MemoryMax=1G`, exposure 1.5) and a five-line install. The `Dockerfile` and `docker_rs3/` are removed — v1.0 (Phase 6). The root-only directives and x86_64 PKU still need an operator run on a real server (06-UAT items 1–3)
- ✓ `Readme.md` rewritten: toolchain, build/run/test, configuration, HTTP API, JS ops and mapping, limits, behaviour changes versus 2021, deployment and the MIT license. `Cargo.toml` declares `license = "MIT"`. Every documented body was replayed against the release binary (130 checks) — v1.0 (Phase 6). A newcomer readability review is pending (06-UAT item 5)

### Active

None yet. `/gsd-new-milestone` defines the next milestone's requirements; the candidates are listed under Next Milestone Goals.

### Out of Scope

- `experiments/`, `aws/`, `Vagrantfile` — left untouched in v1.0. The one exception is the deleted stale `experiments/web_scrape_s3_api/Cargo.lock`, which triggered Dependabot alerts. The `Vagrantfile` still points at the deleted `docker_rs3/`; it is MSc-era material, kept at tag `msc`
- Container images — dropped on 2026-10-21: rs3 ships as a native binary on a single Linux server (see Key Decisions)
- Git integration / serving repos over HTTP — future milestone (FUT-01, FUT-04); keep architecture open for it
- VS Code integration / LSP-style access — future milestone (FUT-02)
- TypeScript execution via tsc-rs — future milestone (FUT-03); JS runtime layer should be swappable/extendable
- Workspace split into multiple crates — single binary crate unless clearly justified (each extra test binary re-links V8)
- Persistence / new store features — not planned. The store stays in memory and is empty after every restart. v1.0 was a modernization, not a feature milestone

## Context

State after v1.0 (2026-10-21):

- **Code:** a single crate, `lib.rs` plus a thin `main.rs`, with these modules:
  - `config`: the config 0.15 builder plus dotenvy;
  - `store`: papaya behind a `Store` facade that returns owned `String`s. `legacy::format_list` is the only `/list` codec;
  - `http`: actix-web 4.17, `AppState` and `ApiError`;
  - `js`: deno_core 0.412 on V8 150.4.0, with five `#[op2]` ops in `rs3_ext`, a bounded `rs3-js-*` worker pool, and a per-job depth limit and watchdog timeout.

  That is about 5,600 lines of Rust in 13 files under `src/`, with 216 tests.
- **Toolchain:** `rust-toolchain.toml` pins 1.99.1 with edition 2024 and `rust-version = "1.99"`. Normal builds use the protected V8 platform. Only test builds enable `unsafe_use_unprotected_platform`, through a dev-dependency.
- **CI:** `.github/workflows/ci.yml` has one job on x86_64 `ubuntu-26.04`. It runs the repo guards, fmt, clippy `-D warnings`, `cargo test --locked`, the release build and a release-binary smoke. `scripts/check-workflow.py` pins the workflow.
- **Deployment:** `deploy/rs3.service` and `deploy/rs3.conf` are examples that listen on loopback by default. rs3 has no authentication, so the README tells operators to put a reverse proxy with auth and TLS in front of it.
- **Docs:** `Readme.md` is the full user and operator guide. `docs/js-ops.md` holds the op contract and the legacy mapping.
- **Known issues and tech debt** (details in `milestones/v1.0-MILESTONE-AUDIT.md`):
  - Human checks still open:
    - 06-UAT items 1–7: the real-server unit run, x86_64 PKU, the sudo update and troubleshooting commands, the `MemoryMax=` sizing wording, README readability, remote CI on the final HEAD and the judgment-tier prohibitions;
    - 02-UAT items 2–3: the live dotenv-file checks.
  - There is no V8 heap limit yet (JSHD-01). An allocating script can abort the process, and the unit then restarts it with an empty store.
  - The `MemoryMax=` sizing rule slightly undercounts mixed `GET /list` plus `op_rs3_list()` load. The verifier measured about 20× the store, against the rule's 17×.
  - The path where a runtime registers after its timeout has already fired is untested. This is an accepted risk, because the watchdog's 50 ms re-fire covers it.
  - Logging is plain stderr/stdout (OPS-01). A script's `Deno.core.print` output reaches stdout raw.
  - `LICENSE` lives on `main` (9d1596b) and arrives with the merge.
- **Before v1.0** (the 2021 baseline, at tag `msc`):
  - about 390 lines of Rust on Rust 1.56 / edition 2018;
  - actix-web 4.0.0-beta.8, deno_core from git, `flurry`, `lazy_static` and `dotenv`;
  - a global map, and `unwrap()` panics on bad input;
  - an archived `actions-rs` CI with `continue-on-error`.

## Constraints

- **Toolchain**: Rust 1.99.1 (pinned), edition 2024 — the v1.0 goal, met
- **Platform**: Linux aarch64 dev host, CI on x86_64 (`ubuntu-26.04`); V8 prebuilt static libs (glibc) from the `v8` crate on both; the first V8 build is slow (expected)
- **Compatibility**: Same HTTP routes and semantics; keep response bodies compatible where tests assert them
- **Dependencies**: Released versions only — no git, no beta
- **Structure**: Single binary crate; modular store / JS runtime / HTTP layers with injected state
- **Deployment**: Native binary on a single Linux server under systemd; no container images
- **Process**: Work on branch `modernize-toolchain`; commit per plan with clear messages; the operator pushes

## Key Decisions

| Decision | Rationale | Outcome |
|----------|-----------|---------|
| Port JS ops to `#[op2]` + `extension!`, call via `Deno.core.ops.*` | Old op API removed from deno_core | ✓ Good — five ops in one `rs3_ext` extension, `op_panic` disabled (Phase 3) |
| Op names may be renamed to valid identifiers (e.g. `op_rs3_get`), mapping documented | Modern ops are Rust fn identifiers | ✓ Good — `rs3_*` → `op_rs3_*`, mapping in docs/js-ops.md and the README (Phases 3 and 6) |
| Shared store injected (app state / op state) instead of global | Test isolation + future git/VS Code/TS layers | ✓ Good — `AppState` built once in `main`, one `web::Data`, per-job `OpState` (Phases 2 and 3) |
| Replace `flurry` if stale (candidates: `dashmap`, `papaya`) | flurry maintenance status | ✓ Good — `papaya` 0.2.5 behind a `Store` facade returning owned `String`s (Phase 1) |
| Stay a single binary crate | Brief constraint; no clear justification for a workspace yet | ✓ Good — single package as `lib.rs` + thin `main.rs` (Phase 1) |
| `GET /list` sorted by key | Legacy order was arbitrary hash order; deterministic output for tests | ✓ Shipped in Phase 1 (only visible store-route change in Phase 1) |
| Accept inert `v8x 149.4.0-rc.4` optional lock entry | Optional dep of `deno_v8`, never built; DEPS-01 verified on manifest + active graph (D-21) | ✓ Accepted (operator UAT, Phase 1) |
| Toolchain auto-install concurrency left unverified | Needs uninstalling 1.99.1; CI installs the pin explicitly | ⚠️ Accepted risk (operator, Phase 1) |
| Normal builds keep the protected V8 platform (`JsRuntime::init_platform(None)`); `unsafe_use_unprotected_platform` only for test builds through a dev-dependency | The Phase 4 fallback (move the feature into `[dependencies]` if x86 CI crashed) was not needed | ✓ Good — x86_64 CI and the release smoke are green on the protected platform (Phases 4–5) |
| JS limits are hard stops set in code: depth 4 (clamped 1..=64) → 422, timeout 5000 ms via a per-worker watchdog → 504, decided by reason (job phase CAS), never by message text | A self-patching or never-ending script must neither crash nor starve the server; tests set short limits without env vars | ✓ Good (Phase 5: CI green on x86_64, operator client-disconnect test; late-registration path an accepted risk) |
| Behaviour changes versus 2021 accepted and documented: `/list` sorted and JSON-escaped, 404 JSON for missing keys (no `"[]"`), 400/422 on bad inserts, ops renamed `rs3_*` → `op_rs3_*`, PATCH errors 422/504/500, depth limit and timeout, strict dotenv handling | The user's Phase 6 scope (2026-10-21) asks the README to document exactly these as "behaviour changes vs the 2021 version" | ✓ Accepted — README "Behaviour changes" section (Phase 6) |
| One CI job on `ubuntu-26.04` x86_64 (operator amendment of Phase 4 D-01 from `ubuntu-24.04`), workflow pinned by `scripts/check-workflow.py` and guard (g) | Latest runner image; the canonical-lines allow-list keeps the gate from being weakened silently | ✓ Good — remote runs 39507141932 and 39527188207 green, 0 annotations (Phase 4) |
| Drop Docker: ship rs3 as a native binary on a single Linux server (release build plus an example hardened systemd unit); delete the root `Dockerfile` and the `docker_rs3/` Docker/Ansible multi-container setup, add no `.dockerignore`, and re-word the Docker requirements (DOCK-01..03 → DEPL-01..03) | User decision 2026-10-21: one server for now, so a container runtime adds nothing; the multi-container setup was obsolete. The MSc-era version stays at git tag `msc` | ✓ Good — shipped in Phase 6 (`deploy/rs3.service`, README Deployment); the unit's root-only behaviour is pending operator UAT |
| The example unit passes `SERVER.HOST` / `SERVER.PORT` through an `env(1)` bridge from `RS3_HOST` / `RS3_PORT` in `/etc/rs3/rs3.conf` | systemd 255 drops variable names containing dots from `EnvironmentFile=` | ✓ Good — live-proven under the systemd user manager (Phase 6) |
| V8-compatible sandbox: seccomp `@system-service @pkey`, no `MemoryDenyWriteExecute=` | V8's JIT needs writable and executable memory (`MemoryDenyWriteExecute=yes` crashes the first PATCH), and `@system-service` lacks the `pkey_*` calls the protected platform uses on PKU CPUs | ✓ Good on aarch64 (kill-action seccomp run, no SIGSYS); x86_64 PKU pending operator UAT (Phase 6) |
| `MemoryMax=1G` plus `MemorySwapMax=0`, with a documented sizing rule | A runaway allocation must not take the host down; on cgroup v2 `MemoryMax=` alone does not cap swap | ⚠️ Revisit — the rule slightly undercounts mixed `/list` plus `op_rs3_list()` load (06-UAT item 4) |
| The README documents only bodies observed from the release binary, replayed byte for byte; the few lines taken from the code are labelled | The docs must match the live contract | ✓ Good — 130-check replay (Phase 6) |

## Evolution

This document evolves at phase transitions and milestone boundaries.

**After each phase transition** (via `/gsd-transition`):
1. Requirements invalidated? → Move to Out of Scope with reason
2. Requirements validated? → Move to Validated with phase reference
3. New requirements emerged? → Add to Active
4. Decisions to log? → Add to Key Decisions
5. "What This Is" still accurate? → Update if drifted

**After each milestone** (via `/gsd-complete-milestone`):
1. Full review of all sections
2. Core Value check — still the right priority?
3. Audit Out of Scope — reasons still valid?
4. Update Context with current state

---
*Last updated: 2026-10-21 after v1.0 milestone*
