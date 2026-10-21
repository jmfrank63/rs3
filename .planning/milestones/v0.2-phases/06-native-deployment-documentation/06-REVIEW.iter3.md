---
phase: 06-native-deployment-documentation
reviewed: 2026-10-21T11:25:00Z
depth: standard
files_reviewed: 8
files_reviewed_list:
  - .gitignore
  - Cargo.toml
  - MODERNIZATION_BRIEF.md
  - Readme.md
  - deploy/rs3.conf
  - deploy/rs3.service
  - docs/js-ops.md
  - src/js/executor.rs
findings:
  critical: 0
  warning: 1
  info: 1
  total: 2
status: issues_found
---

# Phase 6: Code Review Report (iteration 2)

**Reviewed:** 2026-10-21T11:25:00Z
**Depth:** standard
**Files Reviewed:** 8
**Status:** issues_found

## Summary

This is a re-review of the current state of the eight files (phase diff base `2ecb06d`), after
the iteration-1 fixes in commits `92510e5..cf73be6`. I read 06-CONTEXT.md first, so deliberate
decisions (D-01..D-17) are not counted as defects. That includes the env(1) bridge, `@pkey`, the
absent `MemoryDenyWriteExecute`, and `license = "MIT"` with `LICENSE` arriving from `main`.

**All seven iteration-1 findings are resolved:**
- **WR-01.** `docs/js-ops.md:212-218` now quotes the real `<unknown>:-1: …` line, says the source
  location varies, and explains the C stdio buffering.
- **WR-02.** `Readme.md:482-501` now lists only the steps in `.github/workflows/ci.yml`.
  `check-workflow.py` is correctly placed inside guard (g), and the self-test is marked
  local-only.
- **WR-03.** `Readme.md:428-434` and `deploy/rs3.conf:69-71` state the real loopback boundary.
  The commented `IPAddressDeny=any` / `IPAddressAllow=localhost` example is correct.
- **WR-04.** `MemoryMax=1G` was added to `deploy/rs3.service:22-24`, and `Readme.md:390-393`
  explains it. See WR-01 below for a gap in that fix.
- **IN-01, IN-02, IN-03.** The Troubleshooting, Configuration-under-systemd and Update paragraphs
  now say what was asked, and the wording matches `src/config.rs` (only `NotFound` is ignored)
  and the unit.

**Checks I ran myself.** All scratch was under `target/tmp`, has been removed, and nothing is
left running:
- **`systemd-analyze verify --recursive-errors=no deploy/rs3.service`:** no output, exit 0.
- **Restart after an OOM kill.** The README's new claim that a kill at `MemoryMax=` restarts
  rs3 holds under the default `OOMPolicy=stop`. I tested it with a transient user unit
  (`MemoryMax=64M`, `Restart=on-failure`, an allocator process). systemd logged
  `Failed with result 'oom-kill'` and then `Scheduled restart job, restart counter is at 5`.
- **rs3 memory under load.** For the release binary on this 4-CPU host:
  - idle VmRSS was 11 MB;
  - with 8 concurrent PATCHes of a depth-4 chain (4 workers × 4 live isolates), VmHWM was 89 MB;
  - so each live isolate costs about 4.5 MB. IN-01 uses this number.
- **`src/js/executor.rs:147-149`.** The comment matches the production path: `main.rs:16` calls
  `init_platform` on the main thread, then `main.rs:17` calls `JsExecutor::start`.

**New issue from the fixes:** the `MemoryMax=` fix does not do what the unit comment and the
README promise on a host that has swap (WR-01).

## Narrative Findings (AI reviewer)

## Warnings

### WR-01: `MemoryMax=` alone does not OOM-kill rs3 on a host with swap; it pushes rs3 into the host's swap instead

**File:** `deploy/rs3.service:22-24`, `Readme.md:390-393`, `Readme.md:474-475`
**Issue:** The unit comment says a runaway script is "OOM-killed inside rs3's cgroup and
restarted, not the host". The README says: "If rs3 grows past the cap, the kernel kills rs3
inside its own cgroup instead of other services on the host".

On cgroup v2, `memory.max` limits RAM only. Swap is limited separately by `memory.swap.max`,
which defaults to `max`, and the unit does not set `MemorySwapMax=`. On a server with swap,
which is common on installer-built Ubuntu/Debian hosts with a swap file, this is what happens
when rs3 reaches 1G:
- **Swap, not a kill.** The kernel reclaims rs3's anonymous V8 and store pages to swap instead
  of invoking the OOM killer.
- **The host still suffers.** rs3 can keep growing until the host's swap is full. That means
  heavy swap I/O that stalls every service on the host, and global swap exhaustion that can
  then OOM-kill other services. These are exactly the outcomes the fix was meant to prevent.
- **Long stalls.** A swapping rs3 also stalls instead of failing fast. PATCH requests time out
  (504) or hang behind the 2 s backstop for long periods, rather than rs3 being killed and
  restarted.

This host has no swap, so the fixer's live check (memory.max = 1073741824 and `GET /`) could
not show the problem.
**Fix:** Turn off swap for the service, so that reaching the RAM cap really means an OOM kill.
Both `systemd-analyze verify` (systemd 255, scratch copy) and a transient unit accept the
directive:
```ini
# No V8 heap limit yet (v2): cap the whole service so a runaway script is
# OOM-killed inside rs3's cgroup and restarted, not the host. Size to the server.
MemoryMax=1G
# Without this, rs3 would be swapped out past MemoryMax= instead of killed.
MemorySwapMax=0
```
Then update the README sentence, for example: "`MemoryMax=1G` together with `MemorySwapMax=0`
caps the whole service …".

## Info

### IN-01: The README gives no basis for sizing `MemoryMax=`, and the cap also bounds the store

**File:** `Readme.md:390-393`, `deploy/rs3.service:22-24`
**Issue:** "Size the value to the server" leaves the operator guessing. Two facts that matter
are not stated:
- **JS runtimes scale with the CPU count.** There is one JS worker per available CPU, and a
  PATCH can hold up to 4 live isolates at depth 4. In the measurement above each live isolate
  cost about 4.5 MB. So a 64-CPU server under full legitimate nested load needs about
  64 × 4 × 4.5 MB ≈ 1.15 GB for runtimes alone. That exceeds the shipped 1G even with an empty
  store, and the result is an OOM kill under normal traffic.
- **The cap covers the store too.** The in-memory store counts against the same cap. Once the
  stored data plus the working runtimes reach `MemoryMax=`, rs3 is killed and every key is
  lost (the README does say a restart empties the store). Without the cap, this happened only
  when the whole host ran out of memory.

**Fix:** Add one sentence, for example: "Budget about 5 MB per running script (up to 4 per PATCH,
one PATCH per CPU) plus the data you expect to store. Reaching the cap kills rs3 and empties
the store."

---

_Reviewed: 2026-10-21T11:25:00Z_
_Reviewer: Claude (gsd-code-reviewer)_
_Depth: standard_
