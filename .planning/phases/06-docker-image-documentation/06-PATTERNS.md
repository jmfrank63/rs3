# Phase 6: Docker Image & Documentation - Pattern Map

**Mapped:** 2026-10-21
**Files analyzed:** 11
**Analogs found:** 10 / 11 (all analogs are git-tracked; verified with `git ls-files`)

## File Classification

| New/Modified File | Role | Data Flow | Closest Analog | Match Quality |
|-------------------|------|-----------|----------------|---------------|
| `Dockerfile` (rewrite) | config | batch (build) | legacy `Dockerfile` (anti-pattern) + 06-RESEARCH "Code Examples: Dockerfile" | anti-pattern / prototype |
| `.dockerignore` (new) | config | file-I/O (context filter) | none in repo; 06-RESEARCH ".dockerignore" prototype | no analog |
| `scripts/docker-smoke.sh` (new) | test (smoke) | request-response | `scripts/ci-smoke.sh` | exact |
| `.github/workflows/ci.yml` (+ `docker` job) | config | batch | existing `ci` job in same file | exact |
| `scripts/check-workflow.py` | utility (contract check) | transform | itself: `CANONICAL` (l.37+), `structure_errors` (l.136-234) | exact |
| `scripts/ci-guards.sh` (+ guard (i)) | utility (guard) | transform | guards (g) l.200-232 and (h) l.234-252 | exact |
| `scripts/test-ci-guards.sh` (+ planted cases) | test | batch | `fresh()` l.35-46, `expect_fail` l.58-78, cases l.158+ | exact |
| `Readme.md` (rewrite) | docs | n/a | `docs/js-ops.md` (voice/structure); current `Readme.md` (content to replace) | role-match |
| `docs/js-ops.md` (IN-03, IN-02 fixes) | docs | n/a | itself, "Sandbox and known limits" l.195+, ops table l.16-36 | exact |
| `src/js/executor.rs` (IN-05 comment only, l.147-148) | service | n/a | itself | exact |
| `06-UAT.md` | docs (UAT) | n/a | `.planning/phases/05-*/05-UAT.md` (commit 42fc853) | exact |

## Pattern Assignments

### `scripts/docker-smoke.sh` (smoke, request-response)

**Analog:** `scripts/ci-smoke.sh`

**Header + env-knob pattern** (l.1-5, 43-48): a comment block listing every assert as `METHOD PATH -> CODE "exact body"`, then:
```bash
set -euo pipefail
ROOT="${CI_SMOKE_ROOT:-$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)}"
cd "$ROOT"
PORT="${RS3_SMOKE_PORT:-$(python3 -c 'import socket; s=socket.socket(); s.bind(("127.0.0.1", 0)); print(s.getsockname()[1])')}"
BASE="http://127.0.0.1:${PORT}"
LOG="$(mktemp)"
```
Rename knobs to `DOCKER_SMOKE_ROOT`, add `RS3_IMAGE` (default `rs3:ci`).

**Cleanup trap** (l.52-66): `cleanup() {...}; trap cleanup EXIT`. For Docker replace the kill/SIGKILL loop with `docker rm -f "$CID" >/dev/null 2>&1 || true`; keep `rm -f "$LOG"`.

**fail()** (l.88-94): prefix `docker-smoke: FAIL:`, dump `docker inspect -f '{{.State.Status}} {{.State.ExitCode}}'` and `docker logs` instead of `$LOG`.
```bash
fail() {
  server_exit_note
  echo "ci-smoke: FAIL: $*${NOTE}" >&2
  echo "--- server log ---" >&2
  cat "$LOG" >&2 || true
  exit 1
}
```

**Port precheck + config-in-script** (l.96-102): keep the `/dev/tcp` port-in-use check; config goes on the `docker run` line (`-e SERVER.HOST=0.0.0.0 -p 127.0.0.1:${PORT}:8080`), never in the workflow (Research Pattern 3, Pitfalls 2-3).

**Readiness poll, `req` helper, exact-body asserts:** copy the remaining ci-smoke structure (readiness loop from l.104, request helper that splits CODE/BODY). Full verified prototype: 06-RESEARCH "scripts/docker-smoke.sh" (l.358-460) — use it verbatim.

---

### `Dockerfile` (config, build)

**Anti-pattern analog:** legacy `Dockerfile` (31 lines). Do NOT carry over:
```dockerfile
ARG DISTRO
FROM rust:1.53 as planner        # old toolchain, cargo-chef installed per stage
RUN cargo install cargo-chef
COPY --from=cacher $CARGO_HOME $CARGO_HOME   # separate cook stage copying target/ + CARGO_HOME
RUN cargo install --path .        # ignores Cargo.lock (no --locked)
FROM ${DISTRO}                    # untestable runtime, glibc mismatch risk
```
**Replace with:** 06-RESEARCH "Dockerfile" prototype (l.307-345): `lukemathwalker/cargo-chef:latest-rust-1.99.1-trixie` chef base -> planner (`cargo chef prepare`) -> builder (`cargo chef cook --release --locked`, then `cargo build --release --locked --bin rs3`) -> `gcr.io/distroless/cc-debian13:nonroot`, `USER nonroot:nonroot`, exec-form `ENTRYPOINT`, env defaults for `SERVER.PORT`. Keep its header comment style (purpose + which guard/job checks it), matching scripts' header comments.

---

### `.dockerignore` (new)
No analog. Use 06-RESEARCH prototype (l.347-356) verbatim: line 1 `*`, then `!` re-includes kept in sync with Dockerfile COPY lines; `rust-toolchain.toml` excluded (Pitfall 1).

---

### `.github/workflows/ci.yml` (+ `docker` job)

**Analog:** existing `ci` job (same file). Step shape to copy, including the explanatory comment-above-step convention:
```yaml
      - name: Checkout
        uses: actions/checkout@v7
      ...
      - name: Release smoke
        run: bash scripts/ci-smoke.sh
```
New job: `needs: ci`, `runs-on: ubuntu-26.04`, `timeout-minutes`, checkout + `run: bash scripts/docker-smoke.sh`. No `SERVER.`, no new `${{ }}`, no `continue-on-error`. Exact YAML: 06-RESEARCH l.461+.

---

### `scripts/check-workflow.py`

**Analog:** itself.
- `CANONICAL` (l.37-~95): append the docker job's functional lines in the same change (docstring l.20: "EDITING ci.yml MEANS UPDATING CANONICAL IN THE SAME CHANGE").
- `structure_errors` l.176-180 hardcodes one job:
```python
    jobs = wf.get("jobs") or {}
    if not isinstance(jobs, dict) or list(jobs) != ["ci"]:
        errs.append(f"exactly one job 'ci' expected, got ...")
    job = jobs.get("ci") or {}
```
Refactor into a per-job spec (`JOBS = {"ci": {...}, "docker": {...}}`) and a `job_errors(name, job, spec)` that reuses l.181-233 checks (runs-on, timeout, FORBIDDEN_KEYS, JOB_KEYS/STEP_KEYS, EXPECTED_USES, REQUIRED_RUNS exact order); add `needs: ci` check for docker. Error-message format: `errs.append(f"...")`, returned list.

---

### `scripts/ci-guards.sh` (+ guard (i))

**Analog:** guard (h) l.234-252 (accumulate errors, single pass/fail):
```bash
h_errs=""
...
if (( ${#cfg[@]} )); then h_errs+=$'\n'"repo cargo config present: ${cfg[*]}"; fi
...
if [[ -z $h_errs ]]; then pass h "no .cargo/config*, no rustfmt config, no rustfmt::skip"
else fail h "${h_errs#$'\n'}"; fi
```
Helpers `fail TAG MSG` / `pass TAG MSG` (l.11-12); tags print in order, so (i) goes after (h) and before the final `if (( failures ))` (l.254). Update the header comment l.4 (`[a]..[h]` -> `[a]..[i]`). For tool-exit handling copy the `&& rc=0 || rc=$?` + `case $rc` fail-closed idiom (l.243-250). Guard (i) asserts per D-08 (builder tag version == `rust-toolchain.toml` channel, no `-slim`, trixie/debian13 pairing, `--locked`, no `ARG DISTRO`, exec-form ENTRYPOINT, non-root USER, `.dockerignore` starts with `*`, `bash -n scripts/docker-smoke.sh`).

---

### `scripts/test-ci-guards.sh`

**Analog:** itself.
- `fresh()` l.35-46: add `Dockerfile .dockerignore` to the `cp -r` list (otherwise guard (i) fails on every case).
```bash
    && cp -r Cargo.toml Cargo.lock rust-toolchain.toml clippy.toml src requests .github scripts \
```
- Planted cases: `expect_fail i 'LABEL' 'SNIPPET' [EXTRA]` (definition l.58-78; examples l.158-192), e.g. `expect_fail i 'slim builder' 'sed -i "s/-trixie/-slim-trixie/" Dockerfile'`. Workflow-contract cases for the docker job use `expect_wf_fail` (l.80+). Pitfall 5: toolchain-bump plant must edit both files consistently.

---

### `Readme.md` / `docs/js-ops.md`

**Analog:** `docs/js-ops.md` — `#` title, short intro paragraph, `##` sections, tables with `| Old call | New call | ... |` (l.16-36), backticked identifiers, plain declarative prose. README summarizes and links to `docs/js-ops.md` rather than duplicating the table's full "Throws when" column.
Current `Readme.md` (23 lines, malformed `###Running the server` heading) is fully replaced; keep file name.
`docs/js-ops.md` l.199-207: fix IN-03 — the sentence "so a script cannot fake server log lines" must be scoped to stderr (stdout prints are raw). IN-02: fill ops table gaps / limits wording (l.195+).
Docker run example in docs: `-p 127.0.0.1:8080:8080` (Research anti-pattern), dotted env via `-e SERVER.HOST=0.0.0.0`.

---

## Shared Patterns

### Script header + env knobs
**Source:** `scripts/ci-smoke.sh` l.1-5, `scripts/ci-guards.sh` l.1-5
**Apply to:** docker-smoke.sh, guard (i) comment. Header states purpose, decision ID (D-xx), env knobs with defaults, and assert list.

### Fail-closed error accumulation
**Source:** `scripts/ci-guards.sh` l.11-26, 234-252
**Apply to:** guard (i), check-workflow.py job_errors. Every check runs; all errors reported; tool errors (exit >= 2) count as failures.

### Config only in scripts, never in workflow
**Source:** `scripts/ci-smoke.sh` l.99-102 (`env -i ... SERVER.HOST=...`)
**Apply to:** docker-smoke.sh (`docker run -e SERVER.HOST=0.0.0.0`), ci.yml (no `SERVER.`).

### Contract co-change
**Source:** `scripts/check-workflow.py` docstring l.7-20
**Apply to:** any ci.yml edit -> CANONICAL + structure spec + test-ci-guards.sh in the same plan/commit.

## No Analog Found

| File | Role | Data Flow | Reason |
|------|------|-----------|--------|
| `.dockerignore` | config | file-I/O | No ignore-allowlist files exist; use 06-RESEARCH prototype |

## Metadata

**Analog search scope:** `scripts/`, `docs/`, `.github/workflows/`, root `Dockerfile`, `Readme.md`
**Files scanned:** 8
**Pattern extraction date:** 2026-10-21
