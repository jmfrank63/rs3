---
phase: 06-native-deployment-documentation
fixed_at: 2026-10-21T11:21:16Z
review_path: .planning/phases/06-native-deployment-documentation/06-REVIEW.md
iteration: 1
findings_in_scope: 7
fixed: 7
skipped: 0
status: all_fixed
---

# Phase 6: Code Review Fix Report

**Fixed at:** 2026-10-21T11:21:16Z
**Source review:** .planning/phases/06-native-deployment-documentation/06-REVIEW.md
**Iteration:** 1

**Summary:**
- Findings in scope: 7 (fix scope: all)
- Fixed: 7
- Skipped: 0

All fixes are documentation or unit-file changes. No `src/` finding existed. The fixes respect
06-CONTEXT: there is no `MemoryDenyWriteExecute`, the env(1) bridge and `@pkey` are kept, no
packaging was added, and the word "docker" was not reintroduced.

## Fixed Issues

### WR-01: The documented V8 stack-overflow stdout line has the wrong text and the wrong timing

**Files modified:** `docs/js-ops.md`
**Commit:** 92510e5
**Applied fix:** The paragraph now quotes the real line as an example:
`<unknown>:-1: Uncaught RangeError: Maximum call stack size exceeded`. It notes that the line has
no `js stderr: ` prefix and that the source location varies. It also says the line goes through
C stdio buffering, so under systemd these lines can reach the journal late, in batches, or not at
all if the process aborts. The old wording "unprefixed" and "once per failed start" is gone.

### WR-02: The README says CI runs `scripts/test-ci-guards.sh`, but that script is local-only

**Files modified:** `Readme.md`
**Commit:** 2b2cb0b
**Applied fix:**
- The CI list now holds only what `.github/workflows/ci.yml` runs: fmt, clippy, test, the release
  build, `ci-guards.sh` and `ci-smoke.sh`.
- A note says that `check-workflow.py` runs inside `ci-guards.sh` as guard (g).
- `bash scripts/test-ci-guards.sh` moved to its own block: "Also run the guard self-test locally;
  CI does not run it". It stays in the Development section, as D-12 item 10 requires.

### WR-03: The security note treats "loopback" as private

**Files modified:** `Readme.md`, `deploy/rs3.conf`, `deploy/rs3.service`
**Commit:** 21307c1
**Applied fix:**
- **README security note.** A new paragraph says that loopback is not private: every local user
  and process can still reach rs3, even behind a reverse proxy, and so can web pages in a browser
  on the same host (`POST /insert` accepts any Content-Type, and there is no `Host` check). It
  says to run rs3 only on a host where you trust all of them, and points to the optional
  allow-list.
- **rs3.conf.** The comment notes that even 127.0.0.1 is open to local users and browsers.
- **rs3.service.** It gains the allow-list only as commented-out example lines, because it cannot
  be verified without root (D-11):
  ```ini
  # Optional: refuse every peer except loopback, whatever RS3_HOST says.
  #IPAddressDeny=any
  #IPAddressAllow=localhost
  ```

### WR-04: The "hardened" unit has no memory ceiling

**Files modified:** `deploy/rs3.service`, `Readme.md`
**Commit:** d6ff4c6
**Applied fix:**
- **rs3.service.** It now sets `MemoryMax=1G` in `[Service]`, with the comment "No V8 heap limit
  yet (v2): cap the whole service ... Size to the server."
- **README.** The "Service user and sandbox" section gains a paragraph explaining the cap, that
  a kill at the cap stays inside rs3's cgroup and restarts rs3 with an empty store, and that the
  value should be sized to the server.
- **Live check.** Under the user manager, a transient `systemd-run --user -p MemoryMax=1G`
  started the release binary through the env(1) bridge and answered `"Welcome to rs3"`.
  `memory.max` for its cgroup read `1073741824`. The unit was then stopped and collected.

### IN-01: The start limit turns client-triggerable crashes into a permanent outage

**Files modified:** `Readme.md`
**Commit:** 3067034
**Applied fix:** This uses the review's primary fix. The Troubleshooting paragraph now names an
OOM kill at `MemoryMax=` as a crash too. It states that any client can trigger such a crash, and
that a client that crashes rs3 about six times within 60 s takes it down until an operator runs
`reset-failed` and `restart`. `RestartSteps=` and `RestartMaxDelaySec=` were deliberately not
added: they need systemd 254 or later, and the unit must stay valid on older systemd.

### IN-02: "The environment wins over such a file anyway" overstates how harmless `/.env` is

**Files modified:** `Readme.md`
**Commit:** 60b7b9b
**Applied fix:** The sentence now reads: "The environment wins over the values in such a file, but
a malformed or unreadable `/.env` still stops startup." This was checked against
`src/config.rs`: `from_sources` ignores only a not-found error, and any other dotenv error is
returned.

### IN-03: The Update section does not cover a changed unit or environment file

**Files modified:** `Readme.md`
**Commit:** cf73be6
**Applied fix:** A sentence was added after the Update block: "If `deploy/rs3.service` changed,
re-install it and run `sudo systemctl daemon-reload` before the restart. Do not re-install
`rs3.conf` over your edited copy."

## Verification

**Where it ran:**
- **Per-fix checks.** Re-reading each edit, and running `systemd-analyze verify` after each unit
  edit, happened in the isolated worktree `.claude/worktrees/rf-06-*` on a temporary branch.
- **Gates below.** These ran in the main checkout
  (`/home/t3code/rs3/Projects/rs3`, branch `modernize-toolchain` at `cf73be6`) after the branch
  was fast-forwarded and the worktree removed. They are reproducible from the main tree. The
  scratch helpers need `target/release/rs3`, which exists only in the main checkout. `src/` is
  unchanged, so that binary is current.

| Check | Result |
|-------|--------|
| `systemd-analyze verify --recursive-errors=no deploy/rs3.service` | empty output, exit 0 (systemd 255) |
| `systemd-analyze security --offline=true deploy/rs3.service \| tail -1` | `→ Overall exposure level for rs3.service: 1.5 OK :-)` (also 1.5 before the fixes) |
| `bash target/tmp/p6/unit-live.sh bridge` | `unit-live: OK bridge`, exit 0 (banner, `GET /`, PATCH `"120"`, bridge environ, NoNewPrivs/Seccomp) |
| `bash target/tmp/p6/readme-replay.sh` | `readme-replay: OK (130 checks)`, exit 0, no FAIL lines |
| `git grep -n -i docker -- . ':!.planning' ':!aws' ':!Vagrantfile' ':!experiments'` | prints nothing (exit 1) |
| `bash scripts/ci-guards.sh` | `ci-guards: all guards passed`, exit 0 |
| `git diff --stat 33336a6..HEAD -- Cargo.lock .github scripts src` | empty: unchanged |
| MemoryMax live check (transient user unit) | `memory.max` = `1073741824`, `GET /` 200; unit stopped and collected |

Neither scratch helper needed a change. No README wording that they check was altered, and the
HTTP bodies they assert are untouched. No rs3 process or transient unit is left running.
Scratch output under `target/tmp/rf06` was removed.

**Process notes:**
- Commits were made with `git commit -- <paths>` and the required author/committer environment
  variables, one commit per finding. The commit subjects use the `fix(06): <ID> ...` style and end
  with the Co-Authored-By trailer.
- The worktree cleanup ran in this order: fast-forward, worktree remove, temporary branch
  delete, recovery sentinel removed.

---

_Fixed: 2026-10-21T11:21:16Z_
_Fixer: Claude (gsd-code-fixer)_
_Iteration: 1_
