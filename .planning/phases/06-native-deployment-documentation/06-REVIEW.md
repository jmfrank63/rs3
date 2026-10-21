---
phase: 06-native-deployment-documentation
reviewed: 2026-10-21T12:10:00Z
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

# Phase 6: Code Review Report (iteration 3)

**Reviewed:** 2026-10-21T12:10:00Z
**Depth:** standard
**Files Reviewed:** 8
**Status:** issues_found

## Summary

This re-review covers the current state of the eight files (phase diff base `2ecb06d`, HEAD
`ef9ab51`). It follows the iteration-2 fixes `b69b176` (`MemorySwapMax=0`) and `ef9ab51`
(`MemoryMax=` sizing guidance). I read 06-CONTEXT.md and the relevant parts of 06-RESEARCH.md
first, so I did not count these deliberate decisions as defects:
- the env(1) bridge;
- `@pkey`;
- leaving out `MemoryDenyWriteExecute`;
- `ProtectProc=invisible` without `ProcSubset=` (RESEARCH §evidence, open question 2);
- `license = "MIT"` with `LICENSE` arriving from `main`.

**Earlier findings that stay resolved:**
- **Iteration-1 findings (WR-01..WR-04, IN-01..IN-03).** The text and unit lines reviewed in
  iteration 2 are unchanged and still correct.
- **Iteration-2 WR-01 (swap).** `deploy/rs3.service:25-26` now sets `MemorySwapMax=0` with a
  correct comment, and `Readme.md:390-393` explains it.
- **Iteration-2 IN-01 (sizing).** `Readme.md:395-399` now gives a sizing rule and says the store
  counts against the cap. However, the rule's per-script figure is wrong for any script that reads
  the store (WR-01 below).

**Checks I ran myself.** All scratch went under `target/tmp/rv3` and has been removed. No rs3
process or listener is left.
- **`systemd-analyze verify --recursive-errors=no deploy/rs3.service`:** no output, exit 0
  (systemd 255).
- **`systemd-analyze security --offline=true deploy/rs3.service`:** overall exposure 1.5, OK.
- **`git grep -n -i docker -- . ':!.planning' ':!aws' ':!Vagrantfile' ':!experiments'`:** no
  output (exit 1).
- **`src/`, `Cargo.lock`, `.github` and `scripts`:** no uncommitted changes.
- **`src/js/executor.rs:147-149`:** the comment matches `src/main.rs`: `init_platform` runs on
  the main thread (line 16) before `JsExecutor::start` (line 17).
- **Memory measurements behind WR-01:** release binary, loopback, env-var config. The store held
  100 keys × 200 000-byte values (about 20 MB of data). Idle VmRSS/VmHWM was about 36 MB.

| Load | VmHWM | Extra per request |
|------|-------|-------------------|
| One `GET /list` | 113.6 MB | +78 MB, about 3.9× the stored data |
| Four concurrent `GET /list` | 332 MB | about 74 MB each |
| Four concurrent PATCHes of `Deno.core.ops.op_rs3_list().length` | 368 MB | about 83 MB per running script, about 4× the stored data |

## Narrative Findings (AI reviewer)

## Warnings

### WR-01: The new `MemoryMax=` sizing rule underestimates memory by about 4× the store per concurrent `/list` or `op_rs3_list`; an operator who follows it gets an OOM kill and loses every key under normal traffic

**File:** `Readme.md:395-399` (the rule), together with `deploy/rs3.service:24`
(`MemoryMax=1G`)
**Issue:** The README now says: "budget about 5 MB per running script (up to 4 per PATCH, one
PATCH per CPU) plus the data you expect to store". That budget counts the stored data once. It
also assumes a running script costs about 5 MB whatever it does. Neither holds for the routes and
ops that copy the whole store:
- **`GET /list`** (`src/http/kv.rs:57`). `Store::list()` (`src/store/mod.rs:75-85`) clones every
  key and value, `legacy::format_list` builds a second copy, and `HttpResponse::json` escapes it
  into a third. In my measurement one request transiently cost about 3.9× the stored data.
  Concurrent requests stack: actix runs one worker per CPU, and each finished body stays in memory
  until a client that may be slow has read it.
- **`op_rs3_list()`.** It makes the same copies plus the V8 string, so a running script that calls
  it costs about 4× the store, not 5 MB. In my measurement that was about 83 MB per script for a
  20 MB store, about 16× the documented budget.

A concrete case with the shipped `MemoryMax=1G` on a small server, following the README:
- A store of about 250 MB is well within "1G minus the data you expect to store".
- One `GET /list` then needs about 250 MB + 3.9 × 250 MB ≈ 1.2 GB, past the cap.
- With `MemorySwapMax=0`, the kernel OOM-kills rs3, systemd restarts it, and the whole store is
  gone.
- On a 4-CPU host, four concurrent `/list` calls (or four PATCHes that call `op_rs3_list`) do the
  same at a store of about 65 MB.

This comes from this phase's own changes. Before the cap, these copies used host memory and
succeeded. The fix turned "a runaway script" into "a legitimate `/list` on a moderately large
store" as the trigger for total data loss. The README's own sentence, "Reaching the cap kills rs3
and empties the store", tells the operator the consequence. The budget it gives them to avoid that
consequence is wrong.

**Fix:** Correct the rule so it budgets the transient copies, for example (`Readme.md:395-399`):
```markdown
Size the value to the server. Budget about 5 MB per running script (up to 4 per
PATCH, one PATCH per CPU) plus the data you expect to store, and then about 4×
the stored data again for every `GET /list` or `op_rs3_list()` that can run at
the same time (up to one per CPU each), because each one copies the whole store.
The store counts against the same cap, so with the shipped 1G keep the stored
data well below 1G / (1 + 4 × CPUs). On a 64-CPU server, full nested load alone
needs more than the shipped 1G. Reaching the cap kills rs3 and empties the store.
```
Optionally, also shorten the unit comment at `deploy/rs3.service:22-23` to "Size to the server
(see Readme.md, Service user and sandbox)", so operators find the rule.

## Info

### IN-01: The `MemoryMax=` paragraph states the kill-and-empty consequence twice

**File:** `Readme.md:393-395` and `Readme.md:398-399`
**Issue:** The paragraph says "If rs3 grows past the cap, the kernel kills rs3 … and rs3
restarts with an empty store." Four lines later it says "Reaching the cap kills rs3 and empties
the store." This duplication came from the iteration-2 fix (`ef9ab51`) appending its sentence
without merging it with the existing one. The paragraph is now 10 lines long, and the sizing rule
is buried between the two copies.
**Fix:** Drop the trailing "Reaching the cap kills rs3 and empties the store." Or merge the two
sentences, ideally while applying the WR-01 rewrite.

---

_Reviewed: 2026-10-21T12:10:00Z_
_Reviewer: Claude (gsd-code-reviewer)_
_Depth: standard_
