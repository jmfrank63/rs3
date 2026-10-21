---
phase: 06-native-deployment-documentation
fixed_at: 2026-10-21T11:35:01Z
review_path: .planning/phases/06-native-deployment-documentation/06-REVIEW.md
iteration: 3
findings_in_scope: 2
fixed: 2
skipped: 0
status: all_fixed
---

# Phase 6: Code Review Fix Report (iteration 3)

**Fixed at:** 2026-10-21T11:35:01Z
**Source review:** .planning/phases/06-native-deployment-documentation/06-REVIEW.md (iteration 3)
**Iteration:** 3 (final iteration of the --auto loop)

**Summary:**
- Findings in scope: 2 (fix scope: all)
- Fixed: 2 (WR-01 is marked "fixed: requires human verification" because it changes a sizing rule)
- Skipped: 0

Both fixes change only documentation and one unit-file comment. The unit's directives are
unchanged: `MemoryMax=1G` and `MemorySwapMax=0` stay. The fixes respect 06-CONTEXT: there is still
no `MemoryDenyWriteExecute`, the env(1) bridge and `@pkey` are kept, no packaging was added, and
the word "docker" was not reintroduced. The only numbers added are the ones the iteration-3 review
measured (about 4× the stored data per concurrent `/list` or `op_rs3_list()`) and the per-runtime
cost of about 5 MB that was already in the README. "About 60 MB" is plain arithmetic from the stated
bound (1 GiB / 17 ≈ 60 MiB).

## Fixed Issues

### WR-01: The `MemoryMax=` sizing rule underestimates memory by about 4× the store per concurrent `/list` or `op_rs3_list`

**Status:** fixed: requires human verification
**Files modified:** `Readme.md`, `deploy/rs3.service`
**Commit:** 9a1e3b2
**Applied fix:**
- **README ("Service user and sandbox").** The sizing rule is now its own paragraph. It reads:
  "Size the value to the server. The stored data counts against the cap. On top of it, budget
  about 5 MB per running script (up to 4 per PATCH, one PATCH per CPU), and about 4× the stored
  data for each `GET /list` or `op_rs3_list()` that runs at the same time (up to one per CPU),
  because each one copies the whole store. These are rough measurements from a release build. So
  keep the stored data well below MemoryMax / (1 + 4 × CPUs): with the shipped 1G on a 4-CPU
  server, that is about 60 MB. On a 64-CPU server, full nested load alone needs more than the
  shipped 1G."
- **rs3.service comment** (lines 22-24). It now reads: "No V8 heap limit yet (v2): cap the whole
  service so rs3, not the host, is OOM-killed and restarted (with an empty store). Size to the
  server and the stored data (see "Service user and sandbox" in Readme.md)." The old comment named
  "a runaway script" as the only trigger. The new one points operators to the README rule, as the
  review suggested.

### IN-01: The `MemoryMax=` paragraph states the kill-and-empty consequence twice

**Files modified:** `Readme.md`
**Commit:** d738458
**Applied fix:** The two sentences are now one: "Reaching the cap kills rs3 inside its own cgroup
instead of other services on the host, and rs3 restarts with an empty store." It ends the
cap/swap paragraph. The trailing duplicate after the sizing rule was removed. The sizing rule
starts a new paragraph, and the text was reflowed.

## Verification

**Where it ran:** in the main checkout (`/home/t3code/rs3/Projects/rs3`, branch
`modernize-toolchain`, at `d738458`). No worktree was used, as the orchestrator instructed. All
results can be reproduced from the main tree. `src/` is unchanged, so `target/release/rs3` is
current.

| Check | Result |
|-------|--------|
| `systemd-analyze verify --recursive-errors=no deploy/rs3.service` | empty output, exit 0 (systemd 255) |
| `systemd-analyze security --offline=true deploy/rs3.service \| tail -1` | `→ Overall exposure level for rs3.service: 1.5 OK :-)` (unchanged) |
| `bash target/tmp/p6/unit-live.sh bridge` | `unit-live: OK bridge`, exit 0 (log: `target/tmp/p6/bridge-iter3.out`) |
| `bash target/tmp/p6/readme-replay.sh` | `readme-replay: OK (130 checks)`, exit 0, no FAIL lines (log: `target/tmp/p6/replay-iter3.out`) |
| `git grep -n -i docker -- . ':!.planning' ':!aws' ':!Vagrantfile' ':!experiments'` | prints nothing (exit 1) |
| `bash scripts/ci-guards.sh` | `ci-guards: all guards passed`, exit 0 |
| `git diff --stat ef9ab51..HEAD -- Cargo.lock .github scripts src` and `git status` on them | empty: unchanged |
| `git diff --stat ef9ab51..HEAD` | only `Readme.md` and `deploy/rs3.service` |

**Notes:**
- The scratch helpers do not assert anything in the `MemoryMax=` paragraph, so neither needed a
  change.
- The memory figures were not measured again in this iteration. They come from the iteration-3
  review's measurements and are labelled in the README as rough.
- No rs3 process or transient unit is left running (`pgrep` and `systemctl --user list-units`
  show nothing).

**Process notes:** Each finding was committed separately with `git commit -- <paths>` and the
required author/committer environment variables. The subjects use the `fix(06): <ID> ...` style
and end with the Co-Authored-By trailer. The IN-01 commit was amended once, before anything else
built on it, to fold in the reflow of the paragraph it had just edited. Nothing was pushed, and
`git config` was not touched.

## History: iteration 2 (all fixed)

The full iteration-2 report is backed up at `06-REVIEW-FIX.iter3.md`.

| ID | Title | Commit |
|----|-------|--------|
| WR-01 | `MemoryMax=` alone does not OOM-kill rs3 on a host with swap (`MemorySwapMax=0`) | b69b176 |
| IN-01 | The README gives no basis for sizing `MemoryMax=` (this rule is corrected by iteration-3 WR-01) | ef9ab51 |

## History: iteration 1 (all fixed)

The full iteration-1 report is backed up at `06-REVIEW-FIX.iter2.md`. The iteration-2 and
iteration-3 reviews confirmed all seven of these fixes as resolved.

| ID | Title | Commit |
|----|-------|--------|
| WR-01 | The documented V8 stack-overflow stdout line has the wrong text and the wrong timing (`docs/js-ops.md`) | 92510e5 |
| WR-02 | The README says CI runs `scripts/test-ci-guards.sh`, but that script is local-only | 2b2cb0b |
| WR-03 | The security note treats "loopback" as private (`Readme.md`, `deploy/rs3.conf`, `deploy/rs3.service`) | 21307c1 |
| WR-04 | The "hardened" unit has no memory ceiling (`MemoryMax=1G`) | d6ff4c6 |
| IN-01 | The start limit turns client-triggerable crashes into a permanent outage | 3067034 |
| IN-02 | "The environment wins over such a file anyway" overstates how harmless the dotenv file at `/` is | 60b7b9b |
| IN-03 | The Update section does not cover a changed unit or environment file | cf73be6 |

---

_Fixed: 2026-10-21T11:35:01Z_
_Fixer: Claude (gsd-code-fixer)_
_Iteration: 3_
