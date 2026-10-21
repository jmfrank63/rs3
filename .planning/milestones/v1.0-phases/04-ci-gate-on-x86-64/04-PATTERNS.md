# Phase 4: CI Gate on x86_64 - Pattern Map

**Mapped:** 2026-10-20
**Files analyzed:** 6 (4 new, 1 deleted, 1 new UAT doc)
**Analogs found:** 4 / 6 (others use the RESEARCH.md prototypes, which were run locally)

## File Classification

| File | Role | Data Flow | Closest Analog | Match Quality |
|------|------|-----------|----------------|---------------|
| `.github/workflows/ci.yml` (new) | config (CI) | batch | `.github/workflows/actions-rs.yml` (tracked; inverse example) + `rust-toolchain.toml` | role-match |
| `.github/workflows/actions-rs.yml` (delete) | config | - | - | n/a |
| `scripts/ci-guards.sh` (new) | utility (guard) | batch / static scan | grep gates in `.planning/phases/02-http-api-port-error-handling/02-0{1,2}-PLAN.md` acceptance criteria | partial (same commands, now in a script) |
| `scripts/check-workflow.py` (new) | utility (validator) | transform (YAML to assertions) | none | none, use the RESEARCH prototype |
| `scripts/ci-smoke.sh` (new) | test (smoke) | request-response | live-smoke `<automated>` in `02-01-PLAN.md:262`, `02-02-PLAN.md:188` | exact pattern, different harness |
| `.planning/phases/04-ci-gate-on-x86-64/04-UAT.md` (new) | doc | - | `.planning/phases/03-javascript-runtime-port/03-UAT.md` | exact |

No `scripts/` dir exists yet. All analogs named here are git-tracked: `actions-rs.yml`, `Cargo.toml` and `rust-toolchain.toml` were checked with `git ls-files`.

## Pattern Assignments

### `.github/workflows/ci.yml`

**Analog:** `.github/workflows/actions-rs.yml`, as the anti-pattern. Every piece of it gets replaced:
```yaml
on: [push, pull_request]          # keep both triggers (D-03); use map form
env:
  SERVER.HOST: 0.0.0.0            # FORBIDDEN now (CI-02, guard g)
  SERVER.PORT: 8080
    runs-on: ubuntu-latest        # -> ubuntu-24.04
        uses: actions/checkout@v2 # -> @v7
        uses: actions-rs/toolchain@v1   # -> dtolnay/rust-toolchain@1.99.1
        uses: actions-rs/cargo@v1
        continue-on-error: true  # WARNING ...   # FORBIDDEN (all 4 steps had it)
          args: -- -D warnings    # -> --all-targets --locked -- -D warnings
```
**Toolchain source of truth:** `rust-toolchain.toml` (lines 1-4):
```toml
[toolchain]
channel = "1.99.1"
components = ["rustfmt", "clippy"]
profile = "minimal"
```
The action ref `@1.99.1` and `components: rustfmt, clippy` must mirror this file.

**Body:** copy `04-RESEARCH.md` §Code Examples `ci.yml` (lines 240-296) as written. Keep the D-07 comment free of the tokens `SERVER.` and `continue-on-error`, because guard (g) greps comments too. Invoke the scripts as `bash scripts/...`.

### `scripts/ci-guards.sh`

**Analog:** the acceptance-criteria grep gates in the phase 02 plans. These are the regexes to carry over:
- Suppression: `02-02-PLAN.md:234` `! grep -rnE '#!?\[ *(allow|expect) *\(|cfg_attr\([^)]*(allow|expect)' src/config.rs`. Widen it to `src/` and the other rs roots (guard a).
- Feature edge (CONTEXT code_context; 03-VALIDATION): `cargo tree -e features,normal -i deno_core | grep -c unsafe_use_unprotected_platform`. In the script this becomes guard (d), with `--locked --target all`.
- The non-test unwrap check (`02-01-PLAN.md:275`, awk up to `#[cfg(test)]`) is superseded by guard (c), which checks the lint levels with tomllib.

**Cargo.toml region guard (c)/(d) depend on** (lines 26-31, then `[lints.rust]`):
```toml
[dev-dependencies]
# Unprotected V8 platform for test builds only, so libtest's threads can create isolates.
# Phase 4 fallback (research question 1): move this feature to [dependencies] if x86 CI crashes.
deno_core = { version = "0.412.0", features = ["unsafe_use_unprotected_platform"] }
```
**Body:** copy RESEARCH lines 299-386 and apply both resolved open questions:
1. Guard (g): if `python3 -c 'import yaml'` fails **and** `$CI` is set, print `skip [g-structure] PyYAML unavailable` and do not fail. Locally the check stays mandatory.
2. Extend the (g) forbidden regex to `'continue-on-error|SERVER\.|RUSTFLAGS|cap-lints|\$\{\{ *github\.event\.'`.

Style: `set -euo pipefail`, a `fail`/`pass` pair with `[a]`..`[h]` tags, collect every failure, then exit 1 once at the end.

### `scripts/check-workflow.py`
No analog. Copy RESEARCH lines 394-469 verbatim. Keep the `wf.get("on", wf.get(True))` handling (PyYAML treats a bare `on` key as the boolean `True` under YAML 1.1). Its dependencies are PyYAML and the standard library only.

### `scripts/ci-smoke.sh`

**Analog:** `02-01-PLAN.md:262` (live smoke):
```bash
env -i PATH="$PATH" SERVER.HOST=127.0.0.1 SERVER.PORT=18460 target/debug/rs3 >/dev/null 2>&1 & P=$!
W=$(curl -s --retry 30 --retry-connrefused --retry-delay 1 http://127.0.0.1:18460/)
... kill $P; test "$W" = '"Welcome to rs3"'
```
What changes in the script:
- Use `target/release/rs3` (overridable with `RS3_BIN`).
- Pick a free port with Python instead of using a fixed one. Port 18081 collides on this host.
- Install `trap cleanup EXIT` and write the server log to a mktemp file.
- Fail fast if the server dies during startup (`kill -0`, `wait` status 139 = segfault).
- Keep the `env -i` form, because bash cannot `export` dotted names.

Body: copy RESEARCH lines 473-529. The asserted bodies are `"Welcome to rs3"`, `"120"` and `{"error":"js_error","message":"Uncaught Error: boom"}` with status 422 (src/http/mod.rs:27-29, 166-173, 222; src/http/error.rs:62,74).

### `04-UAT.md`
**Analog:** `03-UAT.md`, which uses the same pending-item format. Add one pending item for D-09: push `modernize-toolchain`, confirm the `ci` workflow is green on ubuntu-24.04 x86_64, and record the V8 platform behaviour. If it crashes, apply the D-07 fallback: move the feature in Cargo.toml lines 26-29 into `[dependencies]` and relax guard (d) in the same commit.

## Shared Patterns

- **Logic in scripts, workflow only sequences them.** Every `run:` line must also run locally from the repo root, and the scripts `cd` to `$(dirname BASH_SOURCE)/..`.
- **`--locked` on every cargo command**, matching the phase 02/03 gates (`cargo build --locked`, `cargo test --locked`).
- **Environment only inline via `env -i`**, never workflow `env:`. The dotenv file must never be named, because a project hook blocks it.
- **Scripts get mode 100755** (`chmod +x` before `git add`; check with `git ls-files -s scripts/`), yet CI still calls them through `bash`.
- **Keep scratch work under `target/tmp/`**, because `/tmp` is nearly full. The planted-violation recipe is in RESEARCH lines 600-610.

## No Analog Found

| File | Role | Data Flow | Reason |
|------|------|-----------|--------|
| `scripts/check-workflow.py` | utility | transform | No Python in the repo. Use the RESEARCH prototype |
| `.github/workflows/ci.yml` (structure) | config | batch | The only existing workflow is the anti-pattern. Use the RESEARCH skeleton |

## Metadata
**Search scope:** `.github/`, `Cargo.toml`, `rust-toolchain.toml`, `.planning/phases/02-*/*PLAN.md`, `.planning/phases/03-*/`
**Pattern extraction date:** 2026-10-20
