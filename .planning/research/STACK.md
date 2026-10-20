# Stack Research

**Domain:** Rust HTTP key/value store (actix-web) with embedded V8 JavaScript execution (deno_core), brownfield modernization from Rust 1.56/edition 2018 to Rust 1.99/edition 2024
**Researched:** 2026-10-20
**Confidence:** HIGH. Versions come from the live crates.io, GitHub, Docker Hub and gcr.io APIs, and the APIs were checked in the downloaded crate sources. A throwaway spike crate with exactly the recommended dependency set **compiled, passed `cargo test`, passed `cargo clippy --all-targets -- -D warnings`, and served HTTP** on this aarch64 Linux host with Rust 1.99.0, and also compiled with 1.95.0.

> Note on confidence tiers: the GSD `classify-confidence` seam rates generic web providers LOW. The HIGH ratings below come from stronger evidence than a web search: registry API data, reading the crate source, and a compiled and tested spike. Anything that rests only on web search is marked MEDIUM or LOW.

---

## Recommended Stack

### Toolchain

| Item | Value | Why | Confidence |
|------|-------|-----|------------|
| Rust toolchain | **1.99.1** (current stable, released 2026-10-13; the host has 1.99.0) | The milestone asks for "latest stable 1.99". 1.99.1 is the current point release. Pin the exact patch so CI and Docker agree. | HIGH (static.rust-lang.org channel manifest) |
| `rust-toolchain.toml` | `channel = "1.99.1"`, `components = ["rustfmt", "clippy"]`, `profile = "minimal"` | Replaces the legacy `rust-toolchain` file (`1.56.0`). **Delete the old file.** If both files exist, the legacy `rust-toolchain` file wins. | HIGH |
| `edition` | `"2024"` | All of deno_core, deno_ops, deno_v8 and v8 are already edition 2024 | HIGH |
| `rust-version` | `"1.99"` | Declare only what CI actually tests. The real dependency floor is about 1.95 (see the MSRV table), but we will not test it continuously. | HIGH |

### Core Technologies

| Technology | Version | Purpose | Why Recommended | Confidence |
|------------|---------|---------|-----------------|------------|
| `deno_core` | **0.412.0** (2026-09-16) | Embedded JS runtime, ops, extensions | Only maintained Rust embedding of V8 with an op system. Required by the project. Releases come about weekly and each 0.x minor can break things, so pin with a caret on this exact minor. | HIGH (compiled spike) |
| `v8` (transitive, **do not depend on directly**) | **150.4.0**, resolved through `deno_v8 0.4.0` (`v8 ^150.4.0`) | V8 bindings and prebuilt static lib | deno_core no longer depends on `v8` directly. It goes through the new `deno_v8` facade crate, which re-exports `v8` (or QuickJS behind a feature). Use **`deno_core::v8`**. The newest `v8` on crates.io is 152.2.0, but that version is **not** compatible with deno_core 0.412. | HIGH |
| `actix-web` | **4.17.0** (2026-10-15) | HTTP server, routing, `web::Data`, test utilities | The current 4.x line. MSRV is 1.88. The `#[actix_web::main]`, `#[actix_web::test]` and `actix_web::test::*` utilities are all included, so **`actix-rt` and `tokio` no longer need to be direct dependencies**. | HIGH (compiled spike) |
| `papaya` | **0.2.5** (2026-08-21) | Concurrent in-memory map that replaces `flurry` | Lock-free reads with a guard/`pin()` model. It is the closest conceptual replacement for flurry, and its reads cannot deadlock. That matters because JS ops can re-enter the store, and `op_rs3_patch` runs nested JS. Actively released. MSRV is 1.72. | HIGH (version and compile), MEDIUM (preferred over dashmap; this is a judgement call) |
| `config` | **0.15.27** (2026-09-30), `default-features = false` | Load `SERVER.HOST` / `SERVER.PORT` from the environment | The builder API replaces the removed `Config::new()/merge()/try_into()`. With default features off, the TOML/YAML/RON/JSON5/INI parsers are not pulled in. The project only uses `Environment`. MSRV is 1.88. | HIGH (compiled spike, dotted env vars verified at runtime) |
| `dotenvy` | **0.15.7** | `.env` loading (replaces the unmaintained `dotenv`) | The community fork and de-facto standard. The last release is from 2023, but the repo is active (pushed 2026-08) and the crate is feature-complete. It parses dotted keys such as `SERVER.HOST=...` correctly (verified). | HIGH |
| `std::sync::LazyLock` | std (stable since 1.80) | Replaces `lazy_static` *if* any global remains | Only use it for genuinely process-wide immutable things. **The store itself should not be global.** Inject it through `web::Data<Store>` (HTTP) and `OpState` (JS). | HIGH |

### Supporting Libraries

| Library | Version | Purpose | When to Use | Confidence |
|---------|---------|---------|-------------|------------|
| `serde` (+`derive`) | 1.0.229 | Config structs, JSON | Always | HIGH |
| `serde_json` | 1.0.154 | Parse `/insert` bodies; serialize `/list` properly instead of hand-building JSON | Always. Use `serde_json::to_string(&BTreeMap)` for deterministic, escaped listing output. | HIGH |
| `deno_error` | **`"0.7"`, which resolves to 0.7.1** | `deno_error::JsErrorBox` for fallible ops (`Result<T, JsErrorBox>` throws a JS `Error`) | Only if ops return `Result`. deno_core pins **`deno_error = "=0.7.1"`**, so `cargo add deno_error` (which picks 0.7.3) produces an unresolvable graph. Write `deno_error = "0.7"` by hand. deno_core does **not** re-export `deno_error`. | HIGH (crates.io dependency data plus spike) |
| `thiserror` | 2.0.21 | Typed store/JS errors that map to HTTP status codes (`impl ResponseError`) | Recommended for the error-handling requirement (no `unwrap` in handlers) | HIGH (version), MEDIUM (design choice) |
| `dashmap` | 6.2.1 | Alternative concurrent map | Only if papaya is rejected (see Alternatives) | HIGH (version) |

### Development / CI / Container Tools

| Tool | Version / Ref | Purpose | Notes | Confidence |
|------|---------------|---------|-------|------------|
| `actions/checkout` | **`@v7`** (v7.0.1, 2026-07-20; node24) | Checkout | The brief says v4 or newer. v7 is the current major. Its breaking change only affects `pull_request_target`/`workflow_run` fork checkouts, which is irrelevant here. v4.4.0, v5.1.0 and v6.1.0 also received the backport, so any of them works, but use the newest. | HIGH (GitHub API) |
| `dtolnay/rust-toolchain` | **`@1.99.1`** (branch exists) with `components: rustfmt, clippy` | Install the toolchain | dtolnay publishes no release tags. The ref *is* the toolchain (`@stable`, `@1.99.1`, `@master` with `toolchain:`). Using the exact version keeps CI in step with `rust-toolchain.toml`. | HIGH |
| `Swatinem/rust-cache` | **`@v2`** (v2.9.3, released 2026-10-20) | Cache `~/.cargo` and `target/` | Its default key already hashes `rust-toolchain.toml`, `Cargo.lock` and `Cargo.toml`. Expect large caches, because V8 makes `target/` about 2.6 GB in debug. | HIGH |
| Builder image | **`lukemathwalker/cargo-chef:latest-rust-1.99.1-trixie`** (multi-arch amd64/arm64) | Planner/cook/build stages | Comes with cargo-chef 0.1.78, so you skip `cargo install cargo-chef` in two stages. **Do not use a `-slim` Rust image**: it ships only `wget`, and the v8 build script downloads the prebuilt lib with deno, python, or **curl**, so the download fails. The full `rust:1.99.1-trixie` (based on buildpack-deps, which has curl) also works. | HIGH (Docker Hub API plus `v8` build.rs and the docker-rust Dockerfile) |
| Runtime image | **`gcr.io/distroless/cc-debian13:nonroot`** (arm64 and amd64 available) | Slim runtime | The binary links only `libgcc_s`, `libm` and `libc` (checked with ldd). V8's libc++ is linked statically. This is exactly what `distroless/cc` provides. Use `debian:trixie-slim` instead if you need a shell for debugging. **The glibc must match the builder (trixie / Debian 13).** Do not pair a trixie builder with a bookworm runtime. | HIGH |
| `cargo-chef` | 0.1.78 | Docker dependency-layer caching | Still the standard. `cargo chef cook` also bakes the downloaded V8 static lib into the cached layer, which is a big win. | HIGH |

---

## Verified API Cheat-Sheet (deno_core 0.412 / actix-web 4.17 / config 0.15)

All snippets below come from the spike that compiled, passed its tests and was clippy-clean on 1.99.0.

### Ops with `#[op2]`, shared state in `OpState`, registration through `extension!`

```rust
use std::sync::Arc;
use deno_core::{extension, op2, JsRuntime, OpState, RuntimeOptions, v8};

#[op2]
#[string]                                   // return a JS string
fn op_rs3_get(state: &mut OpState, #[string] key: String) -> Option<String> {
    state.borrow::<Arc<Store>>().get(&key)  // None becomes JS null
}

#[op2]
#[string]
fn op_rs3_delete(state: &mut OpState, #[string] key: String)
    -> Result<String, deno_error::JsErrorBox> {  // Err is thrown as a JS Error
    state.borrow::<Arc<Store>>().remove(&key)
        .ok_or_else(|| deno_error::JsErrorBox::generic(format!("key not found: {key}")))
}

#[op2(fast)]                                // fast path ok for &str args with no return value
fn op_rs3_insert(state: &mut OpState, #[string] key: &str, #[string] value: &str) {
    state.borrow::<Arc<Store>>().insert(key.to_owned(), value.to_owned());
}

extension!(
    rs3_ext,
    ops = [op_rs3_list, op_rs3_get, op_rs3_insert, op_rs3_delete, op_rs3_patch],
    options = { store: Arc<Store> },
    state = |state, options| { state.put::<Arc<Store>>(options.store); },
);
// Usage: RuntimeOptions { extensions: vec![rs3_ext::init(store)], ..Default::default() }
```

- The registration function is **`rs3_ext::init(...)`**. The old `init_ops_and_esm` / `init_ops` names are gone.
- From JS, call **`Deno.core.ops.op_rs3_get("k")`**. `Deno.core.ops` is now a plain object. The old `Deno.core.ops()` bootstrap call (still in the current tests) **throws**, and `Deno.core.opSync` no longer exists. The op name is the Rust function identifier.
- Suggested old-to-new mapping: `rs3_list→op_rs3_list`, `rs3_get→op_rs3_get`, `rs3_insert→op_rs3_insert`, `rs3_delete→op_rs3_delete`, `rs3_patch→op_rs3_patch`. The `op_` prefix is the convention, and deno lints for it.
- `#[string] String`, `#[string] &str` and `#[string] Option<String>` returns all work. The old `rs3_insert(request_body: String)` took a JSON object string. Keep that signature (`#[string] body: String`) if JS callers depend on it.

### `execute_script` and reading the result (replaces the `HandleScope`/`Context::new` code in `bindings::patch`)

```rust
let global: v8::Global<v8::Value> = rt.execute_script(name.to_owned(), code)  // name: impl IntoModuleName (String ok)
    .map_err(|e: Box<deno_core::error::JsError>| e.to_string())?;           // e.g. "Error: boom\n    at t.js:1:7"
deno_core::scope!(scope, &mut rt);        // macro: handle scope + main-context ContextScope
let local = v8::Local::new(scope, global);
Ok(local.to_rust_string_lossy(scope))
// or: deno_core::serde_v8::from_v8::<serde_json::Value>(scope, local)
```

- `execute_script` returns `Result<v8::Global<v8::Value>, Box<JsError>>`. JS exceptions come back as `Err` and **do not panic**, so they map cleanly to HTTP 4xx/5xx.
- `JsRuntime::handle_scope()` no longer exists. v8 now uses pinned scopes (`v8::scope!`, `PinScope`). Use the **`deno_core::scope!(scope, runtime)`** macro, which already enters the main context. **Do not** create a fresh `v8::Context` the way the current code does. Reading the result in a new context was always wrong.

### V8 platform initialisation (do this once, before any `JsRuntime::new`)

```rust
static V8_INIT: std::sync::Once = std::sync::Once::new();
pub fn ensure_v8() {
    V8_INIT.call_once(|| {
        JsRuntime::init_platform(Some(v8::new_unprotected_default_platform(0, false).make_shared()));
    });
}
```

Call it at the top of the JS-runtime constructor. By default deno_core uses V8's *protected* platform (memory protection keys) unless deno_core itself is compiled with `cfg(test)`, which **our** tests do not trigger. With the protected platform, every thread that touches V8 must descend from the thread that called `V8::initialize`. libtest worker threads and actix/`web::block` threads break that rule. On x86_64 CPUs with PKU (typical GitHub `ubuntu-latest` runners) the result is SIGSEGV. On this aarch64 host the spike passed without it, so **the crash would show up only in CI**. The docs for `new_unprotected_default_platform` name this exact case. An alternative is the deno_core feature `unsafe_use_unprotected_platform`.

### actix-web 4.17 handlers and tests

```rust
#[patch("/key/{key}")]
async fn patch_key(store: web::Data<Store>, path: web::Path<String>) -> actix_web::Result<HttpResponse> {
    let store: Arc<Store> = store.into_inner();        // web::Data<T> wraps an Arc<T>
    let res = web::block(move || run_js(store, &key, code)).await?;   // JsRuntime is !Send: create+drop it inside the closure
    ...
}
// App::new().app_data(store.clone())  with  let store = web::Data::new(Store::default());

// tests
use actix_web::{http::StatusCode, test as atest};      // see the pitfall below
#[actix_web::test]
async fn t() {
    let app = atest::init_service(App::new().app_data(store.clone()).service(get_key)).await; // no `mut` needed
    let body: web::Bytes = atest::call_and_read_body(&app, atest::TestRequest::get().uri("/key/a").to_request()).await;
    assert_eq!(body, web::Bytes::from_static(br#""1""#));
    let resp = atest::call_service(&app, req).await;    // &app, not &mut app
    let body = atest::read_body(resp).await;            // replaces resp.into_body() == Body::from(..)
}
```

- `actix_web::body::Body` no longer exists, so compare against `Bytes` instead.
- **Pitfall:** `use actix_web::test;` shadows the built-in `#[test]` attribute in that module. Plain sync tests then fail with "the async keyword is missing". Import it as `test as atest`, or call `actix_web::test::...` with the full path.

### config 0.15 builder (replaces `Config::new()` + `merge` + `try_into`)

```rust
dotenvy::dotenv().ok();
let cfg: Conf = config::Config::builder()
    .add_source(config::Environment::default())   // SERVER.HOST -> server.host (dotted keys become nested paths)
    .build()?
    .try_deserialize()?;
```

The runtime check passed with both a real environment (`env 'SERVER.HOST=127.0.0.1' 'SERVER.PORT=…'`) and a `.env` file. Consider also accepting `SERVER__HOST` (`.separator("__")`) later, because shells cannot `export` dotted names. Treat that as optional and out of scope for compatibility.

---

## Installation

```toml
[package]
name = "rs3"
version = "0.2.0"
edition = "2024"
rust-version = "1.99"

[dependencies]
actix-web  = { version = "4.17.0", default-features = false, features = ["macros"] }
deno_core  = "0.412.0"
deno_error = "0.7"           # MUST stay 0.7.x; deno_core pins =0.7.1. Do not `cargo add` it.
papaya     = "0.2.5"
config     = { version = "0.15.27", default-features = false }
dotenvy    = "0.15.7"
serde      = { version = "1.0.229", features = ["derive"] }
serde_json = "1.0.154"
thiserror  = "2.0.21"
```

```toml
# rust-toolchain.toml
[toolchain]
channel = "1.99.1"
components = ["rustfmt", "clippy"]
profile = "minimal"
```

Turning off actix-web's default features drops brotli, gzip, zstd (C build), cookies, http2 and ws, none of which rs3 uses. The spike verified that the server and tests still work. Add back `"compress-gzip"` etc. later if needed.

---

## Alternatives Considered

| Recommended | Alternative | When to Use Alternative |
|-------------|-------------|-------------------------|
| `papaya` 0.2.5 | `dashmap` 6.2.1 (by far the most downloaded, about 93M recent) | If you want the most battle-tested crate and a simpler `get/insert` API. The caveat is that holding a `Ref` from `get()` while writing to the same shard **deadlocks**, which is a real risk with re-entrant JS ops. Always clone values out immediately. Either map is fine behind a `Store` facade that returns owned `String`s. |
| `papaya` | `flurry` 0.5.2 | Do not. See "What NOT to Use". |
| Per-request `JsRuntime` inside `web::block` | Dedicated JS worker thread(s) with a channel, or a runtime pool | Later, when the TS/tsc-rs milestone needs warm runtimes or snapshots. Isolate creation costs a few ms per request, which is fine for this milestone. |
| `distroless/cc-debian13:nonroot` | `debian:trixie-slim` | When you need a shell or package manager in the runtime image |
| `lukemathwalker/cargo-chef` image | `rust:1.99.1-trixie` plus `cargo install cargo-chef --locked` | If you prefer official images only. It is slower because cargo-chef is compiled in each stage. |
| `dtolnay/rust-toolchain@1.99.1` | Rely on `rust-toolchain.toml` auto-install by the runner's rustup | Possible, but rustup's auto-install behaviour changed during 1.28.x (MEDIUM confidence), so being explicit is safer. |
| GitHub `ubuntu-latest` (x86_64) | `ubuntu-24.04-arm` runner | If you want CI to match the aarch64 dev box. Free for public repos (MEDIUM, not re-verified). |

---

## What NOT to Use

| Avoid | Why | Use Instead |
|-------|-----|-------------|
| `deno_core` from git (`denoland/deno_core`) | The standalone **`denoland/deno_core` repo is archived** (since 2026-02). deno_core now lives at `denoland/deno` `libs/core` and is published to crates.io. Git deps also violate the "released only" rule. | `deno_core = "0.412.0"` from crates.io |
| `rusty_v8` crate | Renamed to `v8` years ago and frozen | `deno_core::v8` |
| A direct `v8 = "152"` (or any `v8`) dependency | deno_core 0.412 goes through `deno_v8 0.4.0`, which needs `v8 ^150.4.0`. Adding a different major causes duplicate V8 linkage or resolution failure. Even a matching version is redundant. | `use deno_core::v8;` |
| `op_sync`, `register_op`, `sync_ops_cache`, `#[op]` (op1), `Deno.core.opSync`, `Deno.core.ops()` | Removed from deno_core | `#[op2]`, `extension!`, `Deno.core.ops.op_x(...)` |
| `JsRuntime::handle_scope()` / `v8::HandleScope::new` plus `Context::new` to read results | Removed (pinned-scope API) and semantically wrong (new context) | `deno_core::scope!(scope, rt)` |
| `actix-web 4.0.0-beta.*`, `actix_web::body::Body`, `&mut app` in test calls | Beta-only API | actix-web 4.17, `Bytes`, `test::read_body`/`call_and_read_body` |
| Direct `actix-rt` / `tokio` deps | Redundant. actix-web re-exports `main`/`test` macros and the runtime. | `#[actix_web::main]`, `#[actix_web::test]` |
| `config::Config::new()`, `merge`, `try_into` | Removed in 0.14/0.15 | `Config::builder().add_source(..).build()?.try_deserialize()` |
| `dotenv` | Unmaintained (RUSTSEC-2021-0141) | `dotenvy` |
| `lazy_static` (and `once_cell::sync::Lazy`) | Superseded by std | `std::sync::LazyLock`, but prefer injected state |
| `flurry` 0.5.2 | Last release 2024-11. About 1.9M recent downloads against papaya's 6.5M and dashmap's 93M. papaya is built on the same `seize` memory-reclamation crate that flurry uses, so it is the natural successor. flurry gets little maintenance for a core data structure. | `papaya` (or `dashmap`) |
| `cargo add deno_error` (unversioned) | Picks 0.7.3, which conflicts with deno_core's `=0.7.1` pin | Write `deno_error = "0.7"` by hand |
| `rust:*-slim` as the **builder** image | No curl or python, so the V8 prebuilt download in `v8`'s build.rs fails | `rust:1.99.1-trixie` or `cargo-chef:latest-rust-1.99.1-trixie` |
| `actions-rs/*` actions | Archived and unmaintained | `dtolnay/rust-toolchain`, plain `run: cargo …` |

---

## Stack Patterns by Variant

**If CI runs on x86_64 (`ubuntu-latest`):**
- `ensure_v8()` with `new_unprotected_default_platform` is mandatory, because PKU is present on most x86 runner CPUs.
- The V8 prebuilt `librusty_v8_release_x86_64-unknown-linux-gnu.a.gz` is downloaded at build time, so the network is required on cold caches.

**If building offline or behind a proxy (Docker or CI):**
- Set `RUSTY_V8_MIRROR` (a base URL) or `RUSTY_V8_ARCHIVE` (a path or URL to the `.a.gz`). The build script also checks `$CARGO_HOME/.rusty_v8/<escaped-url>` before downloading.

**If a musl/Alpine image is wanted later:**
- Prebuilts exist for `aarch64-unknown-linux-musl` and `x86_64-unknown-linux-musl` (v150.4.0 assets), but the cargo-chef/Alpine path is untested here, so stay on glibc and trixie for this milestone.

---

## Version Compatibility

| Package | Compatible With | Notes |
|---------|-----------------|-------|
| deno_core 0.412.0 | deno_ops ^0.288.0, serde_v8 ^0.321.0, deno_v8 ^0.4.0, deno_error **=0.7.1**, tokio ^1.47.1, serde ^1.0.149, thiserror ^2.0.12 | One coherent release train, all published 2026-09-16 |
| deno_v8 0.4.0 | v8 ^150.4.0 (default feature `v8`); v8x =149.4.0-rc.4 **only** with the opt-in `quickjs` feature | The pre-release v8x is *not* pulled in with default features. `Cargo.lock` stays free of pre-releases (verified). |
| v8 150.4.0 | Prebuilt `librusty_v8_release_aarch64-unknown-linux-gnu.a.gz` (35 MB) and `…x86_64-unknown-linux-gnu.a.gz` | Published 2026-07-24. On this aarch64 host the cold build of the full stack took about 1.5 min, because the download replaces a V8 source build. |
| actix-web 4.17.0 | actix-http ^3.19, actix-rt ^2.6 (resolves 2.15.0), tokio ^1.38.2 (resolves 1.54.0) | Shares one tokio with deno_core, with no conflicts |
| config 0.15.27 | serde_core ^1.0.228 | Works with serde 1.0.229 |
| Rust 1.99.1 | Every crate above | The spike also builds on 1.95.0 |

### MSRV per crate (as declared on crates.io, `rust_version`)

| Crate | Version | MSRV |
|-------|---------|------|
| deno_core / deno_ops / deno_v8 / serde_v8 / v8 | 0.412.0 / 0.288.0 / 0.4.0 / 0.321.0 / 150.4.0 | **Not declared.** Edition 2024 implies at least 1.85. Upstream deno builds with **1.95.0** (`denoland/deno` rust-toolchain.toml) and rusty_v8 with 1.91.0. The spike compiled on 1.95.0. |
| actix-web / actix-rt | 4.17.0 / 2.15.0 | 1.88 |
| config | 0.15.27 | 1.88.0 |
| tokio (transitive) | 1.54.0 | 1.85 |
| thiserror | 2.0.21 | 1.77 |
| serde_json | 1.0.154 | 1.71 |
| papaya | 0.2.5 | 1.72.0 |
| dashmap | 6.2.1 | 1.65 |
| serde | 1.0.229 | 1.56 |
| dotenvy | 0.15.7 | 1.56.1 |
| `std::sync::LazyLock` | std | 1.80 |
| **Effective floor for rs3** | | **About 1.95** (deno_core). Declare `rust-version = "1.99"` because that is what CI tests. |

---

## Sources

- crates.io API (`/api/v1/crates/{name}` and `/{name}/{ver}/dependencies`), queried 2026-10-20: versions, dates, `rust_version`, exact dependency pins for deno_core 0.412.0, deno_v8 0.4.0, serde_v8 0.321.0, actix-web 4.17.0, config 0.15.27, papaya 0.2.5, dashmap 6.2.1, flurry 0.5.2, dotenvy 0.15.7. HIGH
- Crate sources in `~/.cargo/registry` (deno_core 0.412.0 `runtime/jsruntime.rs` [`execute_script`, `scope!`, `init_platform`], `extensions.rs` [`extension!` / `init`], `runtime/setup.rs` [protected vs unprotected platform], `examples/op2.rs`, `examples/eval_js_value.rs`, `01_core.js`; v8 150.4.0 `build.rs` [prebuilt URL, `RUSTY_V8_MIRROR`/`RUSTY_V8_ARCHIVE`, deno/python/curl downloader], `src/platform.rs` [PKU note]; actix-web 4.17.0 `src/test/test_utils.rs`; config 0.15.27 `builder.rs`/`env.rs`). HIGH
- A throwaway spike crate (since deleted) built and tested with the exact dependency set on aarch64 Linux, Rust 1.99.0 (build, test, clippy `-D warnings`, live HTTP plus env/.env config) and 1.95.0 (check). HIGH
- GitHub API: `denoland/rusty_v8` release v150.4.0 assets (aarch64/x86_64 gnu and musl prebuilts); `denoland/deno_core` archived=true; `denoland/deno` `rust-toolchain.toml` (1.95.0) and workspace `Cargo.toml` (deno_core 0.412.0 at `libs/core`); `actions/checkout` releases (v7.0.1 latest; v4.4.0/v5.1.0/v6.1.0 backports); `Swatinem/rust-cache` v2.9.3; `dtolnay/rust-toolchain` branches (`1.99.1` exists). HIGH
- static.rust-lang.org `channel-rust-stable.toml`: rustc 1.99.1 (2026-10-13). HIGH
- Docker Hub API: `rust:1.99.1-trixie` / `-slim-trixie` (arm64 and amd64), `lukemathwalker/cargo-chef:latest-rust-1.99.1-trixie` (arm64 and amd64), `debian:trixie-slim`; gcr.io `distroless/cc-debian13` tags (`nonroot`, arm64 and amd64). `rust-lang/docker-rust` trixie/slim Dockerfile (wget only, no curl). HIGH
- Web search on V8 PKU/thread crashes ([v8-users thread](https://groups.google.com/g/v8-users/c/sPJONshJJI8)). This is corroborating background only. The authoritative statement is the `v8` crate doc comment. MEDIUM

---
*Stack research for: rs3 modernization (actix-web + deno_core K/V store)*
*Researched: 2026-10-20*
