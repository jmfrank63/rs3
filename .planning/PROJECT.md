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

### Active

- [ ] `rust-toolchain.toml` pinned to current stable (1.99) with `rustfmt` + `clippy`; `edition = "2024"`; `rust-version` set
- [ ] All dependencies on latest released versions — no git or beta deps; `Cargo.lock` regenerated
- [ ] JS engine ported to current `deno_core` (`#[op2]` ops registered via `extension!`), five store ops still callable from JS
- [ ] JS in tests, `requests/*.js` and `src/example.js` updated to `Deno.core.ops.op_name(...)`; old→new op-name mapping documented
- [ ] actix-web ported to current 4.x release API (test body handling, `test::init_service`, etc.)
- [ ] Request handlers return proper HTTP error codes instead of panicking on missing keys, bad JSON, or JS exceptions; response bodies stay compatible where tests assert them
- [ ] Tests deterministic and isolated — no shared global map races, no `thread::sleep`
- [ ] `lazy_static` replaced (`std::sync::LazyLock` or injected `web::Data` state)
- [ ] Code structured as separate store, JS runtime, and HTTP layers with injected shared state
- [ ] `config` crate ported to its current API; `dotenv` → `dotenvy`; `flurry` replaced if stale
- [ ] CI on GitHub Actions with `actions/checkout@v4`+, `dtolnay/rust-toolchain`, `Swatinem/rust-cache`; check/test/fmt/clippy fail the build (no `continue-on-error`)
- [ ] Dockerfile on a current `rust` image (cargo-chef ok) with a slim runtime image
- [ ] `Readme.md` updated for new toolchain, run instructions and new JS op syntax
- [ ] `cargo build`, `cargo test`, `cargo clippy -- -D warnings`, `cargo fmt --check` all pass

### Out of Scope

- `experiments/`, `aws/`, `docker_rs3/` Ansible content, `Vagrantfile` — explicitly left untouched this milestone (except trivial Rust-version references)
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
| Replace `flurry` if stale (candidates: `dashmap`, `papaya`) | flurry maintenance status | — Pending |
| Stay a single binary crate | Brief constraint; no clear justification for a workspace yet | — Pending |

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
*Last updated: 2026-10-20 after initialization*
