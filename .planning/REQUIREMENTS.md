# Requirements: rs3

**Defined:** 2026-10-20
**Core Value:** rs3 builds, tests and lints cleanly on current stable Rust (edition 2024) with only released dependencies — while every existing HTTP route and the JS-executes-against-the-store behaviour keep working with the same semantics.

## v1 Requirements

Requirements for the modernization milestone. Each maps to roadmap phases.

### Toolchain

- [x] **TOOL-01**: Developer running any `cargo` command in the repo gets Rust 1.99.1 with `rustfmt` and `clippy` via `rust-toolchain.toml`; the legacy `rust-toolchain` file is deleted
- [x] **TOOL-02**: `Cargo.toml` declares `edition = "2024"` and `rust-version = "1.99"`
- [x] **TOOL-03**: `cargo build`, `cargo test`, `cargo clippy --all-targets -- -D warnings` and `cargo fmt --check` all pass on a clean checkout

### Dependencies

- [x] **DEPS-01**: All dependencies are released crates.io versions — no git, path or pre-release (beta/rc) dependencies
- [x] **DEPS-02**: `Cargo.lock` is regenerated from scratch and `cargo tree -d` shows a single V8 (no direct `v8`/`rusty_v8` dependency; `deno_core::v8` used)
- [x] **DEPS-03**: `lazy_static`, `dotenv` and `flurry` are removed (replaced by injected state / `LazyLock`, `dotenvy`, and a maintained concurrent map such as `papaya`)
- [x] **DEPS-04**: Configuration uses the current `config` crate builder API and still reads `SERVER.HOST` / `SERVER.PORT` from `.env` or the environment

### Architecture

- [x] **ARCH-01**: Code is split into separate store, JS runtime, HTTP and config modules (`lib.rs` + thin `main.rs`, single package) with dependencies pointing one way (http → js → store)
- [x] **ARCH-02**: The store is created once in `main` and injected into HTTP handlers (`web::Data`) and JS ops (`OpState`); there is no global map
- [ ] **ARCH-03**: Config is loaded once at startup and injected; handlers do not re-read the environment per request
- [x] **ARCH-04**: Startup errors (bad config, port bind failure) are reported and exit non-zero instead of panicking

### HTTP API

- [x] **HTTP-01**: `GET /` returns 200 with body `"Welcome to rs3"` (byte-identical to before)
- [x] **HTTP-02**: `GET /ip` returns 200 with the configured host in the existing body format
- [ ] **HTTP-03**: `POST /insert` with a JSON object of string values inserts every pair and echoes the object in the existing body format
- [ ] **HTTP-04**: `GET /list` returns all entries in the existing list shape, with keys/values correctly JSON-escaped
- [x] **HTTP-05**: `GET /key/{key}` returns the value for an existing key in the existing body format
- [x] **HTTP-06**: `DELETE /key/{key}` removes an existing key and returns its old value
- [ ] **HTTP-07**: `PATCH /key/{key}` executes the stored value as JavaScript and returns the stringified completion value
- [ ] **HTTP-08**: Handlers are ported to the current actix-web 4.x release API (`#[actix_web::main]`, `#[actix_web::test]`, current test utilities)

### Robustness

- [x] **ROBU-01**: `GET`, `DELETE` or `PATCH` on a missing key returns 404 with a JSON error body (no panic; replaces the old `"[]"` on empty store)
- [ ] **ROBU-02**: `POST /insert` with malformed JSON returns 400; valid JSON that is not an object or contains non-string values returns 422; nothing is inserted on error
- [ ] **ROBU-03**: `PATCH` on a script that throws or has a syntax error returns 422 with a JSON error body containing the JS error message
- [ ] **ROBU-04**: No `unwrap()`/`expect()` on request-dependent data in handlers, store or ops; ops return errors to JS instead of panicking (an op panic would abort the process)
- [ ] **ROBU-05**: A script that recursively PATCHes itself (directly or via a cycle) is stopped by a nesting depth limit and returns 422 instead of crashing the server
- [ ] **ROBU-06**: A non-terminating script (e.g. `while(true){}`) is terminated after a configurable timeout and returns 504; the server keeps serving other requests

### JavaScript Runtime

- [ ] **JSRT-01**: The embedded engine uses current released `deno_core` with ops defined via `#[op2]` and registered through `extension!`
- [ ] **JSRT-02**: JS can call five store ops — `Deno.core.ops.op_rs3_list()`, `op_rs3_get(key)`, `op_rs3_insert(json)`, `op_rs3_delete(key)`, `op_rs3_patch(key)` — with the same semantics as the old `rs3_*` ops
- [ ] **JSRT-03**: A missing key or invalid input inside an op surfaces as a catchable JS exception, not a process abort
- [ ] **JSRT-04**: JS execution runs off the actix worker threads (dedicated JS threads) with V8 platform initialised once in `main` before other threads spawn
- [ ] **JSRT-05**: All JS artefacts (`requests/*.js`, `requests/*.txt`, `requests/*.http`, `src/example.js`, test scripts) use the new `Deno.core.ops.op_rs3_*(...)` syntax with `Deno.core.ops();` calls removed
- [ ] **JSRT-06**: The old→new op name mapping is documented

### Testing

- [ ] **TEST-01**: Tests use an isolated store per test (no shared global state, no `thread::sleep`) and pass deterministically under parallel `cargo test`
- [ ] **TEST-02**: Tests do not depend on process environment variables (config is constructed in code)
- [ ] **TEST-03**: Each route has a success test asserting status and preserved body bytes
- [ ] **TEST-04**: Each error path (404, 400, 422 insert, 422 JS exception, depth limit, timeout) has a test
- [ ] **TEST-05**: The sample programs (`fac.js`, `sum.js`, `bin.js`) run as test fixtures and produce expected results
- [ ] **TEST-06**: A test confirms PATCH running `op_rs3_list` returns the same content as `GET /list`

### CI

- [ ] **CI-01**: GitHub Actions workflow uses `actions/checkout` v4+, `dtolnay/rust-toolchain` and `Swatinem/rust-cache`; the archived `actions-rs/*` workflow is removed
- [ ] **CI-02**: CI runs fmt check, clippy (`--all-targets -D warnings`) and tests with `--locked`, and any failure fails the build (no `continue-on-error`)
- [ ] **CI-03**: CI runs on an x86_64 Linux runner and passes (catches V8 platform/PKU issues not visible on aarch64)

### Docker

- [ ] **DOCK-01**: Dockerfile builds with a current `rust` toolchain image (cargo-chef caching, `--locked`) whose Debian release matches the runtime image
- [ ] **DOCK-02**: Runtime image is slim (e.g. distroless cc / debian-slim), non-root, and the container serves `GET /` when run with `SERVER.HOST=0.0.0.0`
- [ ] **DOCK-03**: A `.dockerignore` excludes `target/` and other non-build files

### Documentation

- [ ] **DOCS-01**: `Readme.md` documents the toolchain (Rust 1.99.1 / edition 2024), first-build V8 note, and build/run/test instructions
- [ ] **DOCS-02**: `Readme.md` documents the new JS op syntax with the old→new op mapping and an example
- [ ] **DOCS-03**: `Readme.md` documents HTTP error status codes and a "Behaviour changes" section (404 instead of `"[]"`, 400/422 inserts, escaped `/list`)

## v2 Requirements

Deferred. Tracked but not in the current roadmap.

### JS Runtime Hardening

- **JSHD-01**: V8 heap limit with near-heap-limit callback returns 422 instead of aborting on OOM
- **JSHD-02**: Script execution is cancelled when the HTTP client disconnects
- **JSHD-03**: Bounded JS queue returns 503 when saturated (with tuning)
- **JSHD-04**: V8 startup snapshot to reduce per-request runtime creation cost

### API Evolution

- **APIV-01**: Versioned API (`/v2/...`) without the double-JSON-encoded string envelope
- **APIV-02**: PATCH supports Promise/async script results
- **APIV-03**: `rs3` JS facade object wrapping `Deno.core.ops.op_rs3_*`

### Ops & Tooling

- **OPS-01**: Structured request logging / tracing
- **OPS-02**: `SERVER__HOST` env alias and default host/port
- **OPS-03**: Additional CI jobs (aarch64 runner, cargo-deny, coverage)
- **OPS-04**: Release profile tuning (LTO, strip)

### Future Milestones

- **FUT-01**: Store trait with git-backed storage backend
- **FUT-02**: VS Code / LSP-style access to the store
- **FUT-03**: TypeScript execution via tsc-rs
- **FUT-04**: Serving git repositories over HTTP

## Out of Scope

| Feature | Reason |
|---------|--------|
| Changes to `experiments/`, `aws/`, `docker_rs3/`, `Vagrantfile` | Explicitly excluded by the brief (except trivial Rust-version references) |
| `web::Json` extractor for `/insert` | Rejects requests without a JSON content-type (breaks `curl -d`) and collapses the 400/422 split |
| Reusing one `JsRuntime` across requests | Top-level `let` collisions between scripts (e.g. `fac.js` fails on second run); state leaks between requests |
| `deno_runtime` / fs / net APIs in the JS sandbox | Expands attack surface; not needed for store ops |
| Back-compat shim for `Deno.core.opSync` | Old API removed upstream; clean migration with documented mapping instead |
| Workspace split into multiple crates | Single package required; each extra test binary re-links V8 |
| Persistence / new store features | Modernization milestone, not a feature milestone |
| musl / Alpine images | V8 prebuilt static libs target glibc |

## Traceability

Which phases cover which requirements. Updated during roadmap creation.

| Requirement | Phase | Status |
|-------------|-------|--------|
| TOOL-01 | Phase 1 | Complete |
| TOOL-02 | Phase 1 | Complete |
| TOOL-03 | Phase 1 | Complete |
| DEPS-01 | Phase 1 | Complete |
| DEPS-02 | Phase 1 | Complete |
| DEPS-03 | Phase 1 | Complete |
| DEPS-04 | Phase 1 | Complete |
| ARCH-01 | Phase 1 | Complete |
| ARCH-02 | Phase 2 | Complete |
| ARCH-03 | Phase 2 | Pending |
| ARCH-04 | Phase 1 | Complete |
| HTTP-01 | Phase 2 | Complete |
| HTTP-02 | Phase 2 | Complete |
| HTTP-03 | Phase 2 | Pending |
| HTTP-04 | Phase 2 | Pending |
| HTTP-05 | Phase 2 | Complete |
| HTTP-06 | Phase 2 | Complete |
| HTTP-07 | Phase 3 | Pending |
| HTTP-08 | Phase 2 | Pending |
| ROBU-01 | Phase 2 | Complete |
| ROBU-02 | Phase 2 | Pending |
| ROBU-03 | Phase 3 | Pending |
| ROBU-04 | Phase 3 | Pending |
| ROBU-05 | Phase 5 | Pending |
| ROBU-06 | Phase 5 | Pending |
| JSRT-01 | Phase 3 | Pending |
| JSRT-02 | Phase 3 | Pending |
| JSRT-03 | Phase 3 | Pending |
| JSRT-04 | Phase 3 | Pending |
| JSRT-05 | Phase 3 | Pending |
| JSRT-06 | Phase 3 | Pending |
| TEST-01 | Phase 2 | Pending |
| TEST-02 | Phase 2 | Pending |
| TEST-03 | Phase 3 | Pending |
| TEST-04 | Phase 5 | Pending |
| TEST-05 | Phase 3 | Pending |
| TEST-06 | Phase 3 | Pending |
| CI-01 | Phase 4 | Pending |
| CI-02 | Phase 4 | Pending |
| CI-03 | Phase 4 | Pending |
| DOCK-01 | Phase 6 | Pending |
| DOCK-02 | Phase 6 | Pending |
| DOCK-03 | Phase 6 | Pending |
| DOCS-01 | Phase 6 | Pending |
| DOCS-02 | Phase 6 | Pending |
| DOCS-03 | Phase 6 | Pending |

**Coverage:**
- v1 requirements: 46 total
- Mapped to phases: 46
- Unmapped: 0 ✓

---
*Requirements defined: 2026-10-20*
*Last updated: 2026-10-20 after roadmap creation (traceability mapped to 6 phases)*
