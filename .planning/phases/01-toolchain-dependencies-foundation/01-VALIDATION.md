---
phase: "1"
slug: "toolchain-dependencies-foundation"
# status lifecycle: draft (seeded by plan-phase) → validated (set by validate-phase §6)
# audit-milestone §5.5 distinguishes NOT-VALIDATED (draft) from PARTIAL (validated + nyquist_compliant: false) (#2117)
status: draft
nyquist_compliant: false
wave_0_complete: false
created: "2026-10-20"
---

# Phase 1 — Validation Strategy

> Per-phase validation contract for feedback sampling during execution.
> Source: `01-RESEARCH.md` § Validation Architecture.

---

## Test Infrastructure

| Property | Value |
|----------|-------|
| **Framework** | libtest (`#[test]`) + `#[actix_web::test]` (actix-web 4.17 `macros` feature) |
| **Config file** | none — `Cargo.toml` `[lints]` only |
| **Quick run command** | `cargo test --lib` |
| **Full suite command** | `cargo build && cargo test && cargo clippy --all-targets -- -D warnings && cargo fmt --check` |
| **Estimated runtime** | ~70 seconds cold (V8 prebuilt download), ~30 seconds warm |

---

## Sampling Rate

- **After every task commit:** Run `cargo test --lib` (from the store/config plan on), or the static checks for the toolchain/manifest plan
- **After every plan wave:** Run the full suite command (or `cargo test --lib && cargo clippy --lib --tests -- -D warnings && cargo fmt --check` while the old `main.rs` is still being replaced)
- **Before `/gsd-verify-work`:** Full suite plus every smoke row below must be green
- **Max feedback latency:** 120 seconds

---

## Per-Task Verification Map

Task IDs are filled in by the planner/executor. Each requirement maps to these automated checks:

| Requirement | Behavior | Test Type | Automated Command | Failure signal | Status |
|-------------|----------|-----------|-------------------|----------------|--------|
| TOOL-01 | Toolchain pinned to 1.99.1 with rustfmt+clippy, legacy file gone | smoke | `test ! -e rust-toolchain && cargo --version \| grep -q '^cargo 1.99.1' && rustc --version \| grep -q '^rustc 1.99.1' && cargo clippy --version && cargo fmt --version` | prints `1.56.0`, or "component unavailable" | ⬜ pending |
| TOOL-02 | `edition = "2024"`, `rust-version = "1.99"` | static | `grep -qx 'edition = "2024"' Cargo.toml && grep -qx 'rust-version = "1.99"' Cargo.toml` | grep exit 1 | ⬜ pending |
| TOOL-03 | Four gates pass | build | `cargo build && cargo test && cargo clippy --all-targets -- -D warnings && cargo fmt --check` | any non-zero exit | ⬜ pending |
| DEPS-01 | Released crates only (manifest + active graph, per D-21) | static | `! grep -nE 'git *=\|path *=\|branch *=\|-(alpha\|beta\|rc)' Cargo.toml && [ "$(cargo tree -e all --target all --prefix none \| grep -cE ' v[0-9]+\.[0-9]+\.[0-9]+-')" = 0 ] && [ "$(grep -c 'source = "git+' Cargo.lock)" = 0 ]` | any match / count > 0 | ⬜ pending |
| DEPS-02 | Single V8 via deno_core; lock regenerated | static | `[ "$(cargo tree -d \| grep -cE '^v8 ')" = 0 ] && [ "$(grep -c '^name = "v8"$' Cargo.lock)" = 1 ] && ! grep -q '^name = "rusty_v8"$' Cargo.lock && cargo tree -i v8 -e normal \| grep -q deno_v8 && ! grep -qE '^(v8\|rusty_v8) *=' Cargo.toml && grep -qx 'version = 4' Cargo.lock && ! grep -q 'github.com/denoland/deno' Cargo.lock` | duplicate v8, 2 lock entries, old lock content | ⬜ pending |
| DEPS-03 | lazy_static / dotenv / flurry removed | static | `! grep -qE '^(lazy_static\|dotenv\|flurry\|actix-rt\|tokio\|anyhow) *=' Cargo.toml && ! grep -rnE 'lazy_static\|flurry\|dotenv::\|use dotenv\b' src/` | match found | ⬜ pending |
| DEPS-04 | config builder + dotted keys | unit | `cargo test --lib config::` | test FAILED / 0 tests run | ⬜ pending |
| DEPS-04 | Real env serves `/ip` | smoke | `env -i PATH=$PATH 'SERVER.HOST=127.0.0.1' 'SERVER.PORT=18453' target/debug/rs3 & sleep 2; curl -sf http://127.0.0.1:18453/ip` returns `"{'IP' : '127.0.0.1'}"`; then kill | curl error or different body | ⬜ pending |
| ARCH-01 | Module layout and dependency direction | static | `test -f src/lib.rs && test -d src/store && { test -e src/js/mod.rs \|\| test -e src/js.rs; } && ! grep -rnE 'actix_web\|deno_core' src/store/ && ! test -e src/bindings && ! test -e src/services` | grep match / file exists | ⬜ pending |
| ARCH-01 | Store behaviour | unit | `cargo test --lib store::` | test FAILED / 0 tests run | ⬜ pending |
| ARCH-04 | Missing config exits 1 without panic | smoke | from a temp dir under `/home` with no `.env` in it or its parents: `env -i PATH=$PATH "$REPO/target/debug/rs3"; echo $?` prints `1` and stderr starts with `error:` | exit 0/101, `panicked` in stderr | ⬜ pending |
| ARCH-04 | Invalid port exits 1 | smoke | `env -i PATH=$PATH 'SERVER.HOST=127.0.0.1' 'SERVER.PORT=abc' target/debug/rs3; echo $?` prints `1` | exit 101 / panic | ⬜ pending |
| ARCH-04 | Busy port exits 1 | smoke | hold 127.0.0.1:18451 with a python listener, then `env -i PATH=$PATH 'SERVER.HOST=127.0.0.1' 'SERVER.PORT=18451' target/debug/rs3 2>err; echo $?` prints `1`, `grep -q 'Address already in use' err && ! grep -q panicked err` | exit 101 / panic text | ⬜ pending |
| SC-2 | Binary serves; PATCH answers 501 | smoke | start as in DEPS-04, then `curl -s -o /dev/null -w '%{http_code}' -X PATCH http://127.0.0.1:18453/key/a` prints `501`, and `curl -s http://127.0.0.1:18453/` prints `"Welcome to rs3"` | other status / body | ⬜ pending |

*Status: ⬜ pending · ✅ green · ❌ red · ⚠️ flaky*

---

## Wave 0 Requirements

- [ ] `src/store/mod.rs` `#[cfg(test)] mod tests` — D-18 store cases (ARCH-01)
- [ ] `src/config.rs` `#[cfg(test)] mod tests` — dotted keys, missing/invalid port from an in-code source, no env (DEPS-04)
- [ ] `src/http/mod.rs` `#[cfg(test)] mod tests` — ported legacy tests (D-16), 2 PATCH tests `#[ignore]` (D-17), plus a `PATCH → 501` test

No framework install is needed (libtest + actix-web test utilities).

---

## Manual-Only Verifications

All phase behaviors have automated verification (the smoke rows are scripted shell commands).

---

## Validation Sign-Off

- [ ] All tasks have `<automated>` verify or Wave 0 dependencies
- [ ] Sampling continuity: no 3 consecutive tasks without automated verify
- [ ] Wave 0 covers all MISSING references
- [ ] No watch-mode flags
- [ ] Feedback latency < 120s
- [ ] `nyquist_compliant: true` set in frontmatter

**Approval:** pending
