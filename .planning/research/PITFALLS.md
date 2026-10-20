# Pitfalls Research

**Domain:** Brownfield port of a Rust actix-web server with embedded V8 (`deno_core`) from Rust 1.56/edition 2018 + git/beta deps to Rust 1.99/edition 2024 + released crates (Linux aarch64)
**Researched:** 2026-10-20
**Confidence:** HIGH overall. Most claims below come from reading the source of the crate versions this port will use. Those crates are already in the local cargo registry: `deno_core 0.412.0`, `deno_ops 0.288.0`, `deno_v8 0.4.0`, `v8 150.4.0`, `config 0.15.27`, `dotenvy 0.15.7`, `actix-web 4.17.0`, `dashmap 6.2.1`. Versions were checked against crates.io on 2026-10-20. Docker and CI facts come from the official-images manifest and GitHub changelogs. Each pitfall states its own confidence.

**Phase names used below.** The roadmap does not exist yet, so these are suggested names:
- **P-Toolchain**: rust-toolchain.toml, edition, Cargo.toml, lockfile
- **P-Store**: store layer, `lazy_static`/`flurry` replacement, injected state
- **P-JS**: `deno_core` op2 port
- **P-HTTP**: actix-web 4.x port, error codes, test isolation
- **P-Config**: `config`/`dotenvy`
- **P-CI**: CI pipeline
- **P-Docker**: Dockerfile
- **P-Docs**: Readme, JS samples, op-name mapping

---

## Critical Pitfalls

### Pitfall 1: A panic inside any op aborts the whole server process

**What goes wrong:**
In the current code every store function uses `unwrap()`: `get` and `delete` on a missing key, `insert` with bad JSON or a non-string value. These same functions are the JS ops. In `deno_core 0.412` an `#[op2]` slow-path op is generated as `extern "C" fn` and the generated code has no `catch_unwind` (checked in `deno_ops-0.288.0/op2/dispatch_slow.rs`). Since Rust 1.81 a panic that unwinds out of an `extern "C"` function aborts the process. So one stored script calling `Deno.core.ops.op_rs3_get("missing")` kills the whole actix server, not just the request.

**Why it happens:**
In the old `op_sync` era a panic was "just" a failed request or a dead worker. Developers port the op bodies 1:1 and only fix `unwrap()`s in the HTTP handlers.

**How to avoid:**
- Store functions return `Result<_, StoreError>`.
- Ops return `Result<String, deno_error::JsErrorBox>`, or a type that derives `deno_error::JsError` with `#[class(...)]`. The JS caller then gets a catchable JS exception.
- Write no `unwrap()`/`expect()` in op bodies or anything they call. Enforce it with `#![deny(clippy::unwrap_used)]` on the store and JS modules.

**Warning signs:**
`cargo test` shows "panic in a function that cannot unwind" or a test binary dies with SIGABRT instead of a test failure. The server exits on the first bad PATCH.

**Phase to address:** P-Store (fallible API), then P-JS (op signatures). **Confidence:** HIGH (source-inspected).

---

### Pitfall 2: Nested `op_rs3_patch` trips deno_core's debug-only re-entrancy check (tests abort, release works)

**What goes wrong:**
`op_rs3_patch` calls `patch()`, which builds a *new* `JsRuntime` on the same thread and runs JS that calls more ops (`op_rs3_get`, ...). In debug builds `deno_core` keeps a thread-local `CURRENT_OP`. It panics with `op op_rs3_patch was not marked as #[op2(reentrant)], but re-entrantly invoked op op_rs3_get` (`deno_core-0.412.0/ops.rs`). Because of Pitfall 1 the panic becomes an abort. `cargo test` (debug) crashes while `--release` works, which is a very confusing split.

**How to avoid:**
- Mark the outer op `#[op2(reentrant)]`. `reentrancy_check` returns early for a re-entrant decl and never sets `CURRENT_OP`, so inner ops pass.
- Add an explicit test where a stored script calls `op_rs3_patch` on another key that itself calls `op_rs3_get`.

**Warning signs:**
The nested-patch test aborts only under `cargo test` and not under `cargo run --release`.

**Phase to address:** P-JS. **Confidence:** HIGH (source-inspected).

---

### Pitfall 3: Unbounded recursion and stack exhaustion through nested isolates (process-killing SIGSEGV)

**What goes wrong:**
A value at key `x` containing `Deno.core.ops.op_rs3_patch("x")` recurses without limit. Each level creates another `JsRuntime` (V8 isolate) on the *same native stack*. V8 computes each isolate's stack limit relative to the stack pointer at entry (default about 1 MB budget). Nesting therefore uses up the real thread stack (actix workers and tokio blocking threads default to 2 MiB). The result is a segfault, not a JS `RangeError`. Even a single nesting level combined with deep JS recursion (`fac.js` with a large `n`) can overflow a 2 MiB thread.

**How to avoid:**
- Cap nesting depth with a thread-local or `OpState` counter (for example max 4) and return a JS error past it.
- Run JS on a thread with an explicit larger stack: a dedicated `std::thread::Builder::new().stack_size(16 << 20)` executor, or a tokio runtime built with `thread_stack_size`.
- Isolates must still be dropped in reverse creation order. `v8::OwnedIsolate::drop` asserts "must be dropped in the reverse order of creation". Create and drop nested runtimes strictly inside the op body, and never store them.

**Warning signs:**
SIGSEGV or "stack overflow" with no Rust backtrace when testing recursion. The process dies under the `requests/fac.js` sample with big inputs.

**Phase to address:** P-JS (depth guard, executor thread); verify in P-HTTP tests. **Confidence:** HIGH for the drop-order assertion and the per-isolate stack-limit mechanism. MEDIUM for the exact stack numbers.

---

### Pitfall 4: V8 platform initialised lazily on a worker thread, then PKU segfaults on x86 CI

**What goes wrong:**
`JsRuntime::new` calls `setup::init_v8` inside a `Once` on whichever thread first creates a runtime. Unless the `unsafe_use_unprotected_platform` feature is on, the default platform enforces Memory Protection Keys. Note that `cfg!(test)` in deno_core is false for *your* tests. The `v8` crate docs say all threads using V8 must be descendants of the thread that called `v8::Initialize`.

Two cases break this:
- libtest runs tests on sibling threads, so the first test that touches JS initialises V8 and the other test threads are not its descendants.
- actix workers are siblings of each other.

On PKU-capable x86 hosts (modern Intel Xeon and AMD EPYC, so GitHub `ubuntu-latest` runners) this segfaults intermittently. On the aarch64 dev box (no Intel PKU) it works. So: "works locally, segfaults in CI."

**How to avoid:**
- In `main`, call `JsRuntime::init_platform(None)` **before** `HttpServer::new(...).run()` and before any thread pool is created. With the default features its signature is a single `Option<SharedRef<Platform>>` argument. Under `unsafe_runtime_options` it takes two arguments, so don't enable that feature in production.
- For tests, enable the feature only in dev-dependencies: `[dev-dependencies] deno_core = { version = "0.412", features = ["unsafe_use_unprotected_platform"] }`. With resolver 2/3 the feature does not leak into the release binary.
- Do **not** hand deno_core a plain `v8::new_unprotected_default_platform()`. deno_core's default is a custom platform (`DenoPlatformImpl`) that routes foreground tasks to its event loop, and replacing it can silently change task handling.

**Warning signs:**
Random SIGSEGV in CI only, more often with `--test-threads>1`. It goes away with `--test-threads=1`.

**Phase to address:** P-JS (init in main and test helper); verify in P-CI on an x86 runner. **Confidence:** HIGH for the mechanism (source and docs). MEDIUM on whether a given GitHub runner exposes PKU.

---

### Pitfall 5: deno_core / v8 / deno_error version coupling breaks resolution or creates duplicate V8s

**What goes wrong:**
- `deno_core 0.412.0` now depends on `deno_v8 0.4.0` (renamed to `v8` inside deno_core). That facade re-exports `v8 = 150.4.0`, while crates.io's latest `v8` is **152.2.0**. The `v8` crate has **no `links` key**. A direct `v8 = "152"` dependency, carried over from the old `rusty_v8` line, is therefore *accepted* by cargo. You get two V8 static libs (about 37 MB gz each to download), possible duplicate-symbol link failures, and baffling type errors like "expected `v8::Local<v8::Value>`, found `v8::Local<v8::Value>`".
- `deno_core` pins `deno_error = "=0.7.1"` and does **not** re-export it, while crates.io's latest is 0.7.3. `cargo add deno_error` writes `"0.7.3"` and resolution fails ("failed to select a version for `deno_error`").
- `deno_core` releases about weekly as `0.x`. Every bump can change op2, scope and error APIs (examples: the move from anyhow to `JsErrorBox`, from `HandleScope::new` to the `scope!` macro, and the new `deno_v8` facade).

**How to avoid:**
- Remove `rusty_v8`/`v8` from `[dependencies]` and use `deno_core::v8` and `deno_core::serde_v8` only.
- Declare `deno_error = "0.7"` (not `0.7.3`) so it unifies with `=0.7.1`.
- Pin `deno_core = "0.412"` (caret on 0.x already means `0.412.*`), commit `Cargo.lock`, and upgrade deno_core only deliberately, as one group with deno_error. Group them in Dependabot or Renovate.
- Don't set `default-features = false` on deno_core. `deno_v8` then has neither `v8` nor `quickjs` and fails with `compile_error!`.
- Add a CI check: `cargo tree -d | grep -E '^v8 '` must be empty.

**Warning signs:**
Two `v8-*` directories in `target/*/build/`, two V8 downloads in the build log, or `cargo tree -i v8` showing two versions.

**Phase to address:** P-Toolchain (Cargo.toml), P-JS. **Confidence:** HIGH (crates.io API and Cargo.toml inspected).

---

### Pitfall 6: `cargo fix --edition` cannot be the first step, and the stale lockfile drags in the deno git repo

**What goes wrong:**
- `cargo fix --edition` needs the crate to *compile on the current edition first*. rs3 does not build on any modern toolchain (deno_core git main and actix beta), so the standard "run cargo fix, then bump edition" workflow is impossible.
- The committed `Cargo.lock` still references `deno_core` from git `main`. The first `cargo build` with the old manifest or lock tries to clone the whole denoland/deno repository (multi-GB).
- The legacy `rust-toolchain` file (contents `1.56.0`) **takes precedence over** `rust-toolchain.toml` if both exist (rustup docs). Adding the new file without deleting the old one silently keeps Rust 1.56, which cannot even parse `edition = "2024"` or lockfile v4. Note that `cargo --version` in this repo currently reports 1.56.0.

**How to avoid:**
- Delete `rust-toolchain` *in the same commit* that adds `rust-toolchain.toml`. In that file set `channel = "1.99"`, `components = ["rustfmt", "clippy"]`, `profile = "minimal"`.
- Delete `Cargo.lock` and rewrite `[dependencies]` before running any cargo command.
- Switch to `edition = "2024"` by hand. With ~390 LOC that will mostly be rewritten anyway, do not try a staged 2018→2021→2024 `cargo fix`.
- Set `rust-version = "1.99"`. Edition 2024 enables resolver 3 (MSRV-aware). A lower `rust-version` (for example "1.85") makes cargo *prefer older* dependency versions whose MSRV fits: config 0.15 needs 1.88 and actix-web 4.17 needs 1.88. You would quietly get stale crates, against the "latest released versions" requirement.

**Warning signs:**
`cargo --version` shows 1.56. A build log shows `Updating git repository https://github.com/denoland/deno`. `cargo update` picks versions older than crates.io latest.

**Phase to address:** P-Toolchain (first phase, everything depends on it). **Confidence:** HIGH.

---

### Pitfall 7: Store created inside the `HttpServer::new` factory gives one map per worker

**What goes wrong:**
When the global `lazy_static MAP` is replaced by injected state, a common mistake is writing `HttpServer::new(|| App::new().app_data(web::Data::new(Store::new())))`. The factory runs **once per worker thread**, so each worker gets its own empty map. A POST lands on worker A and a GET on worker B returns 404. Tests use a single `init_service` app and **pass**; only the real multi-worker server is broken.

Related problems:
- Mismatched extractor types (`web::Data<Store>` vs `web::Data<Arc<Store>>`) are not compile errors. They give a runtime 500 "App data is not configured".
- Test apps built with `App::new().service(get)` and no `app_data` also give a runtime 500.

**How to avoid:**
- Build `let store = web::Data::new(Store::new())` **outside** the closure, then `.app_data(store.clone())` inside.
- The JsRuntime gets the same `Arc` via `OpState::put` per runtime, so HTTP and JS see one store.
- Provide one `fn app(store, config) -> App<...>` helper used by both `main` and the tests.
- Add a test that sends `insert` and `get` through two separately initialised services that share one `Data`, and run the binary with `workers(4)` in a smoke test.

**Warning signs:**
Intermittent 404 for keys just inserted when using curl against `cargo run`. Tests are green.

**Phase to address:** P-Store / P-HTTP. **Confidence:** HIGH (well-known actix semantics).

---

### Pitfall 8: dashmap deadlock when JS runs while a map reference is held

**What goes wrong:**
If `flurry` is replaced with `dashmap`, `patch` naturally becomes `if let Some(v) = map.get(&key) { run_js(v.value()) }`. The JS then calls `op_rs3_insert` or `op_rs3_delete`. dashmap shard locks are **not re-entrant** (its docs say: "May deadlock if called when holding any sort of reference into the map"), so the same thread deadlocks on the shard it is still read-holding. The same applies to `list()` iterating while anything inserts. Edition 2024 fixes the `if let ... else` case, because the scrutinee temporary is dropped before `else`. It does **not** fix `match map.get(k) { None => map.insert(..) }` or a guard held in the `then` branch.

flurry and papaya are lock-free and guard-based, so they don't deadlock here. Holding a papaya or flurry guard across a long JS run only delays memory reclamation.

**How to avoid:**
- Store APIs return **owned** `String`s (clone the value, drop the ref) and never expose `Ref`/`Guard` types outside the store module.
- Never call into JS while holding any map reference.
- If staying lock-free is preferred, papaya keeps flurry's semantics with fewer foot-guns here.

**Warning signs:**
A PATCH whose script writes to the store hangs forever (worker pinned, no CPU use).

**Phase to address:** P-Store (API shape) and P-JS (patch implementation). **Confidence:** HIGH.

---

### Pitfall 9: Old JS no longer runs: `Deno.core.ops()` is now an object, `opSync` is gone

**What goes wrong:**
Every sample begins with `Deno.core.ops();`. In current deno_core `Deno.core.ops` is a plain object, so calling it throws `TypeError: Deno.core.ops is not a function` before any op runs. `Deno.core.opSync(...)` is undefined. Old op names (`rs3_list`) become the Rust function names (`op_rs3_list`).

The old code is in more places than the requirement lists:
- `src/main.rs` tests
- `src/example.js` (also uses the old name `op_list`)
- `requests/*.js`
- **`requests/*.txt` and `requests/*.http`**, as JSON-escaped strings inside request bodies

The old list op took a dummy argument (`opSync("rs3_list", "")`). A zero-arg op2 tolerates the extra argument, but an op that takes `#[string] key: String` called with `undefined` throws a TypeError.

`Deno.core.print(msg, isErr)` still exists (`01_core.js`), so `src/example.js` only needs the op calls changed.

**How to avoid:**
- `grep -rn "opSync\|Deno.core.ops()" requests src` must return nothing.
- Document the old→new name table in the Readme.
- Add one test per sample program (`fac`, `sum`, `bin`, `store`), loading the file via `include_str!`.

**Warning signs:**
PATCH returns an error or 500 for every sample. `test_patch_body_is_listing` fails with a JS TypeError.

**Phase to address:** P-JS (tests), P-Docs (samples). **Confidence:** HIGH.

---

### Pitfall 10: Reading the PATCH result through a fresh `v8::Context` with the old scope API

**What goes wrong:**
`bindings::patch` creates `HandleScope::new(isolate)`, then `Context::new` (a *new, empty* context), and reads the `Global` result there. This does not compile any more: v8 ≥ ~140 uses pinned scopes (`v8::scope!`, `PinScope`). Copying the pattern with a new context would also call `ToString` on objects from a different realm.

`execute_script` now takes `name: impl IntoModuleName`. That is `&'static str` or `String`, so `key.as_str()` doesn't compile. It returns `Result<v8::Global<v8::Value>, Box<JsError>>`, not `anyhow`.

**How to avoid:**
```rust
let global = runtime.execute_script("rs3:patch", source)?; // or key.clone()
deno_core::scope!(scope, &mut runtime);
let local = deno_core::v8::Local::new(scope, global);
let out = local.to_rust_string_lossy(scope);
```
Keep the old semantics: a number gives `"120"`, `undefined` gives `"undefined"`, and a `JSON.stringify` result is passed through. Add tests pinning each.

**Warning signs:**
The compiler rejects `HandleScope::new`. Results come back as `""` or `"[object Object]"` unexpectedly.

**Phase to address:** P-JS. **Confidence:** HIGH (signatures and the `scope!` macro read from `jsruntime.rs` and `examples/eval_js_value.rs`).

---

### Pitfall 11: `std::env::set_var` is `unsafe` in edition 2024, and config re-read per request is a data race

**What goes wrong:**
- `Config::from_env()` runs inside the `/ip` handler on **every request**, and it calls `dotenv()`, which calls `env::set_var`. With multiple actix workers and V8 platform threads running, that is an unsynchronised `setenv`/`getenv` race (UB in glibc, the reason edition 2024 made it `unsafe`). `dotenvy 0.15.7` still calls `set_var` internally (`src/iter.rs`), so swapping crates does not remove the hazard.
- Tests that "fix" config with `unsafe { std::env::set_var("SERVER.HOST", ...) }` are racy across libtest threads, and they make CI and local runs depend on process-wide state.
- `test_ip_body_is_ip` currently needs `SERVER.HOST` from the environment or a gitignored `.env`. On a clean checkout without CI env it fails.

**How to avoid:**
- Load `.env` and config **once** at the top of a plain `fn main()`, before the actix `System`, the V8 platform and any thread starts. Use `actix_web::rt::System::new().block_on(...)` instead of `#[actix_web::main]` if ordering needs to be explicit.
- Inject `web::Data<AppConfig>`. Tests construct `AppConfig { server: ServerConfig { host: "test-host".into(), port: 0 } }` directly, so the tests never touch env.
- Add `#![forbid(unsafe_code)]` at the crate root. It catches any `set_var` creep.

**Warning signs:**
Edition 2024 compile error "call to unsafe function `set_var`". Flaky `/ip` test. Rare segfaults under load.

**Phase to address:** P-Config (load-once plus injection) and P-HTTP (tests). **Confidence:** HIGH.

---

### Pitfall 12: v8 prebuilt download fails in slim images, offline builds, or when curl is missing

**What goes wrong:**
`v8 150.4.0`'s `build.rs` downloads `librusty_v8_simdutf_release_aarch64-unknown-linux-gnu.a.gz` (37.6 MB; deno_core turns on `simdutf`) from GitHub releases. It does this for **every** profile and even for `cargo check`/`clippy`, because the build script runs before type-checking. It tries `deno`, then `python3`, then `curl`, and `Command::new("curl").status().unwrap()` *panics* if curl is absent.

The official `rust:1.99-slim` image installs `wget` and **purges it**, and has neither curl nor python3. So `cargo build` in a slim builder fails inside v8's build script. Builds with `--network=none`, behind a proxy, or in a sandboxed CI also fail. The error message suggests `V8_FROM_SOURCE=1`. **Don't**: that is a multi-hour Chromium build that needs depot_tools.

**How to avoid:**
- Use the full `rust:1.99-trixie` image (has curl), or `apt-get install curl` in a slim builder.
- For reproducible or offline builds, pre-fetch the archive once and set `RUSTY_V8_ARCHIVE=/path/librusty_v8_simdutf_release_aarch64-unknown-linux-gnu.a.gz` (a local path is accepted). Another option is to place it at `$CARGO_HOME/.rusty_v8/<URL with non-alphanumerics replaced by '_'>`, which build.rs checks before downloading.
- Never set `V8_FORCE_DEBUG`, since prebuilt debug libs are the only other option and are much larger.
- Musl prebuilts for aarch64 now exist, but stay on `-gnu`. That is what deno_core is tested on.

**Warning signs:**
"Failed to download V8 prebuilt archive", "No such file or directory" from `build.rs`, or the build hanging at `Compiling v8`.

**Phase to address:** P-Docker and P-CI. **Confidence:** HIGH (build.rs inspected; archive existence and size verified with HTTP HEAD).

---

### Pitfall 13: glibc mismatch between the builder and slim runtime images

**What goes wrong:**
`rust:1.99`, `rust:1`, `rust:latest` and `rust:1.99-slim` are now **Debian trixie** (glibc 2.41). The obvious runtime choice `debian:bookworm-slim` has glibc 2.36, so the binary fails at start with `GLIBC_2.38' not found` (or a similar symbol). The current Dockerfile's `ARG DISTRO` has **no default**, so `FROM ${DISTRO}` fails unless the caller passes it. Picking `alpine` for the runtime with a glibc-built binary fails with "not found" from the dynamic loader.

**How to avoid:**
- Pin the builder and runtime to the same Debian codename: `rust:1.99-trixie` with `debian:trixie-slim`, or `gcr.io/distroless/cc-debian13`. Alternatively `rust:1.99-bookworm` with `debian:bookworm-slim`.
- The prebuilt V8 bundles libc++ statically (`use_custom_libcxx`, on by default), so no libstdc++ is needed at runtime. Still check with `ldd /usr/local/bin/rs3` in a test stage.
- Give `ARG DISTRO=debian:trixie-slim` a default, or remove the ARG.

**Warning signs:**
The container exits immediately with a GLIBC or "not found" error, even though `docker build` succeeds.

**Phase to address:** P-Docker. **Confidence:** HIGH (official-images manifest checked 2026-10-20: `1.99`, `1`, `latest` → trixie; current patch is 1.99.1).

---

### Pitfall 14: cargo-chef layering that throws away the V8-heavy dependency cache

**What goes wrong:**
- The current Dockerfile copies only `recipe.json` into the `cacher` stage. Once `rust-toolchain.toml` exists, the cook stage builds with the image's default toolchain and the final stage with the pinned one (or rustup downloads a toolchain in each stage). The dependency cache is invalidated and V8, deno_core and actix are rebuilt.
- Pinning `channel = "1.99.0"` while the image ships 1.99.1 makes rustup download a full extra toolchain in every stage.
- `cargo install --path .` without `--locked` **ignores Cargo.lock** and re-resolves. Inside Docker it can pick a different deno_core or v8 than CI tested.
- There is no `.dockerignore`, so `COPY . .` sends `target/` (several GB once V8 is in it), `.git`, and the developer's `.env` into the build context.

**How to avoid:**
- Use the `lukemathwalker/cargo-chef:latest-rust-1.99-trixie` base image, or install with `cargo install cargo-chef --locked`.
- `COPY rust-toolchain.toml` into every stage that runs cargo.
- Build with `cargo build --release --locked` (or `cargo chef cook --release --locked`) and copy `target/release/rs3` directly.
- Add a `.dockerignore` with `target/`, `.git/`, `.env`, `.planning/`, `.claude/`, `aws/`, `experiments/`.
- Keep the channel equal to the image tag (`1.99`).

**Warning signs:**
Every Docker build re-downloads V8 and recompiles about 400 crates. The build context upload is over 1 GB. `docker run` shows a different `rs3` behaviour than CI.

**Phase to address:** P-Docker. **Confidence:** HIGH.

---

### Pitfall 15: CI that is green only because of `continue-on-error`, then red forever once it is removed

**What goes wrong:**
Removing `continue-on-error: true` exposes everything at once:
- A floating `stable` toolchain brings new clippy lints every 6 weeks, so `-D warnings` breaks on unrelated days.
- `dtolnay/rust-toolchain@stable` installs stable, but `rust-toolchain.toml` overrides it and rustup auto-installs the pinned channel at first `cargo` call. Components listed only in the action and not in the toml (clippy, rustfmt) may then be missing.
- Three separate jobs (check, test, lints) each download V8 and each cache a multi-GB `target/` under separate keys. The GitHub cache defaults to **10 GB per repo, 7-day retention, LRU eviction** (now checked hourly), so caches thrash and evict each other.
- `cargo clippy -- -D warnings` without `--all-targets` doesn't lint `#[cfg(test)]` code, so test-only warnings appear later or never.
- The existing workflow sets env `SERVER.HOST`/`SERVER.PORT`. That works as process env, but once config is injected in tests it should not be needed. Leaving it hides the env dependency.

**How to avoid:**
- Use a single job: `actions/checkout@v5` (v4 minimum), `dtolnay/rust-toolchain@1.99` with `components: rustfmt, clippy`, and `Swatinem/rust-cache@v2` with `cache-on-failure` and a stable `shared-key`.
- Run in sequence: `cargo fmt --all --check`, `cargo clippy --all-targets --locked -- -D warnings`, `cargo test --locked`.
- Set `CARGO_PROFILE_DEV_DEBUG=line-tables-only` (or `0`) to shrink the target dir. Consider `ubuntu-24.04-arm` runners to match the aarch64 platform, and keep one x86 job for the PKU risk in Pitfall 4.
- Drop the `SERVER.*` env from CI once tests inject config. If a test still needs it, it is a bug.

**Warning signs:**
Cache-save step reports multiple GB. Clippy failures on days with no code changes. "component 'clippy' is unavailable".

**Phase to address:** P-CI. **Confidence:** HIGH for the cache limits (GitHub changelog Nov 2025), MEDIUM for the runner/PKU interaction.

---

## Moderate Pitfalls

### Pitfall 16: `config` crate env-var naming: dots, lowercasing, separators and prefixes

**What goes wrong:**
`config 0.15.27` `Environment::collect` **lowercases every key** and, with no separator set, leaves dots in place. `SERVER.HOST` becomes `server.host`, a nested path, which is why the dotted names keep working (MEDIUM-HIGH: verified in `src/env.rs`). The traps:
- Shells can't `export SERVER.HOST=…` ("not a valid identifier"). Docker `-e`, `.env` files (dotenvy accepts `.` in keys) and GitHub `env:` can, so users hit inconsistent behaviour.
- With **no prefix**, *every* process env var is ingested. An unrelated var literally named `SERVER` would replace the `server` table with a string and break deserialisation.
- If `separator("__")` and `prefix("RS3")` are added, the prefix separator defaults to the separator. The var must then be `RS3__SERVER__HOST`, *not* `RS3_SERVER__HOST`.
- The old API (`Config::new()`, `merge`, `try_into`) is gone. Use `Config::builder().add_source(...).build()?.try_deserialize()`.
- String→`u16` coercion for `port` still works.

**How to avoid:**
- Keep backward compatibility with two sources: `Environment::default()` (accepts `SERVER.HOST`), then `Environment::with_prefix("RS3").separator("__")` (accepts `RS3__SERVER__HOST`, shell-friendly, overrides).
- Add `set_default("server.host", "127.0.0.1")` and `set_default("server.port", 8080)` so a missing `.env` doesn't panic at startup.
- Document both forms. Don't touch `docker_rs3/` Ansible env names (out of scope); the compatibility source keeps them working.
- Unit-test config parsing with `config::Environment::default().source(Some(map))`, which takes an injected map instead of the real environment, so no `set_var` is needed.

**Phase to address:** P-Config. **Confidence:** HIGH (source).

---

### Pitfall 17: actix-web 4.x test API: `body::Body` removed, body comparisons rewritten

**What goes wrong:**
`actix_web::body::Body` no longer exists (it is now `BoxBody`/`EitherBody`), and response bodies are not `PartialEq` against a constructed body. All five `assert_eq!(body, Body::from(...))` tests fail to compile. `call_service` now takes `&S`. `&mut app` still compiles via coercion, but clippy flags `unnecessary_mut_passed` under `-D warnings`.

**How to avoid:**
- Use `test::call_and_read_body(&app, req).await`, or `test::read_body(resp).await`, and compare to `Bytes`/`&[u8]`, e.g. `assert_eq!(body, r#""Welcome to rs3""#.as_bytes())`.
- Use `#[actix_web::test]` and drop the direct `actix-rt` dependency, plus `tokio` unless something else needs it.
- Don't add `let mut app`.

**Phase to address:** P-HTTP. **Confidence:** HIGH (`actix-web-4.17.0/src/test/test_utils.rs` signatures).

---

### Pitfall 18: Changing response formats while "fixing" bugs breaks the compatibility promise

**What goes wrong:**
Several behaviours look like bugs but are part of the asserted, observable contract:
- `/insert` returns a **JSON-encoded string** of the JSON (`"{\"insert_test\":\"John Doe\"}"`).
- `/ip` returns the string `"{'IP' : 'host'}"`.
- `/list` returns a JSON string whose content is hand-built pseudo-JSON `[{"k" : "v"},…]`, double-encoded and **unescaped**. Values containing `"`, which every stored JS program has, make the inner text invalid JSON.
- The `/list` handler's `out.replace(",]", "]")` corrupts any value containing `,]`.
- `get` returns `"[]"` (HTTP 200) whenever the map is empty, *regardless of key*, and has a TOCTOU between `is_empty()` and `get().unwrap()`. With per-test fresh stores, the empty-map case becomes the common one in tests: a "missing key → 404" test passes or fails depending on whether anything else is in the store.

Also note: `deno_core` enables `serde_json/preserve_order`. Through feature unification `serde_json::Map` becomes insertion-ordered in rs3 too, so `/insert` echoes keys in input order, not sorted.

**How to avoid:**
- Before porting, write a "golden" characterization test per route that captures the current bytes.
- Then decide per quirk, in PROJECT.md Key Decisions, whether to keep or fix it. Suggested: keep the `/insert` and `/ip` shapes; **fix** `get` on an empty map to return 404 like any missing key; build `/list` with `serde_json` (escaping) and sorted keys, accepting the documented format change since only patch-vs-list equality is asserted; remove the `,]` replace.
- Never assert on multi-key `/list` output ordering. Map iteration order differs between flurry, dashmap and papaya.

**Phase to address:** P-Store (semantics), P-HTTP (golden tests first). **Confidence:** HIGH (read from `src/`).

---

### Pitfall 19: Untrusted JS can hang or OOM the server

**What goes wrong:**
`PATCH` executes stored code synchronously on an actix worker thread. `while(true){}` pins that worker forever, and N such requests (N = number of workers) take down the service. A large allocation reaches V8's heap limit, which is a **fatal** OOM that kills the process. Neither has any timeout today.

**How to avoid:**
- Execute JS on a dedicated thread or pool (see Pitfall 3). Create the `JsRuntime` *inside* that thread, because it is `!Send`.
- A watchdog calls `runtime.v8_isolate().thread_safe_handle().terminate_execution()` after a deadline (for example 5 s), and the request maps to 408/422.
- Set `RuntimeOptions.create_params` with `heap_limits(0, 64 << 20)` and an `add_near_heap_limit_callback` that terminates execution.
- Never add `deno_runtime`, `deno_fs` or `deno_net` extensions. The bare deno_core sandbox (no fs or net ops) is the main security property.

**Phase to address:** P-JS (could be its own sub-plan); the timeout test goes in P-HTTP. **Confidence:** MEDIUM (API names from v8/deno_core knowledge; verify exact `heap_limits` signature in 150.4.0 during planning).

---

### Pitfall 20: `JsRuntime` is `!Send` and pins futures

**What goes wrong:**
`JsRuntime` (and anything holding `Rc<RefCell<OpState>>`) is `!Send`. actix handlers don't require `Send`, so the code compiles. Moving it into `web::block`, `tokio::spawn` or `spawn_blocking` does not. Holding an `OpState` `borrow_mut()` across an `.await`, or across a call that re-enters ops on the *same* isolate, panics with `BorrowMutError`, which is then an abort (Pitfall 1).

**How to avoid:**
- Send only owned `String` source and `Arc<Store>` into the executor, and build the runtime there.
- Ops take `state: &mut OpState` (or `&OpState`) and clone the `Arc<Store>` out immediately.
- Never reuse one runtime across requests:
  - global `let`/`const` bindings persist between `execute_script` calls, so `let numbers = …` in `sum.js` throws `SyntaxError: Identifier 'numbers' has already been declared` on the second run;
  - state leaks between users.

**Phase to address:** P-JS. **Confidence:** HIGH.

---

### Pitfall 21: Edition 2024 semantic changes that bite this codebase specifically

| Change | Where it hits rs3 | Action |
|--------|-------------------|--------|
| `std::env::set_var`/`remove_var` are `unsafe` | config tests, any dotenv-in-test pattern | Don't call them; inject config (Pitfall 11) |
| `if let` scrutinee temporaries dropped before `else` | Makes `RefCell`/dashmap `if let … else` patterns work, but `match` still holds the guard | Don't rely on it; return owned values from the store |
| Tail-expression temporaries dropped before locals | Old `patch` block-tail `res.to_rust_string_lossy(scope)`; scope/guard-heavy tails may now hit "temporary dropped while borrowed" | Bind to a `let` before returning |
| RPIT captures all in-scope lifetimes | Helpers like `fn entries(&self) -> impl Iterator` now capture `&self` (fine); `fn f(s: &str) -> impl Display` returning owned data now keeps `s` borrowed | Add `+ use<>` where the result must outlive args |
| Never-type fallback → `!` | Rare: closures like `Ok::<_, _>(…)` with unconstrained types, `unwrap_or_else(\|\| panic!())` in generic position | Annotate types if `dependency_on_unit_never_type_fallback` warns |
| `unsafe extern` blocks, `#[unsafe(no_mangle)]`, `static mut` refs denied, `gen` reserved | None in rs3 today; op2 macro output uses deno_ops' own 2024 edition | Nothing, but don't name a variable or fn `gen` |
| rustfmt `style_edition = 2024` (version-sorted imports, etc.) | `cargo fmt --check` fails on code formatted by older rustfmt | Run `cargo fmt` once after the edition bump, as its own commit |
| Merged doctests | Only matters if a `lib.rs` is introduced: each doctest would otherwise link V8 | Prefer `no_run` or no doctests in V8-touching modules |

**Phase to address:** P-Toolchain (fmt commit), P-JS/P-Store as encountered. **Confidence:** HIGH (edition guide), MEDIUM on which items actually fire.

---

### Pitfall 22: Clippy `-D warnings` churn on code that is about to be rewritten

**What goes wrong:**
The existing code trips `let_and_return` (`delete`, `main`), `single_char_add_str` (`push_str("[")`), `format_push_string`-style patterns, `unnecessary_mut_passed` (tests), `needless_pass_by_value`, and `uninlined_format_args` (warn-by-default since 1.88). Fixing lints in code that the port replaces is wasted work. Some lints may also fire on `#[op2]` or `extension!` expansions.

**How to avoid:**
- Turn on `-D warnings` only after the port. Use `[lints.clippy]` in Cargo.toml (for example `unwrap_used = "deny"` in non-test code via `#[cfg_attr(not(test), ...)]` or module-level attributes) instead of ad-hoc `#[allow]`.
- If a lint fires in macro-generated op code, put a narrow `#[allow(clippy::xyz)]` on the ops module, not crate-wide.
- Pin the toolchain so new lints arrive only on deliberate bumps.

**Phase to address:** P-Toolchain (lint config), final gate in P-CI. **Confidence:** HIGH.

---

## Minor Pitfalls

### Pitfall 23: op2 attribute and fast-call constraints
- `String`/`&str` args and returns need `#[string]`. Structs and JSON need `#[serde]`. Missing attributes give long macro errors.
- An op that *could* be a fastcall but isn't marked fails to compile with "This op is fast-compatible and should be marked as (fast)" (`deno_ops/op2/mod.rs`). Example: `#[string] &str` in, `bool`/`u32` out, such as a future `op_rs3_contains`. Mark it `#[op2(fast)]`. The five rs3 ops return `String`, so they are not fast.
- The Rust fn name is the JS name (`Deno.core.ops.op_rs3_get`). Renaming a fn silently breaks stored scripts. Keep the mapping table in the Readme.
- Register ops via `extension!(rs3_ext, ops = [...], state = |state| state.put(store.clone()))`, or `options`/`init` with a `Store` parameter. Do not register them via global state. **Confidence:** HIGH.

### Pitfall 24: Per-request `JsRuntime` startup cost
Without a snapshot, each `JsRuntime::new` creates an isolate and evaluates deno_core's built-in JS (`00_primordials.js`, `01_core.js`, ...): a few ms to tens of ms and several MB of RSS per PATCH. Nested patches multiply this. This is fine at MSc scale. Don't "optimise" by reusing runtimes (Pitfall 20). If it ever matters, use `JsRuntimeForSnapshot` plus `startup_snapshot`, which is a later milestone. **Confidence:** MEDIUM (magnitudes unmeasured; measure in P-JS).

### Pitfall 25: Link time and disk with V8 on aarch64
- Every binary that links deno_core embeds the ~150–200 MB uncompressed `librusty_v8.a`. Each `tests/*.rs` integration file is a separate binary, which adds another 10–30 s link and hundreds of MB.
- Keep tests in-crate (`#[cfg(test)]` modules) or in a single `tests/it/main.rs`.
- The aarch64-linux default linker is still `cc`/bfd. Rust's switch to `rust-lld` by default applied to x86_64-linux. Optionally use mold or lld via `.cargo/config.toml` (`-C link-arg=-fuse-ld=mold`), but only if the tool is installed in CI and Docker too, otherwise builds break there.

**Confidence:** MEDIUM.

### Pitfall 26: Out-of-scope directories pinned to old toolchains
`docker_rs3/` (Ansible; its readme runs `cargo run --release`), `aws/` and `Vagrantfile` may provision older Rust. Edition 2024, lockfile v4 and `rust-version = 1.99` make those paths fail with "feature `edition2024` is required" or a lockfile parse error. The milestone leaves them untouched. Record this as a known break in the Readme instead of "fixing" it out of scope. **Confidence:** MEDIUM.

### Pitfall 27: `flurry` is not dead, so the replacement decision should be about API safety
crates.io shows `flurry 0.5.2` (Nov 2024). The case for replacing it is low activity, not abandonment. If the rs3 store API returns owned values (Pitfall 8), any of flurry, papaya or dashmap works. papaya keeps the lock-free, deadlock-free property that the JS-re-entering-store design relies on. **Confidence:** HIGH (crates.io API).

---

## Technical Debt Patterns

| Shortcut | Immediate Benefit | Long-term Cost | When Acceptable |
|----------|-------------------|----------------|-----------------|
| Keep a global `LazyLock<Store>` instead of injected `web::Data` | Minimal diff from `lazy_static` | Tests share state again (sleep-based races), and the future git/VS Code/TS layers can't have separate stores | Never. Isolation is a milestone requirement |
| `.unwrap()` in op bodies "because the handler validated already" | Shorter code | Process abort (Pitfall 1), and JS can call ops directly with any input | Never |
| `continue-on-error` or `allow(warnings)` to get CI green | Fast green | Hides regressions, and the requirement says the build must fail | Never |
| Direct `v8` dependency for "just one helper" | Access to newest v8 API | Duplicate V8 and type mismatches (Pitfall 5) | Never; use `deno_core::v8` |
| `V8_FROM_SOURCE=1` to fix a download problem | Unblocks an offline build | Hours-long builds, Chromium toolchain in CI and Docker | Never for this project; use `RUSTY_V8_ARCHIVE` |
| One cached `JsRuntime` per thread | Faster PATCH | Global-binding collisions, state leaks across requests, drop-order assertions with nested runtimes | Only with a fresh realm per call plus a benchmark proving the need (not this milestone) |
| Keeping the hand-built `/list` JSON | Byte-for-byte compatibility | Invalid JSON for any value with quotes | Only if the user decides to; otherwise use serde with a documented change |

## Integration Gotchas

| Integration | Common Mistake | Correct Approach |
|-------------|----------------|------------------|
| deno_core ↔ store | Passing the store through a global the ops read | `OpState::put(Arc<Store>)` per runtime; op takes `&mut OpState` |
| deno_core ↔ deno_error | `cargo add deno_error` → 0.7.3 conflicts with `=0.7.1` | `deno_error = "0.7"` |
| deno_core ↔ tokio/actix | Creating `JsRuntime` before V8 platform init, or on a non-descendant thread | `JsRuntime::init_platform(None)` in `main` before the server starts |
| actix ↔ store | `web::Data::new(...)` inside the `HttpServer::new` closure | Create once outside, `.clone()` inside |
| config ↔ env | Relying on `SERVER.HOST` only (not shell-exportable) | Dotted keys plus a `RS3__SERVER__HOST` source plus defaults |
| v8 build.rs ↔ Docker | `rust:*-slim` builder with no curl or python | Full `rust:1.99-trixie` builder, or install curl, or `RUSTY_V8_ARCHIVE` |
| Docker builder ↔ runtime | trixie builder and bookworm runtime | Same codename in both stages |
| CI ↔ toolchain file | `@stable` action plus a pinned toml with no components | `dtolnay/rust-toolchain@1.99` with components, matching the toml |

## Performance Traps

| Trap | Symptoms | Prevention | When It Breaks |
|------|----------|------------|----------------|
| JS on actix worker threads | All requests stall during a long PATCH | Dedicated JS executor thread or pool plus terminate watchdog | As soon as one script loops; workers = CPU count |
| New isolate per nested op | Latency grows with nesting depth | Depth cap of about 4 | Recursive or self-referencing scripts |
| dashmap ref held during JS | Hang (deadlock), not slowness | Owned returns; lock-free map | First script that writes to the store from PATCH |
| Multiple test binaries | CI minutes dominated by linking | One test binary | More than about 3 integration test files |
| Separate CI jobs with separate caches | Cache eviction, cold V8 builds | Single job, shared cache key | Repo cache over 10 GB |

## Security Mistakes

| Mistake | Risk | Prevention |
|---------|------|------------|
| Adding deno_runtime/fs/net extensions "for convenience" | Stored JS gets filesystem or network access, i.e. RCE | Bare deno_core only; review every `extension!` op list |
| No CPU time limit | DoS with `while(true){}` | `terminate_execution` watchdog |
| No heap limit | Fatal V8 OOM kills the process | `create_params.heap_limits` plus near-heap-limit callback |
| Unbounded `op_rs3_patch` recursion | Stack overflow SIGSEGV | Depth cap; larger-stack executor thread |
| `.env` copied into the Docker build context | Secrets in build layers | `.dockerignore` |
| `unsafe_use_unprotected_platform` enabled in `[dependencies]` | Weakens V8 memory isolation in production | Only in `[dev-dependencies]` |

## UX Pitfalls

| Pitfall | User Impact | Better Approach |
|---------|-------------|-----------------|
| JS exceptions mapped to bare 500 | Script author can't see what failed | 422 (or 400) with the `JsError` message in the body |
| Missing key → 200 `"[]"` on an empty store, 404 otherwise | Inconsistent client logic | Always 404 for a missing key (documented change) |
| Old samples silently fail | Readme demo broken | Update `.js`, `.txt` and `.http` samples together; table of old→new op names |

## "Looks Done But Isn't" Checklist

- [ ] **Toolchain:** legacy `rust-toolchain` file deleted; `rustc --version` in the repo dir prints 1.99.x
- [ ] **Deps:** `cargo tree -d` shows no duplicate `v8`; no `git =` entries in Cargo.toml or Cargo.lock; `deno_error = "0.7"`
- [ ] **JS port:** nested-patch test passes under **debug** `cargo test` (reentrant flag), not only release
- [ ] **JS port:** each sample `requests/{fac,sum,bin,store}.js` has a passing test; `grep -rn "opSync\|Deno.core.ops()" requests src` is empty, *including `.txt` and `.http`*
- [ ] **Ops:** a test calls `op_rs3_get`/`op_rs3_delete` on a missing key from JS and gets a JS exception, and the test process survives
- [ ] **Recursion:** a self-patching script returns an error instead of crashing
- [ ] **State:** a smoke test with `workers(4)` does insert then get across requests (shared store, not per-worker)
- [ ] **V8 init:** `init_platform` is called in `main` before `HttpServer::run`; tests run green with default `--test-threads` on an x86 runner
- [ ] **Config:** `/ip` test passes with no `.env` and no `SERVER.*` env vars set; `RS3__SERVER__HOST` and `SERVER.HOST` both work
- [ ] **Tests:** no `thread::sleep`, no `set_var`, no shared global map; each test builds its own store
- [ ] **Docker:** container actually *starts* (`docker run --rm image` then `curl /`), not just builds; builder and runtime are on the same Debian codename; `--locked` used; `.dockerignore` present
- [ ] **CI:** no `continue-on-error`; clippy uses `--all-targets`; the cache step doesn't exceed a few GB
- [ ] **Docs:** Readme lists the op-name mapping, the env-var forms, and the toolchain

## Recovery Strategies

| Pitfall | Recovery Cost | Recovery Steps |
|---------|---------------|----------------|
| Duplicate v8 in graph | LOW | Remove the direct `v8` dep, `cargo update -p deno_core`, check `cargo tree -i v8` |
| Abort on op panic | LOW | Convert store functions to `Result`, ops to `Result<_, JsErrorBox>`, add a deny lint |
| Per-worker stores | LOW | Move `web::Data::new` out of the factory closure |
| PKU segfaults in CI | LOW | Add `init_platform` in main plus the dev-dep feature; re-run CI |
| glibc mismatch | LOW | Align Docker stages on one codename |
| dashmap deadlock in production | MEDIUM | Refactor the store API to owned returns or switch to papaya; add a write-from-JS test |
| deno_core upgrade broke API mid-milestone | MEDIUM | Revert the lock to 0.412.0; schedule the upgrade as its own plan |
| Format change on `/list` upsets clients | LOW | Characterization test plus documented decision; optional compat flag |

## Pitfall-to-Phase Mapping

| Pitfall | Prevention Phase | Verification |
|---------|------------------|--------------|
| 6 Legacy toolchain file / cargo fix / stale lock / MSRV resolver | P-Toolchain | `rustc -V` = 1.99.x; no git in lock; `cargo update --dry-run` shows latest |
| 5 deno_core/v8/deno_error coupling | P-Toolchain + P-JS | `cargo tree -d` has no v8; build succeeds with `deno_error = "0.7"` |
| 21 Edition 2024 semantics, fmt churn | P-Toolchain | `cargo fmt --check` clean after the dedicated fmt commit |
| 22 Clippy churn | P-Toolchain (config), P-CI (gate) | `cargo clippy --all-targets -- -D warnings` clean |
| 8 dashmap deadlock / owned API | P-Store | Test: PATCH script that inserts and deletes keys completes |
| 18 Response quirks / golden tests | P-Store + P-HTTP | Characterization tests written *before* the port, then updated with recorded decisions |
| 1 Op panics abort | P-Store + P-JS | JS calls an op with a missing key, gets a JS exception, the server survives |
| 2 Re-entrancy check | P-JS | Nested-patch test green in debug |
| 3 Recursion / stack | P-JS | Self-recursive script returns an error |
| 4 Platform init / PKU | P-JS + P-CI | Parallel tests green on an x86 runner |
| 9 Old JS syntax | P-JS + P-Docs | grep is empty; sample tests pass |
| 10 Result reading / execute_script API | P-JS | Tests for number, undefined, JSON string results |
| 19 / 20 Hang, OOM, `!Send`, runtime reuse | P-JS | `while(true)` test returns 408/422 within the deadline |
| 7 Per-worker store | P-HTTP | Multi-worker smoke test |
| 17 actix test API | P-HTTP | Tests compile with no `body::Body` import |
| 11 set_var / config load-once | P-Config + P-HTTP | `#![forbid(unsafe_code)]` compiles; `/ip` test env-free |
| 16 config env naming | P-Config | Unit tests via `Environment::source(Some(map))` |
| 12 / 13 / 14 V8 download, glibc, chef layering | P-Docker | `docker build` from clean cache plus `docker run` plus `curl /` |
| 15 CI | P-CI | A deliberately broken fmt commit fails CI |
| 25 Link time | P-HTTP / P-CI | Single test binary; CI time recorded |
| 26 Out-of-scope toolchains | P-Docs | Readme "known limitations" entry |

## Sources

- Crate sources inspected locally (HIGH):
  - `deno_core-0.412.0`: `runtime/jsruntime.rs` (`scope!`, `init_platform`, `execute_script`), `runtime/setup.rs` (PKU / `unsafe_use_unprotected_platform`), `ops.rs` (`reentrancy_check`), `01_core.js` (`Deno.core.print`, `ops` object), `examples/{hello_world,op2,eval_js_value}.rs`, `Cargo.toml` (`deno_v8` facade, `deno_error =0.7.1`, `serde_json/preserve_order`)
  - `deno_ops-0.288.0/op2/`: `mod.rs` (ShouldBeFast), `dispatch_slow.rs` (`extern "C"`, no catch_unwind), `config.rs` (reentrant), `README.md` (string/serde/errors)
  - `deno_v8-0.4.0`: `lib.rs`, `Cargo.toml` (v8 150.4.0, quickjs alternative)
  - `v8-150.4.0`: `build.rs` (URL scheme, `RUSTY_V8_ARCHIVE`/`RUSTY_V8_MIRROR`, `~/.cargo/.rusty_v8`, deno→python→curl fallback), `src/platform.rs` (pkeys doc), `src/isolate.rs` (drop-order assertion)
  - `config-0.15.27/src/env.rs` (lowercasing, separator, prefix_separator), `src/value.rs` (string→uint)
  - `dotenvy-0.15.7` (`.` allowed in keys; internal `set_var`)
  - `actix-web-4.17.0/src/test/test_utils.rs`
  - `dashmap-6.2.1/src/lib.rs` (deadlock docs)
- crates.io API, 2026-10-20 (HIGH): deno_core 0.412.0, v8 152.2.0, deno_v8 0.4.0, deno_error 0.7.3, config 0.15.27, dotenvy 0.15.7, dashmap 6.2.1, papaya 0.2.5, flurry 0.5.2, actix-web 4.17.0
- HTTP HEAD on `github.com/denoland/rusty_v8/releases/download/v150.4.0/librusty_v8_simdutf_release_aarch64-unknown-linux-gnu.a.gz` (200, 37,577,326 bytes); musl and x86_64 variants also present (HIGH)
- docker-library official-images `library/rust` manifest and `rust-lang/docker-rust` `stable/trixie/slim/Dockerfile` (HIGH): `1.99`/`latest` → trixie; slim purges wget, no curl or python
- rustup overrides docs, https://rust-lang.github.io/rustup/overrides.html (`rust-toolchain` beats `rust-toolchain.toml`) (HIGH)
- GitHub changelog: [Actions cache can now exceed 10 GB](https://github.blog/changelog/2025-11-20-github-actions-cache-size-can-now-exceed-10-gb-per-repository/), [cache eviction enforcement date](https://github.blog/changelog/2025-09-29-new-date-for-enforcement-of-cache-eviction-policy/) (MEDIUM-HIGH)
- glibc mismatch reports: [Render community: Missing GLIBC_2.38](https://community.render.com/t/missing-glibc-2-38-from-distro/39054), [users.rust-lang.org: release binary not working in docker](https://users.rust-lang.org/t/release-binary-not-working-in-docker/36383) (MEDIUM)
- Rust Edition Guide 2024 (`set_var` unsafe, if-let rescope, tail-expr temporaries, RPIT capture, never-type fallback, `gen`, rustfmt style edition, merged doctests, resolver v3); Rust 1.81 release notes (abort on unwind out of `extern "C"`) (HIGH, from training knowledge, consistent with the source observed above)
- rs3 repository: `src/main.rs`, `src/bindings/mod.rs`, `src/services/mod.rs`, `src/config/mod.rs`, `requests/*`, `Dockerfile`, `.github/workflows/actions-rs.yml`, `rust-toolchain`, `.planning/PROJECT.md`, `MODERNIZATION_BRIEF.md`

---
*Pitfalls research for: rs3 modernization (actix-web + deno_core/V8, Rust 1.99 / edition 2024, Linux aarch64)*
*Researched: 2026-10-20*
