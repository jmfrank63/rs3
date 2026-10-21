# Milestones

## v0.2 rs3 toolchain modernization (Shipped: 2026-10-21)

**Delivered:** rs3 moved from Rust 1.56 / edition 2018 on git and beta dependencies to Rust 1.99.1 / edition 2024 on released crates only. Every HTTP route and the JS-executes-against-the-store behaviour still work. It gained JSON errors, hard limits for runaway scripts, an x86_64 CI gate and native deployment with an example hardened systemd unit.

**Phases completed:** 6 phases, 16 plans, 42 tasks

**Key accomplishments:**
- **Toolchain and dependencies:** Rust 1.99.1 / edition 2024 with only released crates (deno_core 0.412, actix-web 4.17, papaya 0.2.5, config 0.15 + dotenvy). The crate is a `lib.rs` plus a thin `main.rs`, and startup errors exit 1 instead of panicking.
- **Injected state:** one `AppState { store, Arc<Settings> }`, built once in `main` and shared through `web::Data` and per-job `OpState`. There is no global store left, and every test builds its own state and config in code.
- **HTTP errors:** a JSON error contract replaces the panics.
  - A missing key answers 404 `not_found`.
  - A bad insert answers 400 `malformed_json` or 422 `invalid_input` and writes nothing.
  - A script failure answers 422 `js_error`, and a host fault 500 `internal`.
  - `/list` is sorted by key and escaped with serde_json.
- **JS engine:** five `#[op2]` ops (`Deno.core.ops.op_rs3_*`) in one `extension!`, running on a bounded `rs3-js-*` pool off the actix workers with a reentrant nested PATCH. The four MSc samples run as fixtures.
- **JS limits:** runaway scripts are stopped.
  - The depth limit (default 4) answers 422 `js_depth_limit` with the key chain.
  - The timeout (5000 ms, per-worker watchdog) answers 504 `js_timeout`.
  - Other requests keep being answered meanwhile, as proven by the release smoke on the protected V8 platform.
- **CI, deployment and docs:**
  - CI: one x86_64 `ubuntu-26.04` job running the guards, fmt, clippy `-D warnings`, 216 tests, the release build and the release smoke. It is green remotely with 0 annotations.
  - Deployment: Docker replaced by `cargo build --release --locked` and `deploy/rs3.service` (exposure 1.5).
  - Docs: `Readme.md` rewritten and replayed byte for byte against the release binary (130 checks), under the MIT license.

**Stats:**
- Git range: `97b29ed` (chore: add project config) → `7bee227` (docs: audit milestone v0.2), 261 commits on top of `main`'s merge base `0807b95`
- Outside `.planning`: 58 files changed, 9,230 insertions(+), 3,310 deletions(-)
- About 5,600 lines of Rust in 13 files under `src/`, with 216 tests
- Timeline: 2026-10-20 → 2026-10-21 (2 days)

**Closeout:** `override_closeout`. Not all phases are verified: phases 02 and 06 are `human_needed`, and every automated must-have of both is met (02 20/20, 06 42/43).

Known verification overrides: 4 newly acknowledged, 0 carried forward from a prior close (see STATE.md Deferred Items).
- uat_gaps: 02-UAT.md (items 2–3, the live dotenv-file checks) and 06-UAT.md (items 1–7):
  - the real-server unit run;
  - x86_64 PKU under seccomp;
  - the sudo update and troubleshooting commands;
  - the `MemoryMax=` sizing wording;
  - README readability;
  - remote CI on the final HEAD;
  - the judgment-tier prohibitions.
- verification_gaps: 02-VERIFICATION.md and 06-VERIFICATION.md, both `human_needed`.

**Requirements:** 47/47 v1 requirements checked off. The audit counts 46 satisfied and 1 partial: DEPL-03, whose root-only unit behaviour awaits a real server. See `milestones/v0.2-REQUIREMENTS.md` (Outcomes) and `milestones/v0.2-MILESTONE-AUDIT.md`.

**Archives:** `milestones/v0.2-ROADMAP.md`, `milestones/v0.2-REQUIREMENTS.md`, `milestones/v0.2-MILESTONE-AUDIT.md`, `milestones/v0.2-phases/`

---
