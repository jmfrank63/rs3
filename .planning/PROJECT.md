# rs3

## What This Is

rs3 is a small Rust key/value "information object store" (originally a 2021 Birkbeck MSc
Computer Science project). It is an actix-web HTTP server that stores string key/value pairs
in a concurrent in-memory map and can execute a stored value as JavaScript inside an embedded
V8 runtime (`deno_core`), where the JS can call back into the store through registered ops.
This milestone modernizes it onto the current stable Rust toolchain so it can serve as the
base for later agentic-coding features (git integration, VS Code access, TypeScript execution).

## Core Value

rs3 builds, tests and lints cleanly on current stable Rust (edition 2024) with only released
dependencies — while every existing HTTP route and the JS-executes-against-the-store
behaviour keep working with the same semantics.

## Requirements

### Validated

<!-- Inferred from existing code (src/main.rs, src/services, src/bindings, src/config). -->

- ✓ `GET /` returns the JSON welcome string `"Welcome to rs3"` — existing
- ✓ `GET /ip` returns the configured host — existing
- ✓ `POST /insert` takes a JSON object and inserts every key/value — existing
- ✓ `GET /list` returns all entries — existing
- ✓ `GET /key/{key}` returns the value for a key — existing
- ✓ `DELETE /key/{key}` removes a key and returns the old value — existing
- ✓ `PATCH /key/{key}` executes the stored value as JavaScript in V8 and returns the result — existing
- ✓ JS running in the runtime can call the store via five ops (list, get, insert, delete, patch) — existing
- ✓ Configuration from `.env` / environment (`SERVER.HOST`, `SERVER.PORT`) — existing
- ✓ `rust-toolchain.toml` pinned to Rust 1.99.1 with `rustfmt` + `clippy`; `edition = "2024"`; `rust-version = "1.99"` — Phase 1
- ✓ All dependencies on released crates.io versions (no git/beta); `Cargo.lock` regenerated; single V8 via `deno_core` — Phase 1
- ✓ `lazy_static` → `std::sync::LazyLock` (temporary bridge), `dotenv` → `dotenvy`, `flurry` → `papaya` — Phase 1
- ✓ `config` crate on the 0.15 builder API, still reading dotted `SERVER.HOST` / `SERVER.PORT` from `.env` or the environment — Phase 1
- ✓ `lib.rs` + thin `main.rs` with separate `store`, `config`, `http`, `js` modules; startup errors exit 1 instead of panicking — Phase 1
- ✓ `cargo build`, `cargo test`, `cargo clippy --all-targets -- -D warnings`, `cargo fmt --check` all pass (gate held at every later phase) — Phase 1
- ✓ JS engine on current released `deno_core` 0.412: five `#[op2]` ops (`op_rs3_list/get/insert/delete/patch`) registered through one `extension!`, called as `Deno.core.ops.op_rs3_*(...)` against the same injected store as the HTTP routes; JS runs on a dedicated `rs3-js-*` pool off the actix workers, V8 initialised once in `main`; script and op errors answer 422 `js_error` instead of aborting — Phase 3
- ✓ Every JS artefact (`requests/*`, `src/example.js`, tests) migrated to the new op syntax, and the legacy `rs3_*` → `op_rs3_*` mapping documented in `docs/js-ops.md` (and summarised in the README) — Phase 3
- ✓ CI on GitHub Actions: one `ci` job on an x86_64 `ubuntu-26.04` runner with `actions/checkout@v7`, `dtolnay/rust-toolchain@1.99.1`, `Swatinem/rust-cache@v2`; guards, fmt, clippy `-D warnings`, `cargo test --locked`, release build and release smoke all fail the build (no `continue-on-error`); remote runs 39507141932 and 39527188207 green with 0 annotations — Phase 4
- ✓ No script can crash or pin the server: a PATCH nesting depth limit (default 4) stops self-patching chains and cycles with 422 `js_depth_limit` and the key chain, and an execution timeout (default 5000 ms, per-worker watchdog) ends runaway scripts with 504 `js_timeout` while other requests keep being answered; both limits are set in code and verified on x86_64 CI (run 39527188207) and by an operator client-disconnect test — Phase 5

### Active

- [ ] actix-web ported to current 4.x release API (test body handling, `test::init_service`, etc.)
- [ ] Request handlers return proper HTTP error codes instead of panicking on missing keys, bad JSON, or JS exceptions; response bodies stay compatible where tests assert them
- [ ] Tests deterministic and isolated — no shared global map races, no `thread::sleep`
- [ ] No global store: the temporary `LazyLock` bridge is replaced by injected `web::Data` / `OpState` state (ARCH-02)
- [ ] Store and config injected into the HTTP and JS layers (modules already split in Phase 1)
- [ ] Native deployment on a single Linux server: release build (`cargo build --release --locked`) and an example hardened systemd unit; the obsolete `Dockerfile` and `docker_rs3/` are removed
- [ ] `Readme.md` updated for new toolchain, configuration, run instructions, HTTP API, new JS op syntax, behaviour changes and the MIT license (`license = "MIT"` in `Cargo.toml`)

### Out of Scope

- `experiments/`, `aws/`, `Vagrantfile` — explicitly left untouched this milestone (the one exception is the deleted stale `experiments/web_scrape_s3_api/Cargo.lock`, which triggered Dependabot alerts)
- Container images — dropped on 2026-10-21: rs3 ships as a native binary on a single Linux server (see Key Decisions)
- Git integration / serving repos over HTTP — future milestone; keep architecture open for it
- VS Code integration / LSP-style access — future milestone
- TypeScript execution via tsc-rs — future milestone; JS runtime layer should be swappable/extendable
- Workspace split into multiple crates — single binary crate unless clearly justified
- Persistence / new store features — this is a modernization, not a feature milestone

## Context

- Brownfield codebase, ~390 lines of Rust across `src/main.rs`, `src/config`, `src/bindings`, `src/services`, plus JS samples in `requests/` and `src/example.js`.
- Currently pinned to Rust 1.56 (`rust-toolchain` file), edition 2018; does not build on a modern toolchain.
- Deps: `actix-web 4.0.0-beta.8`, `deno_core` from git `main` (old `op_sync`/`register_op`/`sync_ops_cache`/`Deno.core.opSync` API), `rusty_v8 0.27` (now the `v8` crate), `config 0.11` (old `merge`/`try_into`), `dotenv`, `lazy_static`, `flurry 0.3`.
- Global `lazy_static` `MAP` is accessed directly by both HTTP handlers and JS ops.
- Known robustness bugs: `unwrap()` on missing keys (`get`, `delete`), bad JSON / non-string values (`insert`), JS exceptions (`patch`); `get` returns `"[]"` when map is empty regardless of key; `list` hand-builds JSON without escaping.
- Tests live in `src/main.rs`, share the global map, and use `thread::sleep` to dodge races.
- `patch` creates a new V8 context to read the result value — likely needs rework with modern deno_core.
- CI (`.github/workflows/actions-rs.yml`) uses archived `actions-rs/*` with `continue-on-error: true`; Dockerfile uses `rust:1.53`.

## Constraints

- **Toolchain**: Latest stable Rust (1.99), edition 2024 — milestone goal
- **Platform**: Linux aarch64 — V8 prebuilt static libs from the `v8` crate are available; first V8 build is slow (expected)
- **Compatibility**: Same HTTP routes and semantics; keep response bodies compatible where tests assert them
- **Dependencies**: Released versions only — no git, no beta
- **Structure**: Single binary crate; modular store / JS runtime / HTTP layers with injected state
- **Process**: Work on branch `modernize-toolchain`; commit per plan with clear messages

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
| JS limits are hard stops set in code: depth 4 (clamped 1..=64) → 422, timeout 5000 ms via a per-worker watchdog → 504, decided by reason (job phase CAS), never by message text | A self-patching or never-ending script must neither crash nor starve the server; tests set short limits without env vars | ✓ Good (Phase 5: CI green on x86_64, operator client-disconnect test; late-registration path an accepted risk) |
| Behaviour changes versus 2021 accepted and documented: `/list` sorted and JSON-escaped, 404 JSON for missing keys (no `"[]"`), 400/422 on bad inserts, ops renamed `rs3_*` → `op_rs3_*`, PATCH errors 422/504/500, depth limit and timeout, strict dotenv handling | The user's Phase 6 scope (2026-10-21) asks the README to document exactly these as "behaviour changes vs the 2021 version" | ✓ Accepted — README "Behaviour changes" section (Phase 6) |
| One CI job on `ubuntu-26.04` x86_64 (operator amendment of Phase 4 D-01 from `ubuntu-24.04`), workflow pinned by `scripts/check-workflow.py` and guard (g) | Latest runner image; the canonical-lines allow-list keeps the gate from being weakened silently | ✓ Good — remote runs 39507141932 and 39527188207 green, 0 annotations (Phase 4) |
| Drop Docker: ship rs3 as a native binary on a single Linux server (release build plus an example hardened systemd unit); delete the root `Dockerfile` and the `docker_rs3/` Docker/Ansible multi-container setup, add no `.dockerignore`, and re-word the Docker requirements (DOCK-01..03 → DEPL-01..03) | User decision 2026-10-21: one server for now, so a container runtime adds nothing; the multi-container setup was obsolete. The MSc-era version stays at git tag `msc` | — Pending (Phase 6) |

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
*Last updated: 2026-10-21 after Phases 3 and 4 were re-verified and completed (Phase 6 executed; human UAT pending)*
