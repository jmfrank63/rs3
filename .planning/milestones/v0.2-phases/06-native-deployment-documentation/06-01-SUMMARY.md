---
phase: 06-native-deployment-documentation
plan: 01
subsystem: infra
tags: [systemd, seccomp, deployment, readme, license, v8]

requires:
  - phase: 05-js-limits
    provides: depth limit (422 js_depth_limit) and 5000 ms timeout watchdog (504 js_timeout) exercised under the seccomp kill run
  - phase: 04-ci
    provides: the D-17 gate scripts (ci-guards, check-workflow, test-ci-guards, ci-smoke)
provides:
  - deploy/rs3.service — example hardened systemd unit (env(1) bridge, DynamicUser, no capabilities, @system-service @pkey filter, start limit 5/60 s)
  - deploy/rs3.conf — example env file (RS3_HOST=127.0.0.1, RS3_PORT=5000)
  - Cargo.toml license = "MIT"
  - Readme.md overview (msc pointer), Deployment (+ Troubleshooting) and License sections
  - container deployment removed (Dockerfile, docker_rs3/), brief item 9 re-scoped
affects: [06-02 README developer sections, milestone audit, operator UAT on an x86_64 server]

actuals:
  tokens: 8237
  tasks: 3
  commits: 3
plan_head_before: a782a37ed62808509cebead4045e1cb4baabc777
plan_head_after: 1f6a1bb57efb1e8f36739f827d7e9af758638ac5

tech-stack:
  added: []
  patterns:
    - "systemd env(1) bridge: EnvironmentFile holds RS3_*; ExecStart=/usr/bin/env SERVER.HOST=${RS3_HOST} SERVER.PORT=${RS3_PORT} rs3"
    - "user-manager live run derived from the committed unit file (directives + ExecStart read from deploy/rs3.service)"

key-files:
  created:
    - deploy/rs3.service
    - deploy/rs3.conf
  modified:
    - Cargo.toml
    - Readme.md
    - MODERNIZATION_BRIEF.md
    - .gitignore
    - .planning/phases/06-native-deployment-documentation/06-VALIDATION.md
  deleted:
    - Dockerfile
    - docker_rs3/ (12 tracked files)

key-decisions:
  - "deploy/rs3.service is the verified 06-RESEARCH.md text byte-for-byte (verify clean, exposure 1.5); no directive was changed"
  - "LICENSE is not created on this branch; it arrives from main (9d1596b) with the merge, so the README's LICENSE reference dangles until then"
  - "The reset-failed recovery step is cited from the systemd.unit 255 docs (wording confirmed against freedesktop.org/software/systemd/man/255/systemd.unit.html), not live-run"
  - "README Troubleshooting quotes journal message text as observed in the user-manager runs, without the scratch unit-name prefix"

patterns-established:
  - "Live unit proof: bash target/tmp/p6/unit-live.sh bridge|kill|startlimit|nofile|update (scratch, not committed)"

requirements-completed: [DEPL-01, DEPL-02, DEPL-03, DOCS-04]

coverage:
  - id: D1
    description: "Committed unit + env file run the release binary under systemd (user manager) through the env(1) bridge; GET / and a V8 PATCH answer with NoNewPrivs 1 and Seccomp 2"
    requirement: DEPL-03
    verification:
      - kind: integration
        ref: "bash target/tmp/p6/unit-live.sh bridge"
        status: pass
      - kind: other
        ref: "systemd-analyze verify --recursive-errors=no deploy/rs3.service (empty, exit 0); systemd-analyze security --offline=true --threshold=15 deploy/rs3.service (1.5 OK)"
        status: pass
    human_judgment: false
  - id: D2
    description: "V8 survives a seccomp kill-action run (nested PATCH, 422 depth limit, 504 timeout, clean stop); broken/missing env file stops at the start limit; install replaces a running binary"
    requirement: DEPL-03
    verification:
      - kind: integration
        ref: "bash target/tmp/p6/unit-live.sh kill|startlimit|nofile|update"
        status: pass
    human_judgment: false
  - id: D3
    description: "Root-only unit directives, the non-root DynamicUser and V8 pkey_* calls under the filter on x86_64 with PKU"
    requirement: DEPL-03
    verification: []
    human_judgment: true
    rationale: "Needs PID 1 / root on a real server (ideally x86_64 with PKU); the dev host has no sudo and blocks unprivileged user namespaces. See Pending operator UAT."
  - id: D4
    description: "README Deployment section (build, install, configuration, sandbox, check, update, security note, troubleshooting) and License section; Cargo.toml license MIT"
    requirement: DEPL-02
    verification:
      - kind: other
        ref: "Task 1 acceptance criteria 6-7, Task 2 verify 2 and AC4; install dry-run into target/tmp/p6/root; cargo metadata license == MIT"
        status: pass
    human_judgment: true
    rationale: "README prose readability and accuracy for an operator is a judgment call; the commands and quoted lines are machine-checked."
  - id: D5
    description: "Container deployment removed (Dockerfile, docker_rs3/, dead .gitignore line, brief re-scoped), container-tool grep empty, msc tag intact, full D-17 gate green"
    requirement: DEPL-01
    verification:
      - kind: other
        ref: "git grep -n -i <container tool> -- . ':!.planning' ':!aws' ':!Vagrantfile' ':!experiments' (empty, rc 1) + Task 3 verify 1"
        status: pass
      - kind: integration
        ref: "D-17 gate: ci-guards, check-workflow, test-ci-guards (99 cases, 0 missed), fmt, clippy -D warnings, cargo test (216 passed), release build, ci-smoke"
        status: pass
    human_judgment: false

duration: 11min
completed: 2026-10-21
status: complete
---

# Phase 6 Plan 01: Native Deployment Slice Summary

**Hardened systemd unit (env(1) bridge for the dotted SERVER.* keys, DynamicUser, no capabilities, `@system-service @pkey` seccomp filter, start limit 5 in 60 s) proven under the systemd user manager with V8 executing through nested PATCH, the depth limit and the timeout, plus the README Deployment/License sections, `license = "MIT"`, and removal of the 2021 container deployment.**

## Performance

- **Duration:** 11 min
- **Started:** 2026-10-21T10:40:20Z
- **Completed:** 2026-10-21T10:51:30Z
- **Tasks:** 3 (Task 1 tracer, tracer gate re-verified in auto mode)
- **Files modified:** 20 (2 created, 5 modified, 13 deleted)

## Accomplishments

- `deploy/rs3.service` and `deploy/rs3.conf`, byte-identical to the verified 06-RESEARCH.md text. `systemd-analyze verify` is clean and the exposure score is 1.5.
- Live proof under the systemd user manager in five helper modes. The committed unit's own ExecStart and directives run the release binary, with seccomp active (`Seccomp: 2`) and no SIGSYS even when the filter's action is kill.
- README rewritten as an overview with the `msc` pointer, `## Deployment` (build, install, configuration under systemd, service user and sandbox, check, update, security note, troubleshooting) and `## License`.
- `Dockerfile`, `docker_rs3/` (12 files) and the dead ignore line are gone, and the brief is re-scoped. The container-tool grep outside the MSc-era paths is empty, and the full D-17 gate is green.

## Task Commits

1. **Task 1: tracer, unit + env file + license + README deployment** - `9b9e0ea` (feat)
2. **Task 2: failure modes proven and documented** - `23206db` (docs)
3. **Task 3: container deployment removed, brief re-scoped, D-17 gate** - `1f6a1bb` (chore)

## Evidence

### systemd-analyze (D-11a/b)

```text
$ systemd-analyze verify --recursive-errors=no deploy/rs3.service
(no output, exit 0)
$ systemd-analyze security --offline=true --threshold=15 deploy/rs3.service   # exit 0
→ Overall exposure level for rs3.service: 1.5 OK :-)
```

### Measured glibc floor (aarch64, glibc 2.39 host)

```text
$ ldd target/release/rs3
	linux-vdso.so.1
	libgcc_s.so.1 => /lib/aarch64-linux-gnu/libgcc_s.so.1
	libm.so.6 => /lib/aarch64-linux-gnu/libm.so.6
	libc.so.6 => /lib/aarch64-linux-gnu/libc.so.6
	/lib/ld-linux-aarch64.so.1
$ objdump -T target/release/rs3 | grep -o 'GLIBC_[0-9.]*' | sort -uV | tail -1
GLIBC_2.34
```

The x86_64 floor was not measured.

### Install dry-run (target/tmp/p6/root, no sudo)

```text
644 etc/rs3/rs3.conf
644 etc/systemd/system/rs3.service
755 usr/local/bin/rs3
```

`git diff --quiet 90d06ec -- Cargo.lock` holds, and `cargo metadata` reports license `MIT`.

### unit-live: bridge (Task 1)

```text
unit-live: directives StartLimitIntervalSec=60; StartLimitBurst=5; Type=exec; SyslogIdentifier=rs3; Restart=on-failure; RestartSec=2; NoNewPrivileges=yes; RestrictNamespaces=yes; RestrictRealtime=yes; RestrictSUIDSGID=yes; LockPersonality=yes; RestrictAddressFamilies=AF_UNIX AF_INET AF_INET6; SystemCallArchitectures=native; SystemCallFilter=@system-service @pkey; SystemCallErrorNumber=EPERM; UMask=0077
unit-live: ok   banner tagged rs3 on port 42439
unit-live: ok   GET / -> 200 "Welcome to rs3"
unit-live: ok   PATCH /key/m -> 200 "120"
unit-live: ok   main PID runs target/release/rs3 with SERVER.HOST=127.0.0.1 SERVER.PORT=42439
unit-live: ok   NoNewPrivs: 1 Seccomp: 2
unit-live: OK bridge
```

That is 16 directives read from the committed unit. No `rs3-p6-*` unit was left afterwards.

### unit-live: kill, startlimit, nofile, update (Task 2)

```text
unit-live: directives StartLimitIntervalSec=60; StartLimitBurst=5; Type=exec; SyslogIdentifier=rs3; Restart=on-failure; RestartSec=2; NoNewPrivileges=yes; RestrictNamespaces=yes; RestrictRealtime=yes; RestrictSUIDSGID=yes; LockPersonality=yes; RestrictAddressFamilies=AF_UNIX AF_INET AF_INET6; SystemCallArchitectures=native; SystemCallFilter=@system-service @pkey; UMask=0077
unit-live: ok   banner tagged rs3 on port 37707
unit-live: ok   GET / -> 200 "Welcome to rs3"
unit-live: ok   PATCH /key/m -> 200 "120"
unit-live: ok   main PID runs target/release/rs3 with SERVER.HOST=127.0.0.1 SERVER.PORT=37707
unit-live: ok   NoNewPrivs: 1 Seccomp: 2
unit-live: ok   PATCH /key/n (nested) -> 200 "120"
unit-live: ok   PATCH /key/me -> 422 js_depth_limit
unit-live: ok   PATCH /key/loop -> 504 js_timeout after 5010 ms
unit-live: ok   NRestarts=0, MainPID unchanged
unit-live: ok   clean stop, no SIGSYS or core dump in the journal
unit-live: OK kill
unit-live: directives StartLimitIntervalSec=60; StartLimitBurst=5; Type=exec; SyslogIdentifier=rs3; Restart=on-failure; RestartSec=2; NoNewPrivileges=yes; RestrictNamespaces=yes; RestrictRealtime=yes; RestrictSUIDSGID=yes; LockPersonality=yes; RestrictAddressFamilies=AF_UNIX AF_INET AF_INET6; SystemCallArchitectures=native; SystemCallFilter=@system-service @pkey; SystemCallErrorNumber=EPERM; UMask=0077
unit-live: ok   failed after NRestarts=5: Start request repeated too quickly.
unit-live: ok   journal tagged rs3: error: invalid configuration: invalid type: string "", expected an integer for key `server.port` in the environment
unit-live: ok   journal order: Started before the error line (Type=exec)
unit-live: ok   still failed 5 s later, NRestarts=5
unit-live: OK startlimit
unit-live: directives StartLimitIntervalSec=60; StartLimitBurst=5; Type=exec; SyslogIdentifier=rs3; Restart=on-failure; RestartSec=2; NoNewPrivileges=yes; RestrictNamespaces=yes; RestrictRealtime=yes; RestrictSUIDSGID=yes; LockPersonality=yes; RestrictAddressFamilies=AF_UNIX AF_INET AF_INET6; SystemCallArchitectures=native; SystemCallFilter=@system-service @pkey; SystemCallErrorNumber=EPERM; UMask=0077
unit-live: note systemd-run exited 1: 
unit-live: ok   journal: Failed to load environment files: No such file or directory
unit-live: ok   failed after NRestarts=5
unit-live: OK nofile
unit-live: ok   install over the running binary exits 0
unit-live: OK update
```

The kill run read 15 directives (SystemCallErrorNumber was dropped on purpose, so any syscall outside the filter would kill rs3 with SIGSYS).

Observed journal lines that the README Troubleshooting quotes (startlimit and nofile runs, `-o short`):

```text
rs3[3965102]: error: invalid configuration: invalid type: string "", expected an integer for key `server.port` in the environment
rs3-p6-startlimit-3965092.service: Start request repeated too quickly.
rs3-p6-nofile-3965184.service: Failed to load environment files: No such file or directory
```

### D-17 gate (Task 3)

The full chain `ci-guards && check-workflow && test-ci-guards && fmt --check && clippy -D warnings && test && build --release && ci-smoke` exited 0. The results were: `ci-guards: all guards passed`, `test-ci-guards: 99 cases, 0 missed`, `test result: ok. 216 passed; 0 failed` and `ci-smoke: OK`. `.github/`, `scripts/`, `src/`, `Cargo.lock`, `aws/`, `Vagrantfile` and `experiments/` are unchanged since 90d06ec. The msc tag resolves to `3ca26a1`.

## Files Created/Modified

- `deploy/rs3.service` is the example hardened unit (verified research text, unchanged).
- `deploy/rs3.conf` is the example env file: RS3_HOST/RS3_PORT under comments on why the names are not SERVER.* and on there being no authentication.
- `Cargo.toml` gains `license = "MIT"` after `rust-version` (1 line added).
- `Readme.md` is rewritten with the overview, `## Deployment` (with `### Troubleshooting`) and `## License`.
- `MODERNIZATION_BRIEF.md` has 3 hunks: the CI-only problem bullet, item 9 re-scoped and the out-of-scope line.
- `.gitignore` lost `/docker_rs3/.secrets/` (numstat 0 1).
- `06-VALIDATION.md` has rows 06-01-01..03 set to green. Status stays draft, with nyquist_compliant and wave_0_complete false.
- Deleted: `Dockerfile` and the 12 tracked files under `docker_rs3/`.

## Decisions Made

- **LICENSE file:** not created on this branch. It exists on `main` (9d1596b) and arrives with the merge, so the README's `LICENSE` reference dangles on this branch until then (accepted in the plan).
- **reset-failed citation status:** cited, not live-run. The quote "systemctl reset-failed will cause the restart rate counter for a service to be flushed" was confirmed verbatim against the systemd 255 systemd.unit page (freedesktop.org/software/systemd/man/255/systemd.unit.html, fetched read-only). The man page is not installed on the host. Recovery under the system manager is part of the operator UAT below.
- **Troubleshooting quotes:** the README quotes only the message text observed in the runs, minus the scratch unit-name prefix (`rs3-p6-*.service:`). The rs3 line keeps its observed `rs3[<pid>]:` tag.

## Deviations from Plan

### Auto-fixed Issues

**1. [Rule 1 - Bug] unit-live.sh nofile mode treated systemd-run's own exit status as a failure**
- **Found during:** Task 2, first run of the four modes
- **Issue:** systemd-run waits for the start job. With a missing EnvironmentFile the first start job fails (`Failed to spawn 'start' task`, result `resources`), so systemd-run exits 1, which is exactly the condition under test. The helper aborted with `FAIL systemd-run`.
- **Fix:** `start_unit` takes a `may-fail` flag, used only in nofile mode. It logs `unit-live: note systemd-run exited <rc>` and then asserts the journal line, `ActiveState=failed` and `NRestarts=5` as specified. All other modes still fail on a systemd-run error.
- **Files modified:** `target/tmp/p6/unit-live.sh` (scratch, not committed)
- **Verification:** all four modes green, with no `rs3-p6-*` unit left.
- **Committed in:** n/a (scratch helper)

---

**Total deviations:** 1 auto-fixed (1 bug in the scratch helper).
**Impact on plan:** none on the shipped files. The unit text is unchanged and the assertions are as specified.

## Issues Encountered

- **The release build after the Cargo.toml edit was a no-op.** cargo reported `Fresh rs3`, because the `license` field is not part of cargo's fingerprint. Research had expected the rs3 crate to rebuild. `target/release/rs3` (built 01:17) is newer than the last `src` commit (dfd1120, 01:09), so every live run tested the shipped tree. The Task 3 gate also ran `cargo build --release --locked`.

## Pending operator UAT

**result: [pending]**. None of the items below was verified on the dev host. It has no sudo and blocks unprivileged user namespaces. Under the user manager, ProtectSystem and ProtectProc are silently ignored, and DynamicUser, PrivateDevices and CapabilityBoundingSet are refused. These items stay open until the operator reports them.

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

- Plan 06-02 can insert the developer README sections between the overview and `## Deployment`, and Development between `## Deployment` and `## License`.
- DEPL-03 is complete for everything provable without root. The root-only and x86_64 parts are the pending operator UAT above.

## Self-Check: PASSED

- FOUND: deploy/rs3.service, deploy/rs3.conf, Readme.md, Cargo.toml, MODERNIZATION_BRIEF.md, .gitignore, 06-VALIDATION.md
- GONE (intended): Dockerfile, docker_rs3/
- FOUND commits: 9b9e0ea, 23206db, 1f6a1bb
- Measured: `git rev-list --count a782a37..HEAD` = 3

---
*Phase: 06-native-deployment-documentation*
*Completed: 2026-10-21*
