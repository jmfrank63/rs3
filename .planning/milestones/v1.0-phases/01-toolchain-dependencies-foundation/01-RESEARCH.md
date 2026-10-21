# Phase 1: Toolchain, Dependencies & Foundation - Research

**Researched:** 2026-10-20
**Domain:** Rust toolchain/edition migration, Cargo dependency reset, lib/bin module restructure (papaya store, config 0.15 loader, actix-web 4.17 port, deno_core stub)
**Confidence:** HIGH. Every API, command and failure mode below was checked in a throwaway spike crate. The spike had exactly the Phase 1 dependency set and module layout, and ran on **rustc 1.99.1** (auto-installed from `rust-toolchain.toml`) on this aarch64 host. It was deleted afterwards. The repo is untouched (`git status` is unchanged).

## Summary

The project research (STACK/PITFALLS/ARCHITECTURE) holds up. I rebuilt the planned Phase 1 tree in a spike under `/home/t3code/rs3-spike`, with `rust-toolchain.toml` 1.99.1, edition 2024, the D-04 dependency set, `lib.rs` + thin `main.rs`, papaya `Store`, `config::Settings`, the actix-web 4.17 handlers on a `LazyLock<Store>` bridge, a `js::init_platform()` stub, and an `ExitCode` main. It built cold in about 64 s, including the one-time 37 MB V8 prebuilt download. 16 unit/HTTP tests passed. `cargo clippy --all-targets -- -D warnings` and `cargo fmt --check` (after one `cargo fmt`) were clean. The binary served `/`, `/ip`, `/insert`, `/list` and answered PATCH with 501. Invalid config, an unparsable port, a busy port and an unresolvable host each printed one `error: …` line and exited 1 with no panic.

The spike changes five things for the planner:
1. **Cargo.lock always contains a pre-release package.** `v8x 149.4.0-rc.4` is an optional dependency of `deno_v8`, and Cargo records it even though it is never built. DEPS-01 must be verified against the *active graph* (`cargo tree`), not by grepping the lockfile. This corrects STACK.md's claim that the lock "stays free of pre-releases".
2. **The ported `/ip` tests fail on a clean checkout** if the handler keeps calling `Settings::load().unwrap()`. The error is `missing configuration field "server"`. Injecting `web::Data<Settings>` into `/ip` only (D-15 permits it) fixes this, and the tests then build `Settings` in code.
3. **`#![forbid(unsafe_code)]` is compatible with deno_core.** `#[op2]`, `#[op2(fast)]`, `extension!`, `deno_core::scope!` and `execute_script` all compiled, passed clippy and ran under `forbid`. D-13/D-19 can stay as decided, and Phase 3 is not blocked.
4. **`clippy::uninlined_format_args` is NOT a default lint on clippy 1.99.1.** This corrects PITFALLS #22. The default lints that do fire on the legacy code are `single_char_add_str`, `let_and_return`, `len_without_is_empty`, `new_without_default` and `useless_format`.
5. **Module/file collision.** `src/config.rs` (or a new `src/config/mod.rs`) cannot coexist with the old `src/config/mod.rs` once `lib.rs` declares `pub mod config;` (E0761). Delete or move the old directories in the same plan that declares the new modules.

**Primary recommendation:** Execute in three plans. (1) Toolchain + manifest + lockfile reset, verified by `cargo tree` and lockfile checks with no build. (2) `store` + `config` modules with unit tests, verified by `cargo test --lib`, which does not need the old `main.rs`. (3) `http` port + `js` stub + `ExitCode` main + deletion of `bindings/`/`services/`, then the full four-command gate plus binary smoke tests.

## Architectural Responsibility Map

| Capability | Primary Tier | Secondary Tier | Rationale |
|------------|-------------|----------------|-----------|
| Toolchain selection (1.99.1, rustfmt, clippy) | Build tooling (`rust-toolchain.toml`, rustup) | — | rustup reads the file on every `cargo` call |
| Dependency set / lockfile | Build tooling (`Cargo.toml`, `Cargo.lock`) | — | Resolver 3 (edition 2024) + `rust-version` |
| Key/value storage | Domain library (`rs3::store`) | — | Must not import actix-web or deno_core (ARCH-01, success criterion 4) |
| v0 wire format (insert parse, list format) | Domain library (`rs3::store::legacy`) | HTTP (caller) | Shared by `/list` now and by `op_rs3_list` in Phase 3 (D-08) |
| Configuration loading | Library (`rs3::config`) | Binary (`main` calls `Settings::load`) | The `.env` + env read happens once in `main`; tests use an in-code map |
| HTTP routing / handlers | HTTP adapter (`rs3::http`) | — | actix-web 4.17; temporary `LazyLock<Store>` bridge (D-09) |
| JS runtime seam | JS adapter (`rs3::js`) | Binary (`main` may call `init_platform`) | Phase 1 stub only; makes deno_core a real, linked dependency |
| Startup error handling | Binary (`main.rs`) | — | `ExitCode`, `eprintln!("error: …")`, no panics (ARCH-04) |

## User Constraints (from CONTEXT.md)

<user_constraints>

### Locked Decisions

#### Toolchain & manifest
- **D-01:** `rust-toolchain.toml` = `channel = "1.99.1"`, `components = ["rustfmt", "clippy"]`, `profile = "minimal"`. Delete the legacy `rust-toolchain` file in the same commit (it takes precedence otherwise).
- **D-02:** `Cargo.toml`: `edition = "2024"`, `rust-version = "1.99"`, version bumped to `0.2.0` (signals the modernized line). Single package with both `[lib]` (`src/lib.rs`, crate name `rs3`) and the `rs3` binary — still one crate/package, no workspace.
- **D-03:** Delete the old `Cargo.lock` and regenerate it; never run `cargo fix --edition` (old tree doesn't compile).
- **D-04:** Dependency set from `.planning/research/STACK.md`: `actix-web 4.17` (`default-features = false, features = ["macros"]`), `deno_core 0.412`, `deno_error = "0.7"` (hand-written, not `cargo add`), `papaya 0.2`, `config 0.15` (`default-features = false`), `dotenvy 0.15`, `serde` (derive), `serde_json` (`preserve_order`), `thiserror 2`. No direct `v8`, `rusty_v8`, `tokio`, `actix-rt`, `lazy_static`, `dotenv`, `flurry`, `anyhow`. — **Reversibility:** reversible
- **D-05:** `deno_core` is added in Phase 1 (so the V8 prebuilt download, link and `cargo tree -d` single-V8 check happen now), referenced from a minimal `js` module stub; real ops arrive in Phase 3.

#### Store API & list ordering
- **D-06:** `Store` is a concrete, cheap-`Clone` newtype over `Arc<papaya::HashMap<String, String>>` — no `StoreBackend` trait yet (extract when the git backend arrives). `store` imports nothing from actix-web or deno_core.
- **D-07:** API is synchronous, fallible, and returns owned values (no guards leak): `get(&str) -> Result<String, StoreError>`, `insert(String, String)`, `insert_many(iter)` (validate-then-write, all-or-nothing from the caller's view), `remove(&str) -> Result<String, StoreError>`, `list() -> Vec<(String, String)>` sorted by key, `is_empty()`/`len()`. `StoreError` via `thiserror` with at least `NotFound(key)`.
- **D-08:** `store/legacy.rs` holds the rs3 v0 wire-format helpers (parse insert body → map of strings, format `/list` output) so `/list` and the future `op_rs3_list` share one codec. In Phase 1 the codec keeps the current output shape; the escaping/sort/404 behaviour changes land in Phase 2 with their tests.

#### Phase-1 store access bridge
- **D-09:** Handlers in Phase 1 reach the store through a temporary `static STORE: LazyLock<Store>` in the `http` module (replacing `lazy_static` MAP). Explicitly marked `// TODO(phase-2): inject via web::Data` — Phase 2 removes it. `store` itself contains no global.
- **D-10:** Handlers are ported to actix-web 4.17 with today's success semantics/bodies unchanged; Phase 1 does not change status codes beyond what's needed to compile (`unwrap`s inside handlers may remain until Phase 2 replaces them).
- **D-11:** `PATCH /key/{key}` returns `501 Not Implemented` with a short JSON message until Phase 3.

#### Config shape & failure behaviour
- **D-12:** `config::Settings { server: ServerSettings { host: String, port: u16 } }` (module `config`; rename from `Config` to avoid clashing with the `config` crate). `Settings::load()` = `dotenvy::dotenv().ok()` then `Config::builder().add_source(Environment::default()).build()?.try_deserialize()` — keeps dotted `SERVER.HOST` / `SERVER.PORT`. No defaults added (defaults/`SERVER__HOST` alias deferred).
- **D-13:** A `Settings::from_source(...)` / in-code constructor exists for tests so unit tests never mutate process env (`set_var` is `unsafe` in edition 2024). `#![forbid(unsafe_code)]` crate-wide.
- **D-14:** `main` returns `std::process::ExitCode` (or `Result<(), Box<dyn Error>>`): config load errors and bind errors print a one-line `error: ...` to stderr and exit non-zero — no panics, no `anyhow`.
- **D-15:** `GET /ip` still reports the configured host; in Phase 1 it may keep reading settings the same way it does today (ARCH-03 "load once and inject" is Phase 2).

#### Legacy tests during the transition
- **D-16:** Existing tests move out of `main.rs` into the `http` module's `#[cfg(test)]` and are ported to actix-web 4.17 test APIs (`test::init_service(...)` with `&app`, `call_and_read_body`/`read_body` → `Bytes`, `#[actix_web::test]`). Import as `actix_web::test as atest` to avoid shadowing `#[test]`.
- **D-17:** The two PATCH tests stay but are `#[ignore = "re-enabled in Phase 3 (JS runtime port)"]`; their JS uses the new `Deno.core.ops.op_rs3_list()` syntax already. All other tests must pass; `thread::sleep` removal/full isolation is Phase 2, but no new sleeps are added.
- **D-18:** New unit tests for `store` (get/insert/remove/not-found/sorted list/insert_many all-or-nothing) and `config` (dotted keys parsed from an in-code source; missing/invalid port → error).

#### Lint policy
- **D-19:** `[lints]` in `Cargo.toml`: `rust.unsafe_code = "forbid"`; clippy at default + `-D warnings` in the gate. `clippy::unwrap_used` is NOT enabled yet (handlers still have unwraps) — it is added in Phase 2/3 once they're gone. `cargo clippy --all-targets -- -D warnings` must pass.

#### Commit sequencing
- **D-20:** Suggested commit order: (1) toolchain file swap + manifest + lock regen, (2) module restructure/ports, (3) store & config with tests, (4) a standalone `cargo fmt` commit if formatting churn is large. Every commit on `modernize-toolchain`.

### Claude's Discretion
- Exact file layout inside modules (`store/mod.rs` vs `store.rs`), helper names, error message wording, whether `js` stub is `js/mod.rs` with a single `init_platform()` placeholder.
- Whether `JsRuntime::init_platform` is already called in `main` in Phase 1 (allowed but not required).

### Deferred Ideas (OUT OF SCOPE)
- Config defaults and `SERVER__HOST` alias (OPS-02, v2).
- `clippy::unwrap_used` lint — Phase 2/3 once handler unwraps are gone.
- `StoreBackend` trait for git-backed storage — FUT-01.

</user_constraints>

<phase_requirements>
## Phase Requirements

| ID | Description | Research Support |
|----|-------------|------------------|
| TOOL-01 | `cargo` selects 1.99.1 + rustfmt + clippy via `rust-toolchain.toml`; legacy `rust-toolchain` deleted | rustup 1.29.1 auto-installs the pinned toolchain on the first cargo call (probe output in Environment Availability). The legacy file contains `1.56.0` and must go. |
| TOOL-02 | `edition = "2024"`, `rust-version = "1.99"` | Spike manifest (Code Examples §1) compiled on 1.99.1. Resolver 3 then resolves the latest versions. |
| TOOL-03 | build/test/clippy `-D warnings`/fmt `--check` green on a clean checkout | All four were green in the spike. Watch the `/ip` env dependency (Pitfall 1), the `cargo fmt` churn (Pitfall 6) and the default clippy lints (Pitfall 7). |
| DEPS-01 | Released crates.io versions only | Manifest: all released. Lock: the inert `v8x 149.4.0-rc.4` entry (Pitfall 2). Verify against the active graph. |
| DEPS-02 | Fresh `Cargo.lock`, single V8 via deno_core | `cargo tree -d \| grep -cE '^v8 '` = 0, `grep -c '^name = "v8"$' Cargo.lock` = 1, `cargo tree -i v8` gives `v8 v150.4.0 └── deno_v8 v0.4.0 ├── deno_core v0.412.0`. |
| DEPS-03 | `lazy_static`, `dotenv`, `flurry` removed | `LazyLock` (std), `dotenvy`, `papaya` all verified. `once_cell` remains *transitively* (fine; not a direct dependency). |
| DEPS-04 | config builder API; `SERVER.HOST`/`SERVER.PORT` from `.env` or env | `Environment::default()` lowercases keys and keeps dots as nesting. Verified from a real env, from `.env`, and from an in-code map (Code Examples §3). |
| ARCH-01 | lib.rs + thin main.rs; store/config/http/js modules; one-way dependencies | Layout in Recommended Project Structure. `store` imports only `papaya`, `thiserror`, `serde_json` (legacy codec). |
| ARCH-04 | Bad config / bound port → error + non-zero exit, no panic | `ExitCode` main verified: `exit=1` with a one-line `error:` for missing config, invalid port, busy port and an unresolvable host (Validation Architecture). |
</phase_requirements>

## Project Constraints (from CLAUDE.md)

`./CLAUDE.md` does not exist. `./.claude/CLAUDE.md` is gitignored, generated by GSD and mirrors PROJECT.md. Its actionable directives:
- **Toolchain:** latest stable Rust (1.99), edition 2024.
- **Platform:** Linux aarch64. The V8 prebuilt from the `v8` crate is used, and the first build is slow.
- **Compatibility:** same HTTP routes and semantics. Keep response bodies compatible where tests assert them.
- **Dependencies:** released versions only, no git, no beta.
- **Structure:** single binary crate (one package). Modular store / JS runtime / HTTP layers with injected state.
- **Process:** work on branch `modernize-toolchain`. Commit per plan with clear messages. Commits need `GIT_AUTHOR_NAME/EMAIL` + `GIT_COMMITTER_NAME/EMAIL` = `Johannes Maria Frank <jmfrank63@gmail.com>` env vars (CONTEXT specifics; do not modify git config).
- **Out of scope:** `experiments/`, `aws/`, `docker_rs3/`, `Vagrantfile`.

No project skills directory exists (`.claude/skills/` and `.agents/skills/` are absent).

## Standard Stack

### Core
| Library | Version (resolved in spike) | Purpose | Why Standard |
|---------|---------|---------|--------------|
| Rust toolchain | 1.99.1 (`rustc 1.99.1 (58cf8f9c3 2026-10-13)`, `clippy 0.1.99`, `rustfmt 1.10.0-stable`) | Compiler + lints + formatter | Locked D-01 [VERIFIED: rustup probe output] |
| actix-web | 4.17.0, `default-features = false, features = ["macros"]` | HTTP server, `#[actix_web::main]`, `#[actix_web::test]`, `actix_web::test::*` | Locked D-04 [VERIFIED: cargo search + spike build/test] |
| deno_core | 0.412.0 (pulls `deno_v8 0.4.0` → `v8 150.4.0`, `serde_v8 0.321.0`) | JS runtime (stub in Phase 1) | Locked D-04/D-05 [VERIFIED: cargo search + spike] |
| deno_error | `"0.7"` → resolves **0.7.1** (crates.io latest is 0.7.3) | Op error type (Phase 3) | deno_core pins `=0.7.1` [VERIFIED: spike Cargo.lock + cargo search shows 0.7.3] |
| papaya | 0.2.5 (pulls `seize 0.5.1`) | Concurrent map behind `Store` | Locked D-06 [VERIFIED: cargo search + papaya-0.2.5/src/map.rs] |
| config | 0.15.27, `default-features = false` | Builder API, `Environment` source | Locked D-12 [VERIFIED: config-0.15.27/src/env.rs + spike] |
| dotenvy | 0.15.7 | `.env` loading | Locked D-04 [VERIFIED: spike `.env` run] |
| serde | 1.0.229 + `derive` | `Settings` deserialization | [VERIFIED: spike lockfile] |
| serde_json | 1.0.154 + `preserve_order` | Insert body parse/echo, legacy codec | [VERIFIED: cargo search] |
| thiserror | 2.0.21 | `StoreError` | [VERIFIED: cargo search] |

### Supporting
| Library | Version | Purpose | When to Use |
|---------|---------|---------|-------------|
| `std::sync::LazyLock` | std (1.80+) | Temporary `static STORE` bridge (D-09) | Phase 1 only. Phase 2 removes it. |
| `std::process::ExitCode` | std | Non-zero exit without panic | `main` return type (D-14) |

### Alternatives Considered
None. The stack is locked (D-04). Do not add `tokio`, `actix-rt`, `anyhow`, `once_cell` or `v8` directly.

**Installation:** hand-write the manifest (Code Examples §1), then `rm Cargo.lock && cargo generate-lockfile` (or just `cargo build`). **Do not** `cargo add deno_error`: it writes `0.7.3` and resolution fails against deno_core's `=0.7.1` pin [CITED: .planning/research/PITFALLS.md Pitfall 5].

**Version verification (this session):** `cargo search` returned `papaya = "0.2.5"`, `deno_core = "0.412.0"`, `actix-web = "4.17.0"`, `config = "0.15.27"`, `thiserror = "2.0.21"`, `dotenvy = "0.15.7"`, `serde_json = "1.0.154"` and `deno_error = "0.7.3"` (the last is not used; 0.7.1 resolves) [VERIFIED: cargo search, 2026-10-20].

## Package Legitimacy Audit

| Package | Registry | Age | Downloads (weekly, seam) | Source Repo | Verdict | Disposition |
|---------|----------|-----|-----------|-------------|---------|-------------|
| actix-web | crates.io | since 2017 | 927,020 | github.com/actix/actix-web | OK | Approved |
| deno_core | crates.io | since 2019 | 161,386 | github.com/denoland/deno | OK | Approved |
| deno_error | crates.io | since 2024-11 | 167,948 | github.com/denoland/deno_error | OK | Approved |
| papaya | crates.io | since 2021 | 502,859 | github.com/ibraheemdev/papaya | OK | Approved |
| config | crates.io | since 2015 | 1,608,470 | github.com/rust-cli/config-rs | OK | Approved |
| dotenvy | crates.io | since 2022 | 4,386,634 | github.com/allan2/dotenvy | OK | Approved |
| serde | crates.io | — | 28,864,001 | github.com/serde-rs/serde | OK | Approved |
| serde_json | crates.io | — | 28,844,257 | github.com/serde-rs/json | OK | Approved |
| thiserror | crates.io | — | 34,269,502 | github.com/dtolnay/thiserror | OK | Approved |

**Packages removed due to [SLOP] verdict:** none
**Packages flagged as suspicious [SUS]:** none
Crates have no postinstall scripts. The build-time network action is `v8`'s `build.rs` downloading `librusty_v8_simdutf_release_aarch64-unknown-linux-gnu.a.gz` from `github.com/denoland/rusty_v8/releases` [VERIFIED: spike build-script output]. That is expected and documented.

## Architecture Patterns

### System Architecture Diagram

```
 cargo <cmd> ──► rustup reads rust-toolchain.toml ──► rustc/clippy/rustfmt 1.99.1
                                                        │
 Cargo.toml (edition 2024, resolver 3, rust-version 1.99) ──► Cargo.lock v4 (single v8 via deno_v8)
                                                        │
 process start: target/debug/rs3 (main.rs, thin)        ▼
   ├─1─► rs3::config::Settings::load()
   │       dotenvy::dotenv() (never overrides real env) ──► config::Environment::default()
   │       SERVER.HOST → server.host, SERVER.PORT → server.port ──► try_deserialize::<Settings>
   │       Err ──► eprintln!("error: invalid configuration: …") ──► ExitCode::FAILURE
   ├─2─► (optional) rs3::js::init_platform()  (main thread, before workers)
   ├─3─► web::Data::new(settings)  (created OUTSIDE the factory closure)
   ├─4─► HttpServer::new(move || App::new().app_data(settings.clone()).configure(rs3::http::configure))
   │       .bind((host, port)) ── Err (EADDRINUSE / DNS) ──► eprintln!("error: cannot bind …") ──► FAILURE
   └─5─► .run().await ──► Ok ──► ExitCode::SUCCESS (SIGINT → graceful, exit 0)

 request ──► actix worker ──► rs3::http handler
                                ├─ GET /, GET /ip (reads web::Data<Settings>)
                                ├─ POST /insert ─► store::legacy::parse_insert_body ─► STORE.insert_many
                                ├─ GET /list ───► STORE.list() (sorted) ─► store::legacy::format_list
                                ├─ GET/DELETE /key/{k} ─► STORE.get/remove (owned String)
                                └─ PATCH /key/{k} ─► 501 {"error": "..."}
                     STORE = static LazyLock<rs3::store::Store> in http (TODO(phase-2))
                     rs3::store::Store ─► Arc<papaya::HashMap<String,String>> (pin() guards never escape)
```

### Recommended Project Structure
```
rust-toolchain.toml        # new (legacy `rust-toolchain` deleted in same commit)
Cargo.toml                 # rewritten; Cargo.lock regenerated (v4)
src/
├── lib.rs                 # #![forbid(unsafe_code)]; pub mod config; pub mod http; pub mod js; pub mod store;
├── main.rs                # thin: Settings::load → (init_platform) → HttpServer; ExitCode
├── config.rs              # Settings, ServerSettings, load(), from_map() + tests   (old src/config/ DELETED)
├── store/
│   ├── mod.rs             # Store, StoreError + unit tests
│   └── legacy.rs          # parse_insert_body, format_list (current shape)
├── http/
│   └── mod.rs             # configure(), handlers, static STORE bridge, ported tests (#[cfg(test)])
├── js/
│   └── mod.rs             # pub fn init_platform() { JsRuntime::init_platform(None) }
└── example.js             # untouched (Phase 3)
# DELETED: src/bindings/, src/services/, src/config/mod.rs, rust-toolchain, old Cargo.lock
```
`config.rs` vs `config/mod.rs` is the planner's choice (discretion). Exactly one of them may exist (Pitfall 5).

### Pattern 1: Owned-value facade over papaya
**What:** Every method takes a fresh `pin()` guard, clones the value out, and drops the guard before returning. Guards never escape the method.
**API facts** [VERIFIED: papaya-0.2.5/src/map.rs:337-342 `pub fn pin(&self) -> HashMapRef<'_, K, V, S, LocalGuard<'_>>`; :402 `pub fn len(&self) -> usize`; :419 `pub fn is_empty(&self) -> bool`; `HashMapRef` methods `get(&Q) -> Option<&V>`, `insert(K, V) -> Option<&V>` (returns the *replaced* value), `remove(&Q) -> Option<&V>`, `iter() -> Iter<'_, K, V, G>` yielding `(&K, &V)`, `clear()`]:
- `HashMap: Send + Sync` for `String` keys and values (`unsafe impl … Sync for HashMap<K, V, S>` at map.rs:37), so `Arc<papaya::HashMap<String,String>>` is fine in a `static LazyLock` and across actix workers.
- `HashMap` implements `Default` (map.rs:261), so `#[derive(Clone, Default)] struct Store { map: Arc<papaya::HashMap<…>> }` works.
- `len()`/`is_empty()` are available directly on `HashMap` without a guard.
- `pin()` returns a `!Send` `LocalGuard` ref. It is fine inside sync methods. Never hold it across `.await` (use `pin_owned()` if that is ever needed; it is not needed here).

**insert_many all-or-nothing:** collect into a `Vec` *first* (`collect::<Result<Vec<_>, _>>()?`), then write with one `pin()`. A concurrent reader may see a partially applied batch for a few nanoseconds. "All-or-nothing from the caller's view" (D-07) means that validation failure writes nothing, and that holds by construction. True atomic multi-key visibility is not offered by papaya and is not required.

### Pattern 2: Config from env, testable via an injected map
`Environment::source(Some(map))` replaces the process environment with a map [VERIFIED: config-0.15.27/src/env.rs:227 `pub fn source(mut self, source: Option<Map<String, String>>) -> Self`]. `collect()` lowercases every key (env.rs:269 `let mut key = key.to_lowercase();`) and only replaces a separator if one is set (env.rs:293-294). With none set, `SERVER.HOST` becomes the nested path `server.host`. `config::Map` is `std::collections::HashMap` unless config's `preserve_order` feature is on (config-0.15.27/src/map.rs). Type the parameter as `config::Map<String, String>` so the code survives feature unification.

Observed error strings (useful for test assertions) [VERIFIED: spike test output]:
- nothing set → `missing configuration field "server"`
- host only → `missing configuration field "server.port"`
- `SERVER.PORT=abc` → `invalid type: string "abc", expected an integer for key \`server.port\` in the environment`
- `SERVER.PORT=70000` → `invalid type: 64-bit unsigned integer \`70000\`, expected an unsigned 16 bit integer for key \`server.port\``
- `SERVER_HOST`/`SERVER_PORT` (underscore) → *not* mapped, so it is an error (the alias is deferred, OPS-02)

Assert with `.to_string().contains("port")`, not by exact equality. Message wording belongs to the crate.

### Pattern 3: ExitCode main with explicit bind error handling
`HttpServer::bind` resolves and binds immediately and returns `io::Result<Self>` [VERIFIED: actix-web-4.17.0/src/server.rs:503-511]. EADDRINUSE and DNS failures therefore surface at `.bind()`, before `.run()`. `#[actix_web::main] async fn main() -> ExitCode` compiles and works [VERIFIED: spike]. Code Examples §4 has the full code.

### Pattern 4: actix-web 4.17 test port
Signatures [VERIFIED: actix-web-4.17.0/src/test/test_utils.rs:41,93,143,193]: `init_service(app)`, `call_service(app: &S, req)`, `call_and_read_body(app: &S, req: Request) -> Bytes`, `read_body(res: ServiceResponse<B>) -> Bytes`. Compare with `assert_eq!(body, r#""Welcome to rs3""#.as_bytes())` or `web::Bytes::from_static(br#"…"#)`. `use actix_web::{App, test as atest};` keeps the plain `#[test]` usable in the same module [VERIFIED: spike `plain_sync_test_still_works` passed]. `#[actix_web::test]` + `#[ignore = "…"]` works and reports `ignored, re-enabled in Phase 3 (JS runtime port)` [VERIFIED: spike].

### Pattern 5: js stub
```rust
// src/js/mod.rs
use deno_core::JsRuntime;
/// Initialise the V8 platform once per process, before any other thread spawns.
pub fn init_platform() {
    JsRuntime::init_platform(None);
}
```
Signature under default features: `pub fn init_platform(v8_platform: Option<v8::SharedRef<v8::Platform>>)` [VERIFIED: deno_core-0.412.0/runtime/jsruntime.rs:717-719]. Don't enable `unsafe_runtime_options`, which changes it to two arguments. **Linking:** V8 objects are linked into the binary even when nothing references `init_platform`. The spike binary had 56,104 `v8::internal` symbols without the call and 61,383 with it, at 181 MB vs 184 MB debug [VERIFIED: spike `nm`]. Calling it costs nothing extra at link time. **Recommendation:** call `rs3::js::init_platform()` in `main` after `Settings::load()` and before `HttpServer::new`. It exercises the real V8 init path now, on the main thread, as Phase 3 requires. The spike served all routes with it enabled. The `pub fn` alone is enough to keep deno_core in the graph for `cargo tree`.

### Anti-Patterns to Avoid
- **Creating `web::Data<Settings>` (or a future Store) inside the `HttpServer::new` closure:** the closure runs per worker. Create it outside and `.clone()` inside.
- **`use config::…` where the crate's own `config` module is also in scope:** inside `src/config.rs` the bare path resolves to the external crate (it compiled in the spike). In `lib.rs` or any module that `use crate::config;`, write `::config::…` to avoid E0659 ambiguity.
- **Grepping Cargo.lock for pre-releases as the DEPS-01 gate:** it will always fail because of `v8x` (Pitfall 2).
- **Doc comments with fenced Rust code in lib modules:** each doctest is a separate binary that links V8 (about 180 MB debug). Use `text` fences or none.
- **`cargo fix --edition`, `cargo add deno_error`, a direct `v8` dependency:** see PITFALLS 5/6.

## Don't Hand-Roll

| Problem | Don't Build | Use Instead | Why |
|---------|-------------|-------------|-----|
| Lazy global | `lazy_static!`/`once_cell` | `std::sync::LazyLock` | std since 1.80; DEPS-03 |
| Concurrent map | `RwLock<HashMap>` | `papaya::HashMap` | Locked D-06; lock-free reads, no deadlock under future JS re-entrancy |
| Env/.env parsing | manual `std::env::var` + `parse::<u16>` | `config` builder + `dotenvy` | DEPS-04; typed errors with key names |
| JSON parse/echo | string slicing | `serde_json::Value` | Already used. `preserve_order` keeps the echo key order as the input order. |
| Error enum boilerplate | manual `impl Display/Error` | `thiserror` | Locked D-07 |
| Address parsing for bind | `format!("{host}:{port}")` | `(host, port)` tuple → `ToSocketAddrs` | Avoids IPv6 bracket bugs; verified with `.bind(&addr)` |

## Runtime State Inventory

| Category | Items Found | Action Required |
|----------|-------------|------------------|
| Stored data | None. The store is in-memory only, with no persistence. Verified by reading `src/bindings/mod.rs` (only `MAP`). | none |
| Live service config | `.github/workflows/actions-rs.yml` exists and still uses archived actions with `continue-on-error`. It will run against the new tree on push. | None in Phase 1 (Phase 4 replaces it). Expect it to be non-gating or red until then. |
| OS-registered state | rustup toolchains: `1.99.1-aarch64-unknown-linux-gnu` is **now installed** (by this research probe), and `1.56.0`, `1.95.0`, `stable` (1.99.0) also exist. There is no `rustup override` in `~/.rustup/settings.toml` (`[overrides]` empty). | None. Leaving 1.56.0 installed is harmless once the legacy file is deleted. |
| Secrets/env vars | Env names `SERVER.HOST`/`SERVER.PORT` are unchanged (`env.example`: `SERVER.HOST=localhost`, `SERVER.PORT=5000` [VERIFIED: env.example:1-2]). `.env` is gitignored and absent in the repo. No `CARGO_*`/`RUSTUP_*` env vars are set, and there is no `~/.cargo/config.toml`. | none |
| Build artifacts | The old `Cargo.lock` references the deno git repo. There is no `target/` in the repo yet. `~/.cargo/.rusty_v8/` does not exist, so the V8 archive is downloaded into each fresh `target/`. | Delete `Cargo.lock` before any cargo command that resolves (D-03). |

## Common Pitfalls

### Pitfall 1: `/ip` tests need SERVER.* env unless settings are injected
**What goes wrong:** If `status()` keeps `Settings::load().unwrap()` (today's `Config::from_env().unwrap()` [VERIFIED: src/services/mod.rs:28-30 `let rs3_conf = Config::from_env().unwrap();`]), the ported `test_status_is_ok`/`test_ip_body_is_ip` panic on a clean checkout with `called \`Result::unwrap()\` on an \`Err\` value: missing configuration field "server"` [VERIFIED: spike]. The old test also re-reads env for its expectation [VERIFIED: src/main.rs:84 `let rs3_conf = Config::from_env().unwrap();`].
**How to avoid:** Inject `web::Data<Settings>` into `/ip` only (`status(settings: web::Data<Settings>)`). `main` registers `.app_data(settings.clone())`, and the tests use `App::new().app_data(web::Data::new(Settings { server: ServerSettings { host: "test-host".into(), port: 0 } }))`. Verified green in the spike. D-15 says "may keep", so this is allowed, and it pre-delivers part of ARCH-03 for `/ip`.
**Warning sign:** the test passes locally only when a developer `.env` exists.
**Note:** A test app without `.app_data(...)` returns **500** at runtime, not a compile error [VERIFIED: spike `ip_without_app_data_is_500`].

### Pitfall 2: Cargo.lock lists a pre-release (`v8x 149.4.0-rc.4`) that is never built
**What goes wrong:** `grep -E '^version = "[0-9.]+-' Cargo.lock` matches `v8x 149.4.0-rc.4` (with `bindgen 0.70.1` and `temporal_capi`). `deno_v8 0.4.0` declares it as an optional dependency (`[dependencies.v8x_backend] version = "=149.4.0-rc.4" optional = true`, enabled only by the `quickjs` feature), and Cargo.lock includes optional dependencies regardless of features [VERIFIED: deno_v8-0.4.0/Cargo.toml + spike lock].
**Reality:** `cargo tree -e all --target all` contains **zero** pre-release packages, and `cargo tree -i v8x` prints "nothing to print". No `v8x` build directory exists [VERIFIED: spike].
**How to avoid:** Gate DEPS-01 on the manifest and the active graph:
`cargo tree -e all --target all --prefix none | grep -cE ' v[0-9]+\.[0-9]+\.[0-9]+-'` → `0`. Record the lockfile entry as a known, accepted artifact (Open Question 1).

### Pitfall 3: Checking "single V8" with the wrong grep
**What goes wrong:** `cargo tree -d` prints `v8 v150.4.0` as a *parent* of duplicated crates (`bitflags`, `libc` appear twice because of the build/host split), so `cargo tree -d | grep -c 'v8 v'` is non-zero even with one V8.
**How to avoid:** Count only root lines: `cargo tree -d | grep -cE '^v8 '` → `0`. Also `grep -c '^name = "v8"$' Cargo.lock` → `1`, and `cargo tree -i v8 -e normal` must exit 0 (it errors when two versions exist) [VERIFIED: spike].

### Pitfall 4: Intermediate plans can't build the binary
**What goes wrong:** After the manifest swap, the old `main.rs`/`bindings`/`services` reference `flurry`, `lazy_static`, `rusty_v8` and `op_sync`, so `cargo build` fails until the restructure lands.
**How to avoid:** Plan 1 verifies with commands that don't compile the crate: `cargo --version`, `cargo tree …`, `cargo fetch --locked`. Plan 2 verifies with `cargo test --lib` (it compiles only `src/lib.rs`, not the broken `main.rs`). Plan 3 runs the full gate. The old tree never built on any modern toolchain anyway (D-03).

### Pitfall 5: `src/config.rs` and `src/config/mod.rs` coexisting
**What goes wrong:** E0761 "file for module `config` found at both". The old `src/config/mod.rs` is still `mod config;` from the old `main.rs`.
**How to avoid:** Delete `src/config/` (and later `src/bindings/`, `src/services/`) in the plan that adds `lib.rs`'s `pub mod config;`. Or keep the directory form and overwrite `src/config/mod.rs` in place.

### Pitfall 6: rustfmt style_edition 2024 churn
**What goes wrong:** `cargo fmt --check` exits 1 on hand-written code. Typical changes are long `assert_eq!`/builder chains being re-wrapped and `use` lists being version-sorted (uppercase first: `{App, HttpResponse, Responder, delete, get, …}`, `{Store, legacy}`, `{App, test as atest}`) [VERIFIED: spike fmt-check before = 1, after `cargo fmt` = 0].
**How to avoid:** Every plan's last step runs `cargo fmt`, then `cargo fmt --check`. If the churn is large, commit it separately (D-20 step 4).

### Pitfall 7: Default clippy lints that fire on code copied from the legacy tree
Clippy 1.99.1 defaults fire on [VERIFIED: spike lint probe]: `single_char_add_str` (`push_str("[")`), `let_and_return` (old `delete`), `len_without_is_empty` (a pub `len` without `is_empty`), `new_without_default` (a pub `new()` without `Default`, so derive `Default` on `Store`), and `useless_format` (`format!("{}", x)`). **Not** default: `uninlined_format_args` (fires only with `-W`), `unwrap_used`. Handler `unwrap()`s pass the gate (D-19). With `#[op2]`/`extension!`, clippy was also clean.

### Pitfall 8: Edition 2024 items relevant to this code
- `std::env::set_var` is `unsafe`. With `unsafe_code = "forbid"` any use is a hard error. Tests must use `Settings::from_map` (no env).
- `gen` is a reserved keyword. Don't name a helper `gen`.
- Tail-expression temporaries / `if let` rescope: none hit in the spike. Store methods return owned values from the guard expression directly (`self.map.pin().get(key).cloned()`), which compiles.
- RPIT capture: not used. `list()` returns `Vec`, not `impl Iterator`.
- `forbid(unsafe_code)` + deno_core macros: **compatible** (Summary point 3). deno_ops emits `unsafe { … }` in generated code (deno_ops-0.288.0/op2/dispatch_slow.rs:187,219,226…), but the lint did not fire [VERIFIED: spike build + clippy + passing JS test under `#![forbid(unsafe_code)]` and `[lints.rust] unsafe_code = "forbid"`].
- Merged doctests: any doc-code block links V8. Avoid them.

### Pitfall 9: Shared `STORE` in parallel HTTP tests (Phase 1 only)
Tests share the `LazyLock` store. Use **distinct keys per test** (`get_test`, `delete_test`, `insert_test`) and never call `clear()` from a non-ignored test. The `get` handler's legacy "store empty → `[]`" branch (`src/bindings/mod.rs:44-45` `if MAP.is_empty() { "[]".to_string()`) stays in Phase 1 (D-10). Insert before you GET. The ignored PATCH tests may keep or drop their old `thread::sleep`/`clear`. Dropping them is simplest, and D-17 forbids only *new* sleeps.

## Code Examples

All examples below compiled, were fmt-clean, and passed clippy `-D warnings` and tests on rustc 1.99.1 in the spike.

### §1 rust-toolchain.toml + Cargo.toml
```toml
# rust-toolchain.toml
[toolchain]
channel = "1.99.1"
components = ["rustfmt", "clippy"]
profile = "minimal"
```
```toml
[package]
name = "rs3"
version = "0.2.0"
authors = ["johannes"]
edition = "2024"
rust-version = "1.99"

# [lib]/[[bin]] are auto-discovered from src/lib.rs + src/main.rs; explicit sections are optional.

[dependencies]
actix-web  = { version = "4.17.0", default-features = false, features = ["macros"] }
deno_core  = "0.412.0"
deno_error = "0.7"           # MUST stay "0.7" (deno_core pins =0.7.1). Never `cargo add` it.
papaya     = "0.2.5"
config     = { version = "0.15.27", default-features = false }
dotenvy    = "0.15.7"
serde      = { version = "1.0.229", features = ["derive"] }
serde_json = { version = "1.0.154", features = ["preserve_order"] }
thiserror  = "2.0.21"

[lints.rust]
unsafe_code = "forbid"
```
(`authors` is optional. The current value is `authors = ["johannes"]` [VERIFIED: Cargo.toml:4].)

### §2 Store (src/store/mod.rs, core)
```rust
use std::sync::Arc;

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum StoreError {
    #[error("key not found: {0}")]
    NotFound(String),
    #[error("invalid input: {0}")]
    InvalidInput(String),
}

#[derive(Clone, Default, Debug)]
pub struct Store {
    map: Arc<papaya::HashMap<String, String>>,
}

impl Store {
    pub fn new() -> Self { Self::default() }

    pub fn get(&self, key: &str) -> Result<String, StoreError> {
        self.map.pin().get(key).cloned().ok_or_else(|| StoreError::NotFound(key.to_owned()))
    }

    /// Returns the previous value, if any.
    pub fn insert(&self, key: String, value: String) -> Option<String> {
        self.map.pin().insert(key, value).cloned()
    }

    /// Validate everything first, then write (nothing written on Err).
    pub fn insert_many<I>(&self, entries: I) -> Result<usize, StoreError>
    where
        I: IntoIterator<Item = Result<(String, String), StoreError>>,
    {
        let validated: Vec<(String, String)> = entries.into_iter().collect::<Result<_, _>>()?;
        let n = validated.len();
        let map = self.map.pin();
        for (k, v) in validated {
            map.insert(k, v);
        }
        Ok(n)
    }

    pub fn remove(&self, key: &str) -> Result<String, StoreError> {
        self.map.pin().remove(key).cloned().ok_or_else(|| StoreError::NotFound(key.to_owned()))
    }

    /// Snapshot sorted by key.
    pub fn list(&self) -> Vec<(String, String)> {
        let map = self.map.pin();
        let mut out: Vec<(String, String)> = map.iter().map(|(k, v)| (k.clone(), v.clone())).collect();
        out.sort_unstable_by(|a, b| a.0.cmp(&b.0));
        out
    }

    pub fn len(&self) -> usize { self.map.len() }
    pub fn is_empty(&self) -> bool { self.map.is_empty() }
}
```
`insert_many` can take either `Result` items (as here, so `legacy::parse_insert_body` errors flow through) or a plain `Vec<(String, String)>` validated beforehand. Both satisfy D-07. Spike tests that passed: `get_missing_is_not_found`, `insert_get_remove`, `list_sorted`, `insert_many_all_or_nothing`, `clones_share_state`.

**Legacy codec, current shape:** The old `list()` builds `[{"k" : "v"},…]` [VERIFIED: src/bindings/mod.rs:14 `out.push_str(format!("{{\"{}\" : \"{}\"}}", k, v).as_str());`], and the handler then applies `out.replace(",]", "]")` [VERIFIED: src/services/mod.rs:42]. `format!("[{}]", items.join(","))` with each item `format!("{{\"{k}\" : \"{v}\"}}")` is byte-identical except when a key or value contains `,]`. For strict parity, keep the `.replace(",]", "]")` in the handler (or the codec) until Phase 2 rewrites `/list`. The only visible change in Phase 1 is that the order becomes sorted, where it used to be arbitrary.

### §3 Settings (src/config.rs)
```rust
use config::{Config, ConfigError, Environment};
use serde::Deserialize;

#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
pub struct ServerSettings { pub host: String, pub port: u16 }

#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
pub struct Settings { pub server: ServerSettings }

impl Settings {
    /// Load `.env` (if present; never overrides real env vars) then read the process environment.
    pub fn load() -> Result<Self, ConfigError> {
        dotenvy::dotenv().ok();
        Self::from_env_source(Environment::default())
    }

    /// Build from an explicit map instead of the process environment (tests; no set_var).
    pub fn from_map(vars: config::Map<String, String>) -> Result<Self, ConfigError> {
        Self::from_env_source(Environment::default().source(Some(vars)))
    }

    fn from_env_source(env: Environment) -> Result<Self, ConfigError> {
        Config::builder().add_source(env).build()?.try_deserialize()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn vars(pairs: &[(&str, &str)]) -> config::Map<String, String> {
        pairs.iter().map(|(k, v)| ((*k).to_owned(), (*v).to_owned())).collect()
    }
    #[test]
    fn dotted_keys_parse() {
        let s = Settings::from_map(vars(&[("SERVER.HOST", "127.0.0.1"), ("SERVER.PORT", "5000")])).unwrap();
        assert_eq!(s.server, ServerSettings { host: "127.0.0.1".into(), port: 5000 });
    }
    #[test]
    fn missing_port_is_error() {
        let e = Settings::from_map(vars(&[("SERVER.HOST", "h")])).unwrap_err();
        assert!(e.to_string().contains("port"));
    }
    #[test]
    fn invalid_port_is_error() {
        assert!(Settings::from_map(vars(&[("SERVER.HOST", "h"), ("SERVER.PORT", "abc")])).is_err());
        assert!(Settings::from_map(vars(&[("SERVER.HOST", "h"), ("SERVER.PORT", "70000")])).is_err());
    }
}
```
Precedence: `dotenvy::dotenv()` does not overwrite variables already set in the process. A real `SERVER.PORT=99999` beat a `.env` with `18455` [VERIFIED: spike]. D-13's name `from_source` is fine too. `from_map` is a suggested name only.

### §4 main.rs (thin, ExitCode)
```rust
use std::process::ExitCode;

use actix_web::{App, HttpServer, web};
use rs3::config::Settings;

#[actix_web::main]
async fn main() -> ExitCode {
    let settings = match Settings::load() {
        Ok(s) => s,
        Err(e) => {
            eprintln!("error: invalid configuration: {e}");
            return ExitCode::FAILURE;
        }
    };
    rs3::js::init_platform(); // main thread, before actix spawns workers
    let addr = (settings.server.host.clone(), settings.server.port);
    println!(
        "Starting Http server at host address: {}, with port: {}!",
        settings.server.host, settings.server.port
    );
    let settings = web::Data::new(settings); // OUTSIDE the factory closure
    let server = match HttpServer::new(move || {
        App::new().app_data(settings.clone()).configure(rs3::http::configure)
    })
    .bind(&addr)
    {
        Ok(s) => s,
        Err(e) => {
            eprintln!("error: cannot bind {}:{}: {e}", addr.0, addr.1);
            return ExitCode::FAILURE;
        }
    };
    match server.run().await {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("error: server failed: {e}");
            ExitCode::FAILURE
        }
    }
}
```

### §5 http module skeleton + ported tests
```rust
use std::sync::LazyLock;
use actix_web::{HttpResponse, Responder, delete, get, patch, post, web};
use crate::config::Settings;
use crate::store::{Store, legacy};

// TODO(phase-2): inject via web::Data
static STORE: LazyLock<Store> = LazyLock::new(Store::new);

pub fn configure(cfg: &mut web::ServiceConfig) {
    cfg.service(index).service(status).service(insert).service(delete_key)
        .service(get_key).service(patch_key).service(list);
}

#[get("/ip")]
pub async fn status(settings: web::Data<Settings>) -> impl Responder {
    HttpResponse::Ok().json("{'IP' : '".to_string() + settings.server.host.as_str() + "'}")
}

#[patch("/key/{key}")]
pub async fn patch_key(_path: web::Path<String>) -> impl Responder {
    HttpResponse::NotImplemented()
        .json(serde_json::json!({"error": "PATCH is not implemented yet (Phase 3)"}))
}

#[cfg(test)]
mod tests {
    use super::*;
    use actix_web::http::StatusCode;
    use actix_web::{App, test as atest};

    #[actix_web::test]
    async fn test_index_body_is_welcome() {
        let app = atest::init_service(App::new().service(index)).await;
        let req = atest::TestRequest::default()
            .insert_header(("content-type", "text/plain"))
            .to_request();
        let resp = atest::call_service(&app, req).await;
        assert_eq!(resp.status(), StatusCode::OK);
        assert_eq!(atest::read_body(resp).await, r#""Welcome to rs3""#.as_bytes());
    }

    #[actix_web::test]
    async fn test_insert_map_entry_is_ok() {
        let app = atest::init_service(App::new().service(insert)).await;
        let req = atest::TestRequest::post()
            .insert_header(("Content-Type", "application/json"))
            .set_payload(r#"{"insert_test":"John Doe"}"#)
            .uri("/insert")
            .to_request();
        let body = atest::call_and_read_body(&app, req).await;
        assert_eq!(body, web::Bytes::from_static(br#""{\"insert_test\":\"John Doe\"}""#));
    }

    #[actix_web::test]
    #[ignore = "re-enabled in Phase 3 (JS runtime port)"]
    async fn test_patch_body_is_listing() { /* uses Deno.core.ops.op_rs3_list() */ }
}
```
Handler renames (`delete`→`delete_key` etc.) are optional. The old names shadow the actix attribute macros imported in the same module only if both are `use`d. The spike used `delete_key`/`get_key`/`patch_key` to avoid confusion.

## State of the Art

| Old Approach (in repo) | Current Approach | When Changed | Impact |
|--------------|------------------|--------------|--------|
| `rust-toolchain` plain file `1.56.0` [VERIFIED: rust-toolchain:1] | `rust-toolchain.toml` with components/profile | rustup ≥1.23 | Legacy file wins if both exist |
| `edition = "2018"` [VERIFIED: Cargo.toml:5] | `edition = "2024"` (resolver 3, MSRV-aware) | Rust 1.85 | `set_var` unsafe, rustfmt style 2024 |
| `lazy_static!` | `std::sync::LazyLock` | Rust 1.80 | No dependency |
| `config::Config::new()` + `merge` + `try_into` | `Config::builder().add_source().build()?.try_deserialize()` | config 0.13+ | Rewrite loader |
| `dotenv` | `dotenvy` | dotenv unmaintained (RUSTSEC-2021-0141) | Drop-in `dotenvy::dotenv()` |
| `#[actix_rt::test]`, `body::Body`, `&mut app` | `#[actix_web::test]`, `Bytes`, `&app` | actix-web 4.0 GA | Test rewrite |
| `deno_core` from git + `rusty_v8 = "0.27.0"` [VERIFIED: Cargo.toml:18-19] | `deno_core = "0.412.0"`, V8 only via `deno_core::v8` | — | No direct v8 dependency |
| rustup auto-install | Still on by default in rustup 1.29.1, with a warning and `RUSTUP_AUTO_INSTALL=0` opt-out | rustup 1.28.x churn | First cargo call installs 1.99.1 in about 7 s |

## Assumptions Log

| # | Claim | Section | Risk if Wrong |
|---|-------|---------|---------------|
| A1 | The user accepts that DEPS-01 ("no pre-release") is satisfied by the manifest + active build graph, even though `Cargo.lock` contains the never-built optional `v8x 149.4.0-rc.4` entry | Pitfall 2 / Open Q1 | If the user wants a literal "no pre-release string in Cargo.lock", that is impossible with deno_core 0.412 (it requires dropping deno_core). Needs user confirmation. [ASSUMED] |
| A2 | Injecting `web::Data<Settings>` into `/ip` in Phase 1 is acceptable under D-15's "may keep" wording | Pitfall 1 | If rejected, the `/ip` tests must be `#[ignore]`d or need an env, and TOOL-03 "clean checkout" fails. [ASSUMED] |
| A3 | Keeping the handler's `.replace(",]", "]")` for byte parity is wanted until Phase 2 | Code Examples §2 | Low. It only affects values containing `,]`. [ASSUMED] |
| A4 | CI on x86_64 behaves like this aarch64 host for Phase 1 (no JS execution yet; `init_platform` in `main` is not exercised by tests) | Pattern 5 | Low in Phase 1. The PKU risk applies when tests create runtimes (Phase 3/4). [ASSUMED] |

## Open Questions (RESOLVED)

1. **Lockfile pre-release entry (`v8x`)**: RESOLVED by CONTEXT.md D-21. DEPS-01 is verified on the manifest plus the active graph (`cargo tree -e all --target all` shows zero pre-releases), and the inert `v8x` lock entry is recorded in the phase SUMMARY (plan 01-01).
   - What we know: It is an optional dependency of `deno_v8 0.4.0`, enabled only by `quickjs`. It is recorded in Cargo.lock but absent from every `cargo tree` view, and never compiled or downloaded.
   - What's unclear: whether the user reads DEPS-01 as applying to Cargo.lock text.
   - Recommendation: Define the DEPS-01 check as manifest + `cargo tree -e all --target all` (zero pre-releases). Record the `v8x` lock entry in PROJECT.md Key Decisions or the phase SUMMARY as an accepted artifact.
2. **Calling `init_platform()` in `main` now**: RESOLVED by CONTEXT.md D-23. `main` calls `rs3::js::init_platform()` on the main thread before `HttpServer` starts (plan 01-01).
   - What we know: It works, it is on the main thread before workers start, and it adds no link cost.
   - Recommendation: Call it (discretion item). It makes the V8 init path real in Phase 1, and `cargo run` smoke covers it.
3. **The existing `.github/workflows/actions-rs.yml` will run on push with the new tree**: RESOLVED by CONTEXT.md D-24. The workflow is left untouched in Phase 1 and noted in the SUMMARY; Phase 4 replaces it.
   - Recommendation: Ignore in Phase 1 (Phase 4 replaces it). Note it in the phase SUMMARY.

## Environment Availability

| Dependency | Required By | Available | Version | Fallback |
|------------|------------|-----------|---------|----------|
| rustup | TOOL-01 auto-install | ✓ | 1.29.1 | — |
| Rust 1.99.1 + rustfmt + clippy | TOOL-01/03 | ✓ (auto-installed during this research) | rustc 1.99.1, clippy 0.1.99, rustfmt 1.10.0 | — |
| Network to github.com (V8 prebuilt, 37.6 MB) | first build of `v8` | ✓ (downloaded in the spike) | v150.4.0 simdutf aarch64 | `RUSTY_V8_ARCHIVE=/path/to/.a.gz` |
| Network to crates.io | lock regen / fetch | ✓ | — | — |
| curl / python3 (used by v8 build.rs downloader) | V8 download | ✓ | /usr/bin/curl, /usr/bin/python3 | — |
| cc (linker) | linking | ✓ | gcc 13.3.0 | — |
| Disk under /home | `target/` (~2.1 GB debug after build+test+clippy) | ✓ | ~50 GB free | — |
| Disk under /tmp (root fs) | must NOT be used | ✗ (96% full, 2.1 GB free) | — | Keep the default `./target`. No `CARGO_TARGET_DIR` is set. |
| python3 / nc / ss / timeout | smoke tests (busy port) | ✓ | — | — |
| cargo-audit / cargo-deny | optional advisory scan | ✗ | — | Skip (OPS-03 is v2) |

Rustup auto-install probe (verbatim):
```
info: downloading 5 components
warn: the missing active toolchain `1.99.1-aarch64-unknown-linux-gnu` has been auto-installed
warn: this might cause rustup commands to take longer time to finish than expected
info: you may opt out with `RUSTUP_AUTO_INSTALL=0` or `rustup set auto-install disable`
cargo 1.99.1 (5f94df478 2026-08-27)
```
Inside the repo today `cargo --version` reports `cargo 1.56.0`, because the legacy `rust-toolchain` file is active.

Timings (spike, aarch64): cold `cargo build` 64 s wall (including the V8 download). Incremental `cargo test` 7 s. `cargo clippy --all-targets` 22 s after build. The V8 archive is downloaded once per `target/` (build/test/clippy share it) and is not cached in `~/.cargo/.rusty_v8` by default.

**Missing dependencies with no fallback:** none.

## Validation Architecture

### Test Framework
| Property | Value |
|----------|-------|
| Framework | libtest (`#[test]`) + `#[actix_web::test]` (actix-web 4.17 `macros` feature) |
| Config file | none (`Cargo.toml` `[lints]` only) |
| Quick run command | `cargo test --lib` |
| Full suite command | `cargo build && cargo test && cargo clippy --all-targets -- -D warnings && cargo fmt --check` |

### Phase Requirements → Test Map
| Req ID | Behavior | Test Type | Automated Command (pass condition) | Failure signal |
|--------|----------|-----------|-------------------|-------------|
| TOOL-01 | Toolchain pinned | smoke | `test ! -e rust-toolchain && cargo --version \| grep -q '^cargo 1.99.1' && rustc --version \| grep -q '^rustc 1.99.1' && cargo clippy --version && cargo fmt --version` | prints `1.56.0`, or "component unavailable" |
| TOOL-02 | Manifest edition/MSRV | static | `grep -qx 'edition = "2024"' Cargo.toml && grep -qx 'rust-version = "1.99"' Cargo.toml` | grep exit 1 |
| TOOL-03 | Four gates | build | `cargo build && cargo test && cargo clippy --all-targets -- -D warnings && cargo fmt --check` | any non-zero exit |
| DEPS-01 | Released only | static | `! grep -nE 'git *=\|path *=\|branch *=\|-(alpha\|beta\|rc)' Cargo.toml` and `[ "$(cargo tree -e all --target all --prefix none \| grep -cE ' v[0-9]+\.[0-9]+\.[0-9]+-')" = 0 ]` and `[ "$(grep -c 'source = "git+' Cargo.lock)" = 0 ]` | count > 0 |
| DEPS-02 | Single V8 via deno_core | static | `[ "$(cargo tree -d \| grep -cE '^v8 ')" = 0 ] && [ "$(grep -c '^name = "v8"$' Cargo.lock)" = 1 ] && [ "$(grep -c '^name = "rusty_v8"$' Cargo.lock)" = 0 ] && cargo tree -i v8 -e normal \| grep -q 'deno_v8'` and `! grep -qE '^(v8\|rusty_v8) *=' Cargo.toml` | duplicate root, 2 lock entries, `cargo tree -i v8` error "multiple packages" |
| DEPS-02 | Lock regenerated (v4, no deno git) | static | `grep -qx 'version = 4' Cargo.lock && ! grep -q 'github.com/denoland/deno' Cargo.lock` | old lock content |
| DEPS-03 | Removed crates | static | `! grep -qE '^(lazy_static\|dotenv\|flurry\|actix-rt\|tokio\|anyhow) *=' Cargo.toml && ! grep -rnE 'lazy_static\|flurry\|dotenv::\|use dotenv\b' src/` | match found |
| DEPS-04 | Config builder + dotted keys | unit | `cargo test --lib config::` (dotted_keys_parse, missing_port_is_error, invalid_port_is_error) | test FAILED |
| DEPS-04 | Real env + .env | smoke | `env -i PATH=$PATH 'SERVER.HOST=127.0.0.1' 'SERVER.PORT=18453' target/debug/rs3 & sleep 2; curl -sf http://127.0.0.1:18453/ip` → `"{'IP' : '127.0.0.1'}"`; kill | curl error, wrong body |
| ARCH-01 | Module layout / dependency direction | static | `test -f src/lib.rs && test -d src/store && test -e src/js/mod.rs -o -e src/js.rs && ! grep -rnE 'actix_web\|deno_core' src/store/ && ! test -e src/bindings && ! test -e src/services` | grep match / file exists |
| ARCH-01 | Store behaviour | unit | `cargo test --lib store::` (get/insert/remove/not-found/sorted/insert_many all-or-nothing) | test FAILED |
| ARCH-04 | Missing config | smoke | `cd "$(mktemp -d -p /home/t3code)" && env -i PATH=$PATH "$REPO/target/debug/rs3"; echo $?` → `1`, stderr `error: invalid configuration: missing configuration field "server"`. Must run in a dir without `.env`, because dotenvy walks up from the cwd. | exit 0/101, "panicked" in stderr |
| ARCH-04 | Invalid port | smoke | `env -i PATH=$PATH 'SERVER.HOST=127.0.0.1' 'SERVER.PORT=abc' target/debug/rs3; echo $?` → `1` | exit 101 / panic |
| ARCH-04 | Busy port | smoke | `python3 -c "import socket,time;s=socket.socket();s.bind(('127.0.0.1',18451));s.listen();time.sleep(8)" & sleep 1; env -i PATH=$PATH 'SERVER.HOST=127.0.0.1' 'SERVER.PORT=18451' target/debug/rs3 2>err; echo $?` → `1`, and `grep -q 'Address already in use' err && ! grep -q panicked err` | exit 101 / panic text |
| SC-2 | Binary serves, PATCH 501 | smoke | start as in DEPS-04, then `curl -s -o /dev/null -w '%{http_code}' -X PATCH http://127.0.0.1:18453/key/a` → `501`; `GET /` → `"Welcome to rs3"` | other code |

`SC-2` is ROADMAP success criterion 2 (no requirement ID).

Smoke tip: `dotenvy::dotenv()` searches the cwd **and its parents** for `.env`. Run the missing-config smoke from a temp dir under `/home` (not `/tmp`), or confirm that no `.env` exists in the repo root or above it.

### Sampling Rate
- **Per task commit:** `cargo test --lib` (Plan 2+) or the static checks (Plan 1)
- **Per plan merge:** the full four-command gate (Plan 3), or `cargo test --lib && cargo clippy --lib --tests -- -D warnings && cargo fmt --check` (Plan 2, while the old `main.rs` is still broken)
- **Phase gate:** full suite + all smoke rows green before `/gsd-verify-work`

### Wave 0 Gaps
- [ ] `src/store/mod.rs` `#[cfg(test)] mod tests`: covers D-18 store cases / ARCH-01
- [ ] `src/config.rs` `#[cfg(test)] mod tests`: covers DEPS-04 (in-code map, no env)
- [ ] `src/http/mod.rs` `#[cfg(test)] mod tests`: ported legacy tests (D-16), 2 PATCH tests `#[ignore]` (D-17), plus a `patch → 501` test
- [ ] A smoke script is optional. The commands above can be inlined into the plan's verify step. No framework install is needed.

## Security Domain

`security_enforcement: true`, ASVS level 1.

### Applicable ASVS Categories
| ASVS Category | Applies | Standard Control |
|---------------|---------|-----------------|
| V2 Authentication | no | No auth in rs3 (unchanged) |
| V3 Session Management | no | — |
| V4 Access Control | no | — |
| V5 Input Validation | partial | `serde_json` parse + string-only values in `legacy::parse_insert_body`. Handler unwraps (panic → worker restart) remain until Phase 2 (D-10). This is pre-existing and not a regression. |
| V6 Cryptography | no | — |
| V10 Malicious code / V14 Config | yes | `unsafe_code = "forbid"`. Released crates only. `dotenv` (RUSTSEC-2021-0141) removed. `.env` stays gitignored. Config errors print to stderr without dumping the environment (config's messages name the key only). |

### Known Threat Patterns for this stack
| Pattern | STRIDE | Standard Mitigation |
|---------|--------|---------------------|
| Supply chain: git/pre-release dependencies | Tampering | Released crates.io versions + committed `Cargo.lock` (`--locked` in CI, Phase 4) |
| Duplicate/mismatched V8 static libs | Tampering/DoS | Single `v8` via deno_core (DEPS-02 check) |
| `setenv` data race (UB) from `set_var` in threads/tests | DoS | `forbid(unsafe_code)` makes `set_var` uncompilable. Config is read once in `main`. |
| Panic on malformed `/insert` body | DoS | Pre-existing; Phase 2 replaces it with 400/422. Phase 1 must not add new unwraps outside handlers. |
| V8 binary download over the network at build time | Tampering | HTTPS from github.com/denoland releases. `RUSTY_V8_ARCHIVE` is available for pinned or offline builds. |

## Sources

### Primary (HIGH confidence)
- Spike crate built and tested on rustc **1.99.1** in this session (deleted afterwards). It covered build, test, clippy `-D warnings`, fmt, `cargo tree` and lock checks, `nm` linking, live HTTP smoke, exit-code smoke, `.env` precedence, and op2/extension!/scope! under `forbid(unsafe_code)`.
- Crate sources in `~/.cargo/registry`: papaya-0.2.5 `src/map.rs` (pin, len, is_empty, HashMapRef get/insert/remove/iter, Send/Sync, Default). config-0.15.27 `src/env.rs` (source, lowercase, separator), `src/map.rs`, `src/builder.rs`, `Cargo.toml` features. actix-web-4.17.0 `src/test/test_utils.rs`, `src/server.rs` (bind). deno_core-0.412.0 `runtime/jsruntime.rs` (init_platform), `Cargo.toml` features. deno_v8-0.4.0 `Cargo.toml` (v8x optional). deno_ops-0.288.0 `op2/dispatch_slow.rs`.
- `cargo search` (crates.io index) for current versions. `gsd-tools package-legitimacy check` (all OK).
- In-repo files read this session: `Cargo.toml`, `rust-toolchain`, `src/main.rs`, `src/services/mod.rs`, `src/bindings/mod.rs`, `src/config/mod.rs`, `env.example`, `.gitignore`.

### Secondary (MEDIUM confidence)
- `.planning/research/STACK.md`, `PITFALLS.md`, `ARCHITECTURE.md`, `SUMMARY.md` (project research, same date). This research corrects them on two points: v8x in the lock, and the `uninlined_format_args` default.

### Tertiary (LOW confidence)
- None used.

## Metadata

**Confidence breakdown:**
- Standard stack: HIGH. Versions come from `cargo search`, and the exact set was built and tested on 1.99.1.
- Architecture: HIGH. The full Phase 1 layout was compiled and run in the spike.
- Pitfalls: HIGH. Each one was reproduced or ruled out by a probe (the `/ip` panic, the v8x lock entry, the fmt churn, the default lint set, and forbid with the deno macros).

**Research date:** 2026-10-20
**Valid until:** about 2026-11-20 for the toolchain and APIs. deno_core releases weekly, but the committed `Cargo.lock` pins it.
