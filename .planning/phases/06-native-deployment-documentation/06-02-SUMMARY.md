---
phase: 06-native-deployment-documentation
plan: 02
subsystem: docs
tags: [readme, docs, deno_core, actix-web, replay, ci]

requires:
  - phase: 06-native-deployment-documentation
    provides: "06-01: README overview, Deployment and License sections, deploy/rs3.service, unit-live.sh helper"
  - phase: 05-js-execution-limits
    provides: depth limit (422 js_depth_limit) and 5000 ms timeout (504 js_timeout) documented here
  - phase: 04-ci
    provides: the D-17 gate scripts and the single ubuntu-26.04 CI job described in Development
provides:
  - "Readme.md complete in D-12 order: Requirements, Quick start, Configuration, HTTP API, JavaScript ops, Limits, Behaviour changes, Development (plus 06-01's overview, Deployment, License)"
  - "docs/js-ops.md: stdout raw, unescaped (IN-03) with the V8 RangeError exception (IN-07); ops table and limits wording completed (IN-02)"
  - "src/js/executor.rs: watchdog spawn comment names the main thread as the V8 initialiser (IN-05)"
  - "06-VALIDATION.md: six rows green, wave_0_complete true"
affects: [phase verification, milestone audit, operator UAT (06-UAT.md)]

actuals:
  tokens: 8812
  tasks: 3
  commits: 3
plan_head_before: e99699f1d22397c5f95ecf9f59fbed5e91923f90
plan_head_after: e3e34850a85485ef4059c01e94ada098778d44f5

tech-stack:
  added: []
  patterns:
    - "README facts are replayed: target/tmp/p6/readme-replay.sh starts the release binary with env -i and the dotted names, diffs every documented body/status/startup line byte for byte, and requires each documented text verbatim on one README line (free port written as 5000)"

key-files:
  created: []
  modified:
    - Readme.md
    - docs/js-ops.md
    - src/js/executor.rs
    - .planning/phases/06-native-deployment-documentation/06-VALIDATION.md

key-decisions:
  - "Every README body, status and error line comes from the release binary via the replay; only 500 internal and the dotenv-file rules (malformed, BOM, unreadable) are taken from the code and labelled so"
  - "The 262144-byte limit (413) and accepting any Content-Type are documented in the HTTP API section as unchanged since 2021, never under Behaviour changes; the UTF-8 body rule sits next to them"
  - "2021 failure modes are worded as 'panicked' where the msc code used unwrap() in a handler, and 'crashed the server' only for the in-op unwrap"
  - "The README's port 80 line is labelled as assuming net.ipv4.ip_unprivileged_port_start=1024; a SIGINT claim was dropped because it was not replayed"

patterns-established:
  - "documented TEXT [PORT|none]: a replayed body counts as documented only if it appears verbatim on one README line"

requirements-completed: [DOCS-01, DOCS-02, DOCS-03]

coverage:
  - id: D1
    description: "README Requirements, Quick start, Configuration and HTTP API: every route body, error body, edge (byte order, overwrite, empty store, empty insert, body-order echo, 262144/262145 boundary, 70000 port) and startup error line equals the release binary's live output"
    requirement: DOCS-03
    verification:
      - kind: integration
        ref: "bash target/tmp/p6/readme-replay.sh (sections a-g; 88 checks at Task 1, part of the final 130)"
        status: pass
      - kind: other
        ref: "Task 1 verify 2 (heading order, exact Quick start lines, section greps, no container-tool word)"
        status: pass
    human_judgment: false
  - id: D2
    description: "README JavaScript ops, Limits and Behaviour changes: op syntax, 2021 -> now mapping with the observed legacy 422 bodies, fac worked example, String(result) table, print streams, depth 4/5 boundary, sibling calls, writes kept after the timeout, the eight verified changes"
    requirement: DOCS-02
    verification:
      - kind: integration
        ref: "bash target/tmp/p6/readme-replay.sh (section h; final run OK (130 checks))"
        status: pass
      - kind: other
        ref: "Task 2 verify 3 (sections, no 413/content-type in Behaviour changes) and AC4 (8 bullets)"
        status: pass
    human_judgment: false
  - id: D3
    description: "docs/js-ops.md no longer overclaims: stdout raw, unescaped; IN-07 exception; ops-table depth/timeout behaviour; after-timeout TypeError; sibling calls; worker queueing; untouched sections byte-identical to 90d06ec"
    requirement: DOCS-02
    verification:
      - kind: other
        ref: "Task 2 verify 2 (flattened greps: 3 overclaims gone, 5 corrections present, 4 table rows) and AC3 (five sections unchanged since 90d06ec)"
        status: pass
    human_judgment: false
  - id: D4
    description: "README Development section, IN-05 comment fix (comment lines only), D-17 gate, replay and unit bridge green on the final release build"
    requirement: DOCS-01
    verification:
      - kind: integration
        ref: "D-17 gate: ci-guards, check-workflow, test-ci-guards (99 cases, 0 missed), fmt, clippy -D warnings, cargo test (216 passed), release build, ci-smoke; then readme-replay OK (130 checks) and unit-live.sh bridge OK"
        status: pass
      - kind: other
        ref: "Task 3 verify 2 (git grep for the container tool rc 1, ten D-12 headings, executor.rs diff comment-only, no change since 90d06ec in .github/scripts/Cargo.lock/other src)"
        status: pass
    human_judgment: false
  - id: D5
    description: "README readability: a new developer can build, configure, run, call and extend rs3 from the README alone (ROADMAP Phase 6 SC-2/SC-4)"
    requirement: DOCS-01
    verification: []
    human_judgment: true
    rationale: "Every command and quoted line is machine-checked, but whether the prose reads well for a newcomer is a judgment call."

duration: 11min
completed: 2026-10-21
status: complete
---

# Phase 6 Plan 02: Developer Documentation Slice Summary

**README completed in D-12 order (requirements, quick start, configuration, HTTP API, JavaScript ops, limits, behaviour changes, development). Every documented body, status and startup line was replayed byte for byte against the release binary: 130 checks, covering the 262144/262145 and 4/5-deep boundaries. docs/js-ops.md now says stdout is raw, unescaped (IN-03, IN-07, IN-02), and the watchdog comment names the main thread (IN-05).**

## Performance

- **Duration:** 11 min
- **Started:** 2026-10-21T10:54:08Z
- **Completed:** 2026-10-21T11:05:00Z
- **Tasks:** 3. Task 1 was the tracer. Auto mode is on, so the tracer gate re-ran the Task 1 verify (green) before expanding.
- **Files modified:** 4 (Readme.md, docs/js-ops.md, src/js/executor.rs, 06-VALIDATION.md)

## Accomplishments

- **Readme.md now has ten `##` sections in D-12 order under `# rs3`.** The new ones are Requirements, Quick start, Configuration (a table of 10 observed startup error lines), HTTP API (route, edge and error tables), JavaScript ops (op table, 2021 mapping, worked example, results, output), Limits, Behaviour changes (8 bullets against tag `msc`) and Development (gate commands plus the CI paragraph).
- **The replay helper `target/tmp/p6/readme-replay.sh` (scratch, not committed) proves the README.** It runs 130 checks against one server plus separate config processes, and it ends with SIGTERM giving exit 0 and no rs3 process left.
- **docs/js-ops.md stops overclaiming.** stdout is raw, unescaped; V8's unprefixed RangeError line is the documented exception. The ops-table depth and timeout behaviour, the after-timeout TypeError, the uncounted sibling calls and worker queueing are now covered. The five untouched sections are byte-identical to 90d06ec.
- **IN-05:** the executor.rs comment is fixed. `git diff 90d06ec -- src/js/executor.rs` changes comment lines only, and fmt is clean.
- **Final tree is green:** the D-17 gate, the replay and the unit bridge all pass on the rebuilt release binary.

## Task Commits

1. **Task 1: tracer. README Requirements, Quick start, Configuration and HTTP API, replayed live:** `5e9ea69` (docs)
2. **Task 2: README JavaScript ops, Limits and Behaviour changes; js-ops.md IN-02/IN-03/IN-07:** `5a711de` (docs)
3. **Task 3: README Development; IN-05 comment; validation map:** `e3e3485` (docs)

## Evidence

### Final replay (after the Task 3 release rebuild, binary 11:02:43, newer than the executor.rs edit at 11:01:13)

`bash target/tmp/p6/readme-replay.sh` ends with `readme-replay: OK (130 checks)`. The check labels, in order (each `documented:` check, omitted here, confirms the body appears verbatim in Readme.md):

```text
banner on stdout
GET / -> 200 "Welcome to rs3"            GET / content-type application/json
GET /ip -> 200                            GET /list (empty store) -> 200 "[]"
GET|DELETE|PATCH /key/greeting (empty store) -> 404 not_found (+ content type)
POST /insert greeting, m -> 200 echo (+ content type)   GET /key/greeting -> 200 "hello"
GET /list -> 200 sorted                   PATCH /key/m -> 200 "120" (+ content type)
POST /insert q as text/plain -> 200       GET /list with an escaped value -> 200
DELETE /key/greeting -> 200 "hello"       DELETE /key/q (cleanup) -> 200
POST /insert b, a, B -> 200 echo in body order   GET /list -> B, a, b, m in byte order
POST /insert a=9 (overwrite) -> 200       GET /key/a after overwrite -> 200 "9"
POST /insert {} -> 200 "{}"               GET /list unchanged after {}
POST /insert {"a" -> 400 malformed_json   POST /insert empty body -> 400 malformed_json
POST /insert [1] -> 422 invalid_input     POST /insert {"a":1} -> 422 invalid_input
GET /key/a after rejected inserts -> 200 "9" (nothing inserted)
PATCH /key/boom -> 422 js_error           PATCH /key/syn (let a = ;) -> 422 js_error
PATCH /key/me -> 422 js_depth_limit       PATCH /key/loop -> 504 js_timeout (after 5014 ms)
POST /insert 262144 bytes -> 200          POST /insert 262145 bytes -> 413 payload reached size limit (text/plain)
GET /nope -> 404 empty body               POST / -> 404 empty body
nothing set | SERVER_HOST/SERVER_PORT only | host only | port only | SERVER.PORT=abc |
SERVER.PORT=70000 | SERVER.PORT= (empty) | SERVER.HOST= (empty) | busy port |
port 80 unprivileged -> exit 1 (each exact stderr line)
server.host / Server.Port (any case) -> banner
bare SERVER.HOST=... prefix in bash -> 127 command not found
POST /insert -d {"num":"5"} -> 200 echo   POST /insert -d @requests/fac.txt -> 200 echo
PATCH /key/fac -> 200 "120"
PATCH /key/s1 (let a=1;) -> 200 "undefined"     PATCH /key/s2 (({a:1})) -> 200 "[object Object]"
PATCH /key/pr (Promise.resolve(1)) -> 200 "[object Promise]"
PATCH /key/f (0.1+0.2) -> 200 "0.30000000000000004"
PATCH /key/old1 (Deno.core.ops();) -> 422 js_error   PATCH /key/old2 (Deno.core.opSync) -> 422 js_error
PATCH /key/c1 (caught missing key) -> 200 "key not found: nope"
PATCH /key/c2 (object argument) -> 200 "true" (TypeError)
PATCH /key/k1 (k1 -> k2 -> k3 -> k4, 4 deep) -> 200 "leaf"
PATCH /key/j1 (5 deep) -> 422 js_depth_limit j1 -> j2 -> j3 -> j4 -> j5
PATCH /key/sib (10 sibling op_rs3_patch calls) -> 200 "done"
PATCH /key/pt -> 200 "printed"; Deno.core.print(text) -> raw line on stdout;
Deno.core.print(text, true) -> stderr js stderr: "x\ny"
GET / while PATCH /key/w runs -> 200     GET /key/w2 while PATCH /key/w runs -> 200 "kept"
PATCH /key/w (insert w2, then loop) -> 504 js_timeout after 5008 ms
GET /key/w2 after the timeout -> 200 "kept"
SIGTERM -> exit 0
readme-replay: OK (130 checks)
```

Observed live values that were new at execution time: PATCH of `Deno.core.opSync("rs3_list", "")` answers 422 `{"error":"js_error","message":"Uncaught TypeError: Deno.core.opSync is not a function"}`, and the fac insert echoes the compact pair shown in the README. rs3 processes: 0 before and 0 after every run.

### unit-live bridge on the final tree

```text
unit-live: directives StartLimitIntervalSec=60; StartLimitBurst=5; Type=exec; SyslogIdentifier=rs3; Restart=on-failure; RestartSec=2; NoNewPrivileges=yes; RestrictNamespaces=yes; RestrictRealtime=yes; RestrictSUIDSGID=yes; LockPersonality=yes; RestrictAddressFamilies=AF_UNIX AF_INET AF_INET6; SystemCallArchitectures=native; SystemCallFilter=@system-service @pkey; SystemCallErrorNumber=EPERM; UMask=0077
unit-live: ok   banner tagged rs3 on port 54273
unit-live: ok   GET / -> 200 "Welcome to rs3"
unit-live: ok   PATCH /key/m -> 200 "120"
unit-live: ok   main PID runs target/release/rs3 with SERVER.HOST=127.0.0.1 SERVER.PORT=54273
unit-live: ok   NoNewPrivs: 1 Seccomp: 2
unit-live: OK bridge
```

No `rs3-p6-*` unit was left (`systemctl --user list-units --all 'rs3-p6-*'` shows 0).

### D-17 gate (Task 3, final tree)

| Command | Result |
|---------|--------|
| `bash scripts/ci-guards.sh` | 0, `ci-guards: all guards passed` ([a]-[h] ok) |
| `python3 scripts/check-workflow.py` | 0 |
| `bash scripts/test-ci-guards.sh` | 0, `test-ci-guards: 99 cases, 0 missed` |
| `cargo fmt --check` | 0 |
| `cargo clippy --all-targets --locked -- -D warnings` | 0 |
| `cargo test --locked` | 0, `216 passed; 0 failed` (the one "ignored" doctest is deno_core's own `extension!` macro doc, expanded at `js::ops::rs3_ext`, which predates this plan) |
| `cargo build --release --locked` | 0 (rebuilt the rs3 crate after the comment edit) |
| `bash scripts/ci-smoke.sh` | 0, `ci-smoke: OK` |

Scope: `git diff --quiet 90d06ec -- .github scripts Cargo.lock aws Vagrantfile experiments env.example requests` and every `src/` file except `src/js/executor.rs` holds. The case-insensitive `git grep` for the container tool outside `.planning`, `aws`, `Vagrantfile` and `experiments` prints nothing (rc 1).

### README facts taken from code, not a live run

- **500 `internal`**: `{"error":"internal","message":"internal error"}` comes from `#[error("internal error")]` in `src/http/error.rs`. It is a JS host fault and cannot be triggered on demand. The README says so in the error table.
- **The dotenv-file lines and the BOM rule**: a malformed, BOM-prefixed or unreadable dotenv file gives a line starting `error: invalid configuration: .env: ` and exit 1. This comes from `src/config.rs` (`dotenv_error`) and its tests (`bom_prefixed_dotenv_is_error`, `malformed_dotenv_is_error_with_env_prefix`, `unreadable_dotenv_is_error`), because agents never create or name the dotenv file. The README labels it.
- **UTF-8 body rule** (a charset parameter is ignored) and **"unchanged since 2021"** for the 413 limit and Content-Type come from 06-RESEARCH's audit of the msc sources. The 413 boundary itself and the acceptance of `text/plain` were replayed.
- The `env.example` copy path is prose only (never run, per the dotenv hook).

## Files Created/Modified

- `Readme.md` gains 8 sections between the overview and License (+214 lines across the three commits). 06-01's overview, Deployment and License are unchanged.
- `docs/js-ops.md`: the intro, 4 migration-table rows, the Errors timeout bullet, and the Sandbox print, depth and timeout bullets (+33/-19).
- `src/js/executor.rs`: the watchdog spawn comment (+2/-1, comment lines only).
- `06-VALIDATION.md`: rows 06-02-01..03 green, `wave_0_complete: true`, Wave 0 boxes and four sign-off items checked. `status: draft` and `nyquist_compliant: false` are unchanged.

## Decisions Made

- Only replayed facts go into the README. Where the plan's fact list allowed two readings, the README follows the live output: for example the old2 body, and the fac echo quoted verbatim instead of paraphrased.
- 2021 behaviour is worded as "panicked" where msc's handler used `unwrap()`, and "crashed the server" only for the in-op unwrap (process abort).
- A SIGINT/Ctrl-C claim was removed from Configuration because the replay only proves SIGTERM.
- The port 80 line carries the `net.ipv4.ip_unprivileged_port_start=1024` assumption, as the plan's flagged assumption asked.

## Deviations from Plan

### Auto-fixed Issues

**1. [Rule 1 - Bug] The replay helper's `documented` rewrote the literal port 80 to 5000**
- **Found during:** Task 1, first green-path run
- **Issue:** `documented TEXT PORT` substitutes the port with 5000. For the port 80 line that turned `127.0.0.1:80` into `127.0.0.1:5000`, so the documented check failed even though the README was right.
- **Fix:** `documented` takes `none` as the port argument to skip the substitution. The port 80 check uses it.
- **Files modified:** `target/tmp/p6/readme-replay.sh` (scratch, not committed)
- **Verification:** replay OK (88 checks), then 130.
- **Committed in:** n/a (scratch helper)

### Additions beyond the plan's check list (no behaviour change)

- The replay also asserts:
  - content-type `application/json` on POST /insert, PATCH and 404;
  - `POST /` gives an empty 404;
  - nothing is inserted after the rejected bodies (GET /key/a stays `"9"`), and `GET /list` is unchanged after `{}`;
  - SIGTERM gives exit 0;
  - `Deno.core.print` writes raw stdout and an escaped stderr line;
  - GET / and GET /key/w2 are answered while the w script loops.
  These make the README's Configuration, JavaScript ops "Output" and Limits claims replayed rather than code-derived.

---

**Total deviations:** 1 auto-fixed (1 bug in the scratch helper).
**Impact on plan:** none on the shipped files.

## Issues Encountered

- The first docs/js-ops.md patch attempt asserted a table-cell prefix (`| `[]` when…`) that is really `; `[]` when…`. The scripted edit aborted before writing anything, and the corrected pattern applied cleanly. No partial edit was written.

## Known Stubs

None. (`LICENSE`, referenced by the README License section from 06-01, still arrives from `main` with the merge, as accepted in 06-01.)

## Pending operator UAT

**result: [pending]**. Carried forward unchanged from 06-01. None of the items below was verified on the dev host. It has no sudo and blocks unprivileged user namespaces. Under the user manager, ProtectSystem and ProtectProc are silently ignored, and DynamicUser, PrivateDevices and CapabilityBoundingSet are refused. These items stay open until the operator reports them.

Scope: the root-only directives (DynamicUser, ProtectSystem, ProtectHome, PrivateTmp, PrivateDevices, ProtectKernelTunables/Modules/Logs, ProtectControlGroups, ProtectClock, ProtectHostname, ProtectProc, the empty CapabilityBoundingSet/AmbientCapabilities), the non-root service user, and V8's `pkey_*` calls under `SystemCallFilter=@system-service @pkey` on an x86_64 CPU with PKU.

Steps, on a Linux server with systemd and root, ideally x86_64 with PKU:

1. Install per the README Deployment section (`cargo build --release --locked`, then the five `sudo install` / `systemctl` lines).
2. `systemctl status rs3` shows active (running).
3. `curl -s http://127.0.0.1:5000/` prints `"Welcome to rs3"`.
4. `curl -s -X POST http://127.0.0.1:5000/insert -d '{"m":"6*20"}'`, then `curl -s -X PATCH http://127.0.0.1:5000/key/m` prints `"120"`.
5. `ps -o user= -p "$(systemctl show -p MainPID --value rs3)"` is not root.
6. `journalctl -u rs3` has no `status=31/SYS`.
7. Record `systemd-analyze security rs3` (expected about 1.5).
8. Optional: confirm the Troubleshooting recovery (`sudo systemctl reset-failed rs3` and `sudo systemctl restart rs3` after a start-limit failure).

## User Setup Required

None. No external service configuration is needed.

## Next Phase Readiness

- Phase 6 has both plans complete. It is ready for phase verification, with the operator UAT above still pending for 06-UAT.md.
- `06-VALIDATION.md` `status`, `nyquist_compliant` and approval are left for validate-phase.

## Self-Check: PASSED

- FOUND: Readme.md, docs/js-ops.md, src/js/executor.rs, 06-VALIDATION.md, target/tmp/p6/readme-replay.sh (scratch)
- FOUND commits: 5e9ea69, 5a711de, e3e3485 (all ancestors of HEAD; `check evaluation-scope --plan 06-02` resolves all three)
- Measured: `git rev-list --count e99699f..HEAD` = 3

---
*Phase: 06-native-deployment-documentation*
*Completed: 2026-10-21*
