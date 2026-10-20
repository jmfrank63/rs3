# Phase 1: Toolchain, Dependencies & Foundation - Context

**Gathered:** 2026-10-20
**Status:** Ready for planning

<domain>
## Phase Boundary

Get rs3 compiling, linting and testing on Rust 1.99.1 / edition 2024 with only released
crates, restructured as `lib.rs` + thin `main.rs` with `store`, `config`, `http` and `js`
modules. Delivers: `rust-toolchain.toml`, new `Cargo.toml` + regenerated `Cargo.lock`, a
papaya-backed `Store` and a `config`+`dotenvy` loader (both unit-tested), the existing
non-JS routes compiling on actix-web 4.17 with today's behaviour, and clean startup-error
handling. PATCH may answer 501 until Phase 3.

Not in this phase: removing the global store from handlers / `web::Data` injection (Phase 2,
ARCH-02/03), new HTTP error codes (Phase 2), deno_core ops and JS execution (Phase 3), CI
(Phase 4), Docker/README (Phase 6).

</domain>

<decisions>
## Implementation Decisions

### Toolchain & manifest
- **D-01:** `rust-toolchain.toml` = `channel = "1.99.1"`, `components = ["rustfmt", "clippy"]`, `profile = "minimal"`. Delete the legacy `rust-toolchain` file in the same commit (it takes precedence otherwise).
- **D-02:** `Cargo.toml`: `edition = "2024"`, `rust-version = "1.99"`, version bumped to `0.2.0` (signals the modernized line). Single package with both `[lib]` (`src/lib.rs`, crate name `rs3`) and the `rs3` binary — still one crate/package, no workspace.
- **D-03:** Delete the old `Cargo.lock` and regenerate it; never run `cargo fix --edition` (old tree doesn't compile).
- **D-04:** Dependency set from `.planning/research/STACK.md`: `actix-web 4.17` (`default-features = false, features = ["macros"]`), `deno_core 0.412`, `deno_error = "0.7"` (hand-written, not `cargo add`), `papaya 0.2`, `config 0.15` (`default-features = false`), `dotenvy 0.15`, `serde` (derive), `serde_json` (`preserve_order`), `thiserror 2`. No direct `v8`, `rusty_v8`, `tokio`, `actix-rt`, `lazy_static`, `dotenv`, `flurry`, `anyhow`. — **Reversibility:** reversible
- **D-05:** `deno_core` is added in Phase 1 (so the V8 prebuilt download, link and `cargo tree -d` single-V8 check happen now), referenced from a minimal `js` module stub; real ops arrive in Phase 3.

### Store API & list ordering
- **D-06:** `Store` is a concrete, cheap-`Clone` newtype over `Arc<papaya::HashMap<String, String>>` — no `StoreBackend` trait yet (extract when the git backend arrives). `store` imports nothing from actix-web or deno_core.
- **D-07:** API is synchronous, fallible, and returns owned values (no guards leak): `get(&str) -> Result<String, StoreError>`, `insert(String, String)`, `insert_many(iter)` (validate-then-write, all-or-nothing from the caller's view), `remove(&str) -> Result<String, StoreError>`, `list() -> Vec<(String, String)>` sorted by key, `is_empty()`/`len()`. `StoreError` via `thiserror` with at least `NotFound(key)`.
- **D-08:** `store/legacy.rs` holds the rs3 v0 wire-format helpers (parse insert body → map of strings, format `/list` output) so `/list` and the future `op_rs3_list` share one codec. In Phase 1 the codec keeps the current output shape; the escaping/sort/404 behaviour changes land in Phase 2 with their tests.

### Phase-1 store access bridge
- **D-09:** Handlers in Phase 1 reach the store through a temporary `static STORE: LazyLock<Store>` in the `http` module (replacing `lazy_static` MAP). Explicitly marked `// TODO(phase-2): inject via web::Data` — Phase 2 removes it. `store` itself contains no global.
- **D-10:** Handlers are ported to actix-web 4.17 with today's success semantics/bodies unchanged; Phase 1 does not change status codes beyond what's needed to compile (`unwrap`s inside handlers may remain until Phase 2 replaces them).
- **D-11:** `PATCH /key/{key}` returns `501 Not Implemented` with a short JSON message until Phase 3.

### Config shape & failure behaviour
- **D-12:** `config::Settings { server: ServerSettings { host: String, port: u16 } }` (module `config`; rename from `Config` to avoid clashing with the `config` crate). `Settings::load()` = `dotenvy::dotenv().ok()` then `Config::builder().add_source(Environment::default()).build()?.try_deserialize()` — keeps dotted `SERVER.HOST` / `SERVER.PORT`. No defaults added (defaults/`SERVER__HOST` alias deferred).
- **D-13:** A `Settings::from_source(...)` / in-code constructor exists for tests so unit tests never mutate process env (`set_var` is `unsafe` in edition 2024). `#![forbid(unsafe_code)]` crate-wide.
- **D-14:** `main` returns `std::process::ExitCode` (or `Result<(), Box<dyn Error>>`): config load errors and bind errors print a one-line `error: ...` to stderr and exit non-zero — no panics, no `anyhow`.
- **D-15:** `GET /ip` still reports the configured host; in Phase 1 it may keep reading settings the same way it does today (ARCH-03 "load once and inject" is Phase 2).

### Legacy tests during the transition
- **D-16:** Existing tests move out of `main.rs` into the `http` module's `#[cfg(test)]` and are ported to actix-web 4.17 test APIs (`test::init_service(...)` with `&app`, `call_and_read_body`/`read_body` → `Bytes`, `#[actix_web::test]`). Import as `actix_web::test as atest` to avoid shadowing `#[test]`.
- **D-17:** The two PATCH tests stay but are `#[ignore = "re-enabled in Phase 3 (JS runtime port)"]`; their JS uses the new `Deno.core.ops.op_rs3_list()` syntax already. All other tests must pass; `thread::sleep` removal/full isolation is Phase 2, but no new sleeps are added.
- **D-18:** New unit tests for `store` (get/insert/remove/not-found/sorted list/insert_many all-or-nothing) and `config` (dotted keys parsed from an in-code source; missing/invalid port → error).

### Lint policy
- **D-19:** `[lints]` in `Cargo.toml`: `rust.unsafe_code = "forbid"`; clippy at default + `-D warnings` in the gate. `clippy::unwrap_used` is NOT enabled yet (handlers still have unwraps) — it is added in Phase 2/3 once they're gone. `cargo clippy --all-targets -- -D warnings` must pass.

### Commit sequencing
- **D-20:** Suggested commit order: (1) toolchain file swap + manifest + lock regen, (2) module restructure/ports, (3) store & config with tests, (4) a standalone `cargo fmt` commit if formatting churn is large. Every commit on `modernize-toolchain`.

### Claude's Discretion
- Exact file layout inside modules (`store/mod.rs` vs `store.rs`), helper names, error message wording, whether `js` stub is `js/mod.rs` with a single `init_platform()` placeholder.
- Whether `JsRuntime::init_platform` is already called in `main` in Phase 1 (allowed but not required).

</decisions>

<canonical_refs>
## Canonical References

**Downstream agents MUST read these before planning or implementing.**

### Project scope
- `MODERNIZATION_BRIEF.md` — original milestone brief (requirements 1–10, out-of-scope dirs, future direction)
- `.planning/PROJECT.md` — core value, constraints, key decisions
- `.planning/REQUIREMENTS.md` — TOOL-01..03, DEPS-01..04, ARCH-01, ARCH-04 (this phase)
- `.planning/ROADMAP.md` §Phase 1 — success criteria and notes

### Research
- `.planning/research/SUMMARY.md` — resolved decisions (a)–(i), phase implications
- `.planning/research/STACK.md` — exact versions, `Cargo.toml`, API cheat-sheet (config builder, actix test utils), what NOT to use
- `.planning/research/PITFALLS.md` — legacy toolchain precedence, deno_error pin, duplicate v8, resolver/rust-version, edition-2024 `set_var` unsafe
- `.planning/research/ARCHITECTURE.md` — module layout (`store/`, `config.rs`, `http/`, `js/`), dependency direction

</canonical_refs>

<code_context>
## Existing Code Insights

### Reusable Assets
- `src/bindings/mod.rs` `list/insert/delete/get`: logic to move into `store` + `store/legacy.rs` (minus the global and unwraps inside the store).
- `src/services/mod.rs`: route definitions and `app_service_config` → becomes `http::configure`.
- `src/config/mod.rs`: `ServerConfig`/`Config` serde structs → `config::Settings` with the new builder API.
- `src/main.rs` tests: port to actix 4.17 into the `http` test module.

### Established Patterns
- Responses are JSON-encoded strings via `HttpResponse::Ok().json(String)` — preserve (golden bodies asserted in Phase 2).
- Routes declared with actix attribute macros (`#[get("/key/{key}")]` etc.) — keep.

### Integration Points
- `main.rs` wires `Settings::load()` → `HttpServer::new(|| App::new().configure(http::configure))`.
- `js` stub is the seam Phase 3 fills (`op2` ops + executor); `http` PATCH handler is the call site.
- `requests/`, `src/example.js` JS migration is Phase 3 (not this phase).

</code_context>

<specifics>
## Specific Ideas

- Keep `target/` inside the project dir — `/tmp` (root fs) is ~96% full; `/home` has ~51 GB.
- First build downloads the V8 prebuilt (~35 MB) and needs network; expect a slow first link.
- Git commits need an author: the environment has no global git identity; use `GIT_AUTHOR_NAME/EMAIL` + `GIT_COMMITTER_NAME/EMAIL` = `Johannes Maria Frank <jmfrank63@gmail.com>` env vars (do not modify git config).
- rustup will auto-install 1.99.1 on first cargo invocation (host has 1.99.0).

</specifics>

<deferred>
## Deferred Ideas

- Config defaults and `SERVER__HOST` alias (OPS-02, v2).
- `clippy::unwrap_used` lint — Phase 2/3 once handler unwraps are gone.
- `StoreBackend` trait for git-backed storage — FUT-01.

</deferred>

---

*Phase: 01-toolchain-dependencies-foundation*
*Context gathered: 2026-10-20*
