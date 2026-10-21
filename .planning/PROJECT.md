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

### Active

- [ ] JS engine ported to current `deno_core` (`#[op2]` ops registered via `extension!`), five store ops still callable from JS
- [ ] JS in tests, `requests/*.js` and `src/example.js` updated to `Deno.core.ops.op_name(...)`; old→new op-name mapping documented
- [ ] actix-web ported to current 4.x release API (test body handling, `test::init_service`, etc.)
- [ ] Request handlers return proper HTTP error codes instead of panicking on missing keys, bad JSON, or JS exceptions; response bodies stay compatible where tests assert them
- [ ] Tests deterministic and isolated — no shared global map races, no `thread::sleep`
- [ ] No global store: the temporary `LazyLock` bridge is replaced by injected `web::Data` / `OpState` state (ARCH-02)
- [ ] Store and config injected into the HTTP and JS layers (modules already split in Phase 1)
- [ ] CI on GitHub Actions with `actions/checkout@v4`+, `dtolnay/rust-toolchain`, `Swatinem/rust-cache`; check/test/fmt/clippy fail the build (no `continue-on-error`)
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
| Port JS ops to `#[op2]` + `extension!`, call via `Deno.core.ops.*` | Old op API removed from deno_core | — Pending |
| Op names may be renamed to valid identifiers (e.g. `op_rs3_get`), mapping documented | Modern ops are Rust fn identifiers | — Pending |
| Shared store injected (app state / op state) instead of global | Test isolation + future git/VS Code/TS layers | — Pending |
| Replace `flurry` if stale (candidates: `dashmap`, `papaya`) | flurry maintenance status | ✓ Good — `papaya` 0.2.5 behind a `Store` facade returning owned `String`s (Phase 1) |
| Stay a single binary crate | Brief constraint; no clear justification for a workspace yet | ✓ Good — single package as `lib.rs` + thin `main.rs` (Phase 1) |
| `GET /list` sorted by key | Legacy order was arbitrary hash order; deterministic output for tests | ✓ Shipped in Phase 1 (only visible store-route change in Phase 1) |
| Accept inert `v8x 149.4.0-rc.4` optional lock entry | Optional dep of `deno_v8`, never built; DEPS-01 verified on manifest + active graph (D-21) | ✓ Accepted (operator UAT, Phase 1) |
| Toolchain auto-install concurrency left unverified | Needs uninstalling 1.99.1; CI installs the pin explicitly | ⚠️ Accepted risk (operator, Phase 1) |
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
*Last updated: 2026-10-21: Phase 6 re-scoped from Docker to native deployment*
