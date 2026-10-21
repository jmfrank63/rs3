---
phase: 06-native-deployment-documentation
fixed_at: 2026-10-21T11:26:42Z
review_path: .planning/phases/06-native-deployment-documentation/06-REVIEW.md
iteration: 2
findings_in_scope: 2
fixed: 2
skipped: 0
status: all_fixed
---

# Phase 6: Code Review Fix Report (iteration 2)

**Fixed at:** 2026-10-21T11:26:42Z
**Source review:** .planning/phases/06-native-deployment-documentation/06-REVIEW.md (iteration 2)
**Iteration:** 2

**Summary:**
- Findings in scope: 2 (fix scope: all)
- Fixed: 2
- Skipped: 0

Both fixes change only documentation or the unit file. They respect 06-CONTEXT: there is still no
`MemoryDenyWriteExecute`, the env(1) bridge and `@pkey` are kept, no packaging was added, and the
word "docker" was not reintroduced. `MemorySwapMax=` dates from systemd 232, so the unit stays
valid on older systemd.

## Fixed Issues

### WR-01: `MemoryMax=` alone does not OOM-kill rs3 on a host with swap; it pushes rs3 into the host's swap instead

**Files modified:** `deploy/rs3.service`, `Readme.md`
**Commit:** b69b176
**Applied fix:**
- **rs3.service.** `MemorySwapMax=0` now follows `MemoryMax=1G`, with the comment "Without this,
  rs3 would be swapped out past MemoryMax= instead of killed."
- **README** ("Service user and sandbox"). The paragraph now opens with "`MemoryMax=1G` together
  with `MemorySwapMax=0` caps the whole service". It also says that `MemorySwapMax=0` matters on
  a host with swap, because without it rs3 would be swapped out past the cap instead of killed.
- **Live check.** A transient user unit with `-p MemoryMax=1G -p MemorySwapMax=0` was accepted.
  Its cgroup read `memory.max` = `1073741824` and `memory.swap.max` = `0`. The unit was stopped
  and collected.

### IN-01: The README gives no basis for sizing `MemoryMax=`, and the cap also bounds the store

**Files modified:** `Readme.md`
**Commit:** ef9ab51
**Applied fix:** "Size the value to the server" now continues: "budget about 5 MB per running
script (up to 4 per PATCH, one PATCH per CPU) plus the data you expect to store, because the store
counts against the same cap. On a 64-CPU server, full nested load alone needs more than the
shipped 1G. Reaching the cap kills rs3 and empties the store." The numbers were checked against
the README's own Limits section: one JS worker per available CPU, and a depth limit of 4 that
counts the PATCHed key.

## Verification

**Where it ran:** in the main checkout (`/home/t3code/rs3/Projects/rs3`, branch
`modernize-toolchain`, at `ef9ab51`). No worktree was used, as the orchestrator instructed. All
results can be reproduced from the main tree. `src/` is unchanged, so `target/release/rs3` is
current.

| Check | Result |
|-------|--------|
| `systemd-analyze verify --recursive-errors=no deploy/rs3.service` | empty output, exit 0 (systemd 255) |
| `systemd-analyze security --offline=true deploy/rs3.service \| tail -1` | `→ Overall exposure level for rs3.service: 1.5 OK :-)` (unchanged) |
| `bash target/tmp/p6/unit-live.sh bridge` | `unit-live: OK bridge`, exit 0 |
| `bash target/tmp/p6/readme-replay.sh` | `readme-replay: OK (130 checks)`, exit 0, no FAIL lines |
| `git grep -n -i docker -- . ':!.planning' ':!aws' ':!Vagrantfile' ':!experiments'` | prints nothing (exit 1) |
| `bash scripts/ci-guards.sh` | `ci-guards: all guards passed`, exit 0 |
| `git diff --stat 33336a6..HEAD -- Cargo.lock .github scripts src` | empty: unchanged |
| MemorySwapMax live check (transient user unit) | `memory.max` = `1073741824`, `memory.swap.max` = `0`; unit stopped and collected |

**Notes on the checks:**
- `unit-live.sh` forwards only a fixed list of unit keys, and that list does not include
  `MemoryMax`/`MemorySwapMax`. That is why the separate transient-unit check above was run.
- This host has no swap, so the swap-versus-kill behaviour itself could not be shown live. Only
  the cgroup setting was checked.
- Neither scratch helper needed a change, and the HTTP bodies they assert are untouched.
- No rs3 process or transient unit is left running.

**Process notes:** Each finding was committed separately with `git commit -- <paths>` and the
required author/committer environment variables. The subjects use the `fix(06): <ID> ...` style
and end with the Co-Authored-By trailer. Nothing was pushed, and `git config` was not touched.

## History: iteration 1 (all fixed)

The full iteration-1 report is backed up at `06-REVIEW-FIX.iter2.md`. The iteration-2 review
confirmed all seven of these fixes as resolved.

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

_Fixed: 2026-10-21T11:26:42Z_
_Fixer: Claude (gsd-code-fixer)_
_Iteration: 2_
