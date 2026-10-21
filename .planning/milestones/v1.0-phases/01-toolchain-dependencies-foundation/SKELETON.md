# Walking Skeleton — rs3

**Phase:** 1
**Generated:** 2026-10-20
**Delivered by:** plan 01-01 (wave 1). Plans 01-02 and 01-03 expand it in parallel in wave 2.

> Adapted for a brownfield modernization of an existing Rust HTTP service. rs3 has **no database** (in-memory store only) and **no UI** (an HTTP API exercised with `curl`). "Dev deployment" means a documented local `cargo run`. Docker is Phase 6 and CI is Phase 4.

## Capability Proven End-to-End

On Rust 1.99.1 with only released crates, a developer runs the `rs3` binary with `SERVER.HOST`/`SERVER.PORT` in the environment, and it does four things. It answers `GET /` with `"Welcome to rs3"`. It stores a key sent with `POST /insert`. It returns that key from `GET /key/{key}` through the papaya-backed `Store`. With missing or invalid configuration it prints one `error: …` line and exits 1 instead of panicking.

Local full-stack run command (the "deployment" for this phase):

```bash
cargo build --locked
env -i PATH="$PATH" SERVER.HOST=127.0.0.1 SERVER.PORT=5000 target/debug/rs3 &
curl -s http://127.0.0.1:5000/                                        # "Welcome to rs3"
curl -s -X POST -d '{"greeting":"hello"}' http://127.0.0.1:5000/insert  # "{\"greeting\":\"hello\"}"
curl -s http://127.0.0.1:5000/key/greeting                            # "hello"
kill %1
```

## Architectural Decisions

Later phases build on these. Each row cites the locked decision it implements.

| Decision | Choice | Rationale |
|---|---|---|
| Toolchain | `rust-toolchain.toml` pinned to `1.99.1` with `rustfmt` + `clippy`, `profile = "minimal"`; the legacy `rust-toolchain` file is deleted (D-01) | The exact pin keeps local, CI (Phase 4) and Docker (Phase 6) in step. The legacy file would take precedence if it remained |
| Edition / MSRV | `edition = "2024"`, `rust-version = "1.99"`, crate version `0.2.0` (D-02) | All deno crates are already edition 2024. MSRV-aware resolver 3 then picks current releases |
| Crate layout | One package with a `rs3` library (`src/lib.rs`) and a thin `rs3` binary (`src/main.rs`); no workspace (D-02, ARCH-01) | Tests and future binaries reach everything through the lib. A single package avoids re-linking V8 per crate |
| Module map | `config` (Settings), `store` (Store + `legacy` wire codec), `http` (`mod.rs` service routes + `kv.rs` store routes), `js` (V8 platform seam) | Dependencies point one way. `store` imports no web, JS or rs3 module. `http` reaches JS only through `js` |
| HTTP framework | actix-web 4.17, `default-features = false, features = ["macros"]`; attribute-macro routes; one `http::configure` route table used by `main` and tests | It is the current 4.x line, and the macros and test utilities ship in the crate, so there is no direct tokio or actix-rt dependency |
| Data layer | In-memory `Store`: a cheap-`Clone` newtype over `Arc<papaya::HashMap<String, String>>` with synchronous, owned-value methods (`get`, `insert`, `insert_many`, `remove`, `list` sorted by key, `len`, `is_empty`) and a `thiserror` `StoreError` (D-06, D-07) | Lock-free reads, and guards never escape a method, so JS ops that re-enter the store (Phase 3) cannot deadlock. There is no backend trait until the git backend arrives (FUT-01) |
| Wire format | `store::legacy` holds the rs3 v0 codec (`parse_insert_body`, `format_insert_echo`, `format_list`), shared by HTTP now and `op_rs3_list` later (D-08) | A single codec keeps `/list` and the JS listing byte-identical. Phase 2 changes escaping in one place |
| State access (temporary) | `pub(crate) static STORE: LazyLock<Store>` in `http::kv`, marked `TODO(phase-2): inject via web::Data` (D-09) | It bridges the handlers until Phase 2 injects the store via `web::Data` and `OpState` (ARCH-02). The `store` module itself holds no global |
| Configuration | `config` 0.15 builder (`Environment::default()`, no prefix or separator) + `dotenvy`; dotted `SERVER.HOST`/`SERVER.PORT`; no defaults; `Settings::from_map` for tests (D-12, D-13) | Keeps the existing env names. Tests never touch the process environment (setting env vars is `unsafe` in edition 2024) |
| Config injection | `Settings` is loaded once in `main` and registered as `web::Data<Settings>` outside the `HttpServer::new` closure; `GET /ip` reads it (D-22) | There is one copy per process, not one per worker, and handlers never re-read the environment |
| JS runtime seam | `deno_core` 0.412 (V8 150.4 through `deno_v8`, never a direct `v8` dependency); `js::init_platform()` runs on the main thread before `HttpServer` starts (D-05, D-23) | The V8 download, link and single-V8 check happen now. Phase 3 fills in the ops and executor behind this seam |
| Error handling | Per-layer `thiserror` enums; `main` returns `ExitCode` and prints a one-line `error: …` for config, bind and server errors; no `anyhow` (D-14) | This gives clean non-zero exits for supervisors. Handler-level legacy unwraps remain until Phase 2 (D-10) |
| Lints | `[lints.rust] unsafe_code = "forbid"` + `#![forbid(unsafe_code)]`; `cargo clippy --all-targets -- -D warnings`; no `allow` attributes (D-13, D-19) | Safety by construction. `clippy::unwrap_used` is added in Phase 2/3 once the handler unwraps are gone |
| Tests | In-crate `#[cfg(test)]` modules only (no `tests/*.rs` binaries, no doctest code fences); `#[actix_web::test]` with `actix_web::test as atest` (D-16) | Every extra test binary or doctest links about 180 MB of V8 |
| PATCH | `PATCH /key/{key}` → `501 Not Implemented` with a JSON error until Phase 3 (D-11) | The route keeps its place while the JS runtime is ported |

## Stack Touched in Phase 1

- [x] Project scaffold: toolchain pin, edition 2024 manifest, regenerated v4 lockfile, `cargo build`/`test`/`clippy -D warnings`/`fmt --check` green (01-01)
- [x] Routing: real routes `GET /` and `POST /insert` (01-01); `GET /key/{key}` (01-01); `DELETE /key/{key}` and `GET /list` (01-02); `GET /ip` and `PATCH` 501 (01-03)
- [x] Data layer: one real write (`POST /insert` → `Store::insert_many`) and one real read (`GET /key/{key}` → `Store::get`) on the papaya store (01-01)
- [x] Client interaction: `curl` against the running binary in every plan's smoke test (stands in for a UI; rs3 has none)
- [x] Deployment: the documented local full-stack run command above (`cargo run` / `target/debug/rs3`)

## Out of Scope (Deferred to Later Slices)

- Injecting the store through `web::Data`/`OpState` and removing the `STORE` global; typed HTTP errors (404/400/422); `/list` escaping; isolated tests with no shared store (Phase 2)
- `PATCH` executing JavaScript; `#[op2]` ops `op_rs3_*` with `extension!`; dedicated JS threads; migrating `requests/*` and `src/example.js` to the new op syntax (Phase 3)
- GitHub Actions on x86_64 replacing the archived `actions-rs` workflow, which stays untouched in Phase 1 (D-24) (Phase 4)
- JS nesting depth limit and execution timeout (Phase 5)
- Docker image, `.dockerignore`, README (Phase 6)
- Config defaults and the `SERVER__HOST` alias (OPS-02, v2), the `StoreBackend` trait (FUT-01), and the `clippy::unwrap_used` lint (Phase 2/3)

## Subsequent Slice Plan

Each later phase adds one vertical slice on top of this skeleton without altering its architectural decisions:

- Phase 2: clients call every non-JS route with the same success bodies and clear 4xx errors, on injected state with isolated tests
- Phase 3: PATCH runs stored JavaScript on current deno_core with the five `op_rs3_*` ops against the same injected store
- Phase 4: every push is gated by fmt/clippy/test on an x86_64 GitHub Actions runner
- Phase 5: self-patching and non-terminating scripts are stopped by a depth limit (422) and a timeout (504)
- Phase 6: rs3 runs from a slim non-root Docker image, documented in the README
