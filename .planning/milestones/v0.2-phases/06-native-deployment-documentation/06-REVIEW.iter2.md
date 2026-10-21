---
phase: 06-native-deployment-documentation
reviewed: 2026-10-21T11:15:08Z
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
  warning: 4
  info: 3
  total: 7
status: issues_found
---

# Phase 6: Code Review Report

**Reviewed:** 2026-10-21T11:15:08Z
**Depth:** standard
**Files Reviewed:** 8
**Status:** issues_found

## Summary

I reviewed the diff from `2ecb06d` to HEAD for the eight files in scope. I checked the README and
`docs/js-ops.md` claims against `src/` (`main.rs`, `config.rs`, `http/mod.rs`, `js/mod.rs`,
`js/ops.rs`, `js/executor.rs`), `.github/workflows/ci.yml` and `scripts/`. I read
06-CONTEXT.md and 06-RESEARCH.md first, so this report does not count deliberate decisions
(D-01..D-17, the resolved open questions) as defects.

Live checks against `target/release/rs3` (scratch under `target/tmp`, removed afterwards):
- Confirmed: `GET /ip`, the sibling-`op_rs3_patch` loop answering `"done"`, the timeout plus
  kept write (`w2` → `"kept"`), and the stack-exhaustion stderr line.
- `systemd-analyze verify --recursive-errors=no deploy/rs3.service` exits 0 with no output.
- The D-04 `git grep -i docker` acceptance check prints nothing.

The unit's directives are coherent:
- The env(1) bridge cannot be turned into option injection, because every expanded argument
  starts with `SERVER.`.
- `@pkey` is required, because `@system-service` lacks it.
- `MemoryDenyWriteExecute` is correctly absent.
- `EnvironmentFile=` has no `-` prefix, so a missing file fails loudly.
- Ambient capabilities would survive the intermediate `/usr/bin/env` exec, so the low-port
  advice works.

The executor.rs change is a comment only, and the comment is accurate for the production path
(`main` calls `init_platform`, then `JsExecutor::start` spawns the workers).

There are no blockers. The problems are in the documentation:
- One documented V8 output line does not match what V8 actually prints.
- The README says CI runs a script that is explicitly not a CI step.
- The security note is narrower than the real exposure.
- The hardened unit has no memory ceiling, although the docs admit an unbounded heap.

Noted but not flagged: `Readme.md:487` and `license = "MIT"` refer to a `LICENSE` file that is
not on this branch. This is deliberate (D-12 item 11): the file is on `main` at 9d1596b and is
MIT.

## Warnings

### WR-01: The documented V8 stack-overflow stdout line has the wrong text and the wrong timing

**File:** `docs/js-ops.md:213-216`
**Issue:** The new text says V8 prints "an unprefixed `Uncaught RangeError: Maximum call stack
size exceeded` line to stdout, once per failed start". Both parts are inaccurate:
- **Text.** A live run of the release binary (a nested `op_rs3_patch` from a stack-exhausted
  script) wrote `<unknown>:-1: Uncaught RangeError: Maximum call stack size exceeded` to stdout.
  The line has a `<unknown>:-1: ` source-location prefix, so it is not "unprefixed". 06-RESEARCH
  Pitfall 7 (line 522) also recorded `ext:core/00_primordials.js:6413: …` variants in the same
  catch-and-retry run, so the location part is not even fixed.
- **Timing.** V8's default message reporter writes through C stdio. When stdout is not a
  terminal, which is the case under systemd (journal stream) or when redirected, C stdio is fully
  buffered. In the live run the line appeared only when the process exited after SIGTERM. So the
  lines do not appear "once per failed start". They show up in batches, can come out of order
  with Rust's line-buffered stdout (05-REVIEW IN-07 already notes the interleaving), and are lost
  if the process aborts (for example on heap exhaustion). An operator who greps the journal for
  the quoted text, or who expects one entry per failure, will be misled.

**Fix:** Reword this part of the paragraph, for example:
```markdown
One exception: when a nested `op_rs3_patch` cannot start for lack of stack,
V8's own message reporter prints a line such as
`<unknown>:-1: Uncaught RangeError: Maximum call stack size exceeded` to
stdout, without the `js stderr: ` prefix. It goes through C stdio buffering,
so under systemd these lines can reach the journal late, in batches, or not
at all if the process aborts.
```

### WR-02: The README says CI runs `scripts/test-ci-guards.sh`, but that script is local-only

**File:** `Readme.md:461-472`
**Issue:** "These are the checks CI runs, in the order a contributor runs them locally:" is
followed by a list that includes `bash scripts/test-ci-guards.sh` and
`python3 scripts/check-workflow.py`.
- `scripts/test-ci-guards.sh:2` says: "Planted-violation self-test for scripts/ci-guards.sh
  (D-05). Local only, not a CI step."
- `.github/workflows/ci.yml` has no step for either script. `check-workflow.py` runs only inside
  `ci-guards.sh` as guard (g).

The paragraph at `:477-479` describes the CI job correctly, so the two parts of the section
contradict each other. A contributor who expects CI to catch a broken self-test will not get
that.

**Fix:** Split the list, for example: "CI runs these checks (`check-workflow.py` runs inside
`ci-guards.sh` as guard (g)):" followed by the guards, fmt, clippy, test, release build and
smoke. Then add a separate line: "Also run the guard self-test locally (CI does not):
`bash scripts/test-ci-guards.sh`". Alternatively, drop "These are the checks CI runs" and say
"the local gate, a superset of CI".

### WR-03: The security note treats "loopback" as private, but loopback is reachable by every local account and by browsers on the host

**File:** `Readme.md:411-416`, `deploy/rs3.conf:4-5`
**Issue:** The note says: "Keep the loopback default (`RS3_HOST=127.0.0.1`), or put something you
control in front of it". It presents both as sufficient mitigations for an unauthenticated JS
executor. They are not:
- **Local accounts.** On a shared server every local account and every compromised local process
  can reach `127.0.0.1:5000` directly. Putting a reverse proxy with authentication in front does
  not remove that path, because the backend port stays open on loopback.
- **Browsers on the same machine.** This matters most for the Quick start on a developer machine.
  The README itself says (`:154`) that `POST /insert` accepts any Content-Type. A web page the
  user visits can therefore send a CORS "simple" request (`fetch(..., {method:'POST',
  mode:'no-cors', body:'{"k":"<script>"}'})`, sent as `text/plain`) without a preflight. That
  request overwrites stored keys, including scripts that the user later PATCHes.
- **DNS rebinding.** rs3 does not check the `Host` header, so a DNS-rebinding page can also issue
  PATCH requests and read the responses.

The unit has no network-level enforcement either. If `RS3_HOST` is changed to `0.0.0.0`, nothing
in the sandbox limits who can connect.

**Fix:** State the real boundary in the security note, for example: "Loopback still lets every
local user and process, and web pages open in a browser on the same machine, reach rs3. Use it
only on a host where you trust all of them." Optionally, add defence in depth to the unit that
keeps it loopback-only even if `RS3_HOST` changes. A reverse proxy on the same host still works,
because it connects over loopback. Because the root-only directives are UAT items (D-11), add it
as a commented example if it cannot be verified here:
```ini
# Optional: refuse every peer except loopback, whatever RS3_HOST says.
#IPAddressDeny=any
#IPAddressAllow=localhost
```

### WR-04: The "hardened" unit has no memory ceiling, although the docs admit there is no heap limit

**File:** `deploy/rs3.service:12-56`; related `Readme.md:304-305`, `docs/js-ops.md:245-249`
**Issue:** Both documents say that a script that allocates without bound can exhaust V8's heap and
abort the process. The JS pool runs one worker per available CPU (`src/js/mod.rs:245-248`), and
each worker has its own isolates, plus up to 3 nested isolates per job. Concurrent PATCH requests
can therefore drive the service's RSS to several times one isolate's default heap limit before
V8 aborts. On a small server that is more than the host has, so the kernel OOM killer acts on
the whole host instead of only on rs3, and other services on the server can be killed or
stalled.

The unit sets no `MemoryMax=` (or `MemoryHigh=`), so systemd does not contain this to rs3's
cgroup. A memory ceiling is a standard sandbox directive. It is the natural place to bound the
gap that v2 JSHD-01 will close in code, and it needs no code change.

**Fix:** Add a cgroup ceiling with a comment, and mention it in the README's Service user and
sandbox section:
```ini
# No V8 heap limit yet (v2): cap the whole service so a runaway script is
# OOM-killed inside rs3's cgroup and restarted, not the host. Size to the server.
MemoryMax=1G
```
Under the current `Restart=on-failure`, a cgroup OOM kill counts as a failure and restarts rs3
with an empty store, which is the behaviour the README already documents for heap exhaustion.

## Info

### IN-01: The start limit turns client-triggerable crashes into a permanent outage

**File:** `deploy/rs3.service:7-10`, `Readme.md:439-457`
**Issue:** `StartLimitIntervalSec=60` / `StartLimitBurst=5` were added deliberately (06-RESEARCH
Open Question 1) so that a broken `rs3.conf` stops the restart loop. Heap exhaustion, however,
can be triggered by any client: no auth, no heap limit. A client that crashes rs3 about 6 times
within 60 s leaves the unit in `failed` until an operator runs `systemctl reset-failed`.
`Readme.md:456-457` says "Repeated crashes hit the same start limit" but does not say that this
is a denial of service that lasts until someone intervenes.
**Fix:** Spell out the consequence in the Troubleshooting paragraph. Alternatively, on
systemd ≥ 254 (the research host has 255), consider backoff instead of a hard stop for crashes,
for example `RestartSteps=5` with `RestartMaxDelaySec=60`, and keep the start limit wide enough
to still catch configuration failures. WR-04's `MemoryMax=` does not change this, because a
cgroup OOM kill is still a failure.

### IN-02: "The environment wins over such a file anyway" overstates how harmless `/.env` is under the unit

**File:** `Readme.md:372-374`
**Issue:** Under the system manager the working directory is `/`, and `Settings::load` calls
`dotenvy::dotenv_iter()` (`src/config.rs:88-90`). The environment wins only over the values in a
well-formed file. A malformed, BOM-prefixed or unreadable `/.env` (for example root-owned with
mode 0600, which the DynamicUser cannot read) is a hard startup error
(`src/config.rs:99-104`, `error: invalid configuration: .env: …`), whatever the environment
holds. 06-RESEARCH Pitfall 4 notes this edge, but the README sentence suggests the file can never
matter.
**Fix:** Change the end of the sentence to: "…and the environment wins over the values in such a
file, but a malformed or unreadable `/.env` still stops startup."

### IN-03: The Update section does not cover a changed unit or environment file

**File:** `Readme.md:399-409`
**Issue:** The update steps re-install only the binary and restart. If a pull also changes
`deploy/rs3.service`, the installed unit stays stale. Running the steps above it instead also
fails the update: re-running the Install block overwrites the operator's edited
`/etc/rs3/rs3.conf` (`install -D … deploy/rs3.conf /etc/rs3/rs3.conf`), and skipping
`daemon-reload` leaves the old unit loaded.
**Fix:** Add one sentence: "If `deploy/rs3.service` changed, re-install it and run
`sudo systemctl daemon-reload` before the restart. Do not re-install `rs3.conf` over your
edited copy."

---

_Reviewed: 2026-10-21T11:15:08Z_
_Reviewer: Claude (gsd-code-reviewer)_
_Depth: standard_
