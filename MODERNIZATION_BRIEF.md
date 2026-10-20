# rs3 Modernization Brief

## What rs3 is

rs3 is a small Rust key/value "information object store" written in 2021 as an MSc
Computer Science project (Birkbeck). It is an actix-web HTTP server that stores
string key/value pairs in a lock-free concurrent map (`flurry`) and can execute a
stored value as JavaScript inside an embedded V8 runtime (`deno_core` + `rusty_v8`).
JavaScript running in that runtime can call back into the store through registered
ops (`rs3_list`, `rs3_get`, `rs3_insert`, `rs3_delete`, `rs3_patch`).

HTTP API (must keep working, same routes and semantics):

- `GET /` → welcome message
- `GET /ip` → configured host
- `POST /insert` → JSON object body, inserts every key/value
- `GET /list` → all entries
- `GET /key/{key}` → value for key
- `DELETE /key/{key}` → removes key, returns old value
- `PATCH /key/{key}` → executes the stored value as JavaScript in V8, returns result

Configuration comes from `.env` / environment (`SERVER.HOST`, `SERVER.PORT`).
Sample requests live in `requests/` (JS programs like `fac.js`, `sum.js`, `bin.js`).

## The problem

The project was pinned to Rust 1.56 (`rust-toolchain`), edition 2018, and depends on
pre-release / moving targets that no longer build with a modern toolchain:

- `actix-web = "4.0.0-beta.8"` (beta API: `body::Body`, test utilities changed in 4.x final)
- `deno_core` from `git` **branch = "main"** (unpinned; the `op_sync` / `register_op` /
  `sync_ops_cache` / `Deno.core.opSync` API was removed long ago — modern deno_core uses
  `#[op2]` macros, `extension!`, and `Deno.core.ops.*`)
- `rusty_v8 = "0.27.0"` (renamed to the `v8` crate)
- `config = "0.11"` (old `merge`/`try_into` API), `dotenv` (unmaintained → `dotenvy`),
  `lazy_static` (→ `std::sync::LazyLock`), `flurry` (consider `dashmap`/`papaya` if flurry is stale)
- Dockerfile uses `rust:1.53`; CI uses the archived `actions-rs/*` actions with
  `continue-on-error: true` everywhere.

## Goal of this milestone

Bring rs3 to the **latest stable Rust toolchain (currently 1.99) and edition 2024**,
with all dependencies on current, released (non-git, non-beta) versions, so that
`cargo build`, `cargo test`, `cargo clippy -- -D warnings` and `cargo fmt --check`
all pass cleanly.

Requirements:

1. `rust-toolchain.toml` pinned to current stable with `rustfmt` and `clippy` components;
   `edition = "2024"`, `rust-version` set.
2. All dependencies upgraded to latest released versions; no git or beta dependencies.
   `Cargo.lock` regenerated.
3. Port the embedded JavaScript engine to the current `deno_core` API (ops via `#[op2]`,
   registered through an `extension!`), keeping the five store ops callable from JS.
   Update the JS in tests and in `requests/*.js` / `src/example.js` to the new op call
   syntax (`Deno.core.ops.op_name(...)`). Op names may change to valid identifiers but
   document the mapping.
4. Port actix-web to the current 4.x release API (body handling in tests, `test::init_service`, etc.).
5. Fix the obvious robustness issues uncovered while porting: no `unwrap()` panics on
   missing keys / bad JSON / JS exceptions in request handlers — return proper HTTP
   error codes instead. Keep response bodies compatible where tests assert them.
6. Fix test isolation: tests currently share a global map and use `thread::sleep`
   to avoid races. Make them deterministic (e.g. unique keys per test or injected state).
7. Replace `lazy_static` with `std::sync::LazyLock` (or app state via `web::Data`).
8. Modernize CI: GitHub Actions using `actions/checkout@v4`+, `dtolnay/rust-toolchain`,
   `Swatinem/rust-cache`; check/test/fmt/clippy must actually fail the build (remove
   `continue-on-error`).
9. Update the Dockerfile to a current `rust` image (cargo-chef still fine) and a slim
   runtime image.
10. Update `Readme.md` for the new toolchain, run instructions, and new JS op syntax.

Out of scope for this milestone (leave untouched): `experiments/`, `aws/`, `docker_rs3/`
Ansible content, `Vagrantfile` — except where they directly reference the Rust version
and changing that is trivial.

## Future direction (NOT this milestone — but don't paint us into a corner)

The modernized rs3 is meant to be a base for modern agentic coding. Planned later work:

- Git integration (store/serve objects backed by git repositories)
- VS Code integration (extension / LSP-style access to the store)
- TypeScript execution via the new Rust-native TypeScript compiler (tsc-rs) so stored
  TypeScript can be type-checked and run, not just JavaScript
- Online git access (serving repositories over HTTP)

Architecture choices in this milestone should keep the code modular (separate store,
JS runtime, and HTTP layers; shared state injected rather than global) so these can be
added as later milestones.

## Constraints

- Platform: Linux aarch64 (V8 prebuilt static libs from the `v8` crate are available for
  aarch64-linux). First build of V8 is slow — that is expected.
- Keep it a single binary crate unless a workspace split is clearly justified.
- Work on branch `modernize-toolchain`; commit per plan with clear messages.
