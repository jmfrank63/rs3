---
phase: 06-native-deployment-documentation
verified: 2026-10-21T11:43:41Z
status: human_needed
score: 42/43 must-haves verified
covered_files:
  - ".gitignore"
  - ".planning/phases/06-native-deployment-documentation/06-01-PLAN.md"
  - ".planning/phases/06-native-deployment-documentation/06-01-SUMMARY.md"
  - ".planning/phases/06-native-deployment-documentation/06-02-PLAN.md"
  - ".planning/phases/06-native-deployment-documentation/06-02-SUMMARY.md"
  - "Cargo.toml"
  - "MODERNIZATION_BRIEF.md"
  - "Readme.md"
  - "deploy/rs3.conf"
  - "deploy/rs3.service"
  - "docs/js-ops.md"
  - "src/js/executor.rs"
covered_digest: "v3:sha256:10432410c2f3c9eaf9d5c830d364fd766e759d0edb70bdf2dc52b69a17b5054d"
behavior_unverified: 1
overrides_applied: 0
behavior_unverified_items:
  - truth: "ROADMAP SC-3 (DEPL-03): deploy/rs3.service runs rs3 as an unprivileged user and applies sandboxing directives that V8 tolerates"
    test: "On a Linux server with systemd as root (ideally x86_64 with PKU), install per the README Deployment section, then: systemctl status rs3; curl -s http://127.0.0.1:5000/; POST /insert {\"m\":\"6*20\"} then PATCH /key/m; ps -o user= -p \"$(systemctl show -p MainPID --value rs3)\"; journalctl -u rs3 | grep -c 'status=31/SYS'; systemd-analyze security rs3"
    expected: "active (running); \"Welcome to rs3\"; PATCH answers \"120\"; the main PID user is not root (the DynamicUser rs3); no status=31/SYS line; exposure about 1.5"
    why_human: "The dev host has no sudo and blocks unprivileged user namespaces. The user manager refuses DynamicUser, PrivateDevices and the capability sets and silently ignores ProtectSystem/ProtectProc, so the non-root user and the root-only sandbox around V8 cannot be exercised here. Only the user-manager-compatible subset (seccomp filter, NoNewPrivileges, Restrict*, start limit, MemoryMax/MemorySwapMax) was run live."
human_verification:
  - test: "Operator UAT on a real server as root (06-01-SUMMARY 'Pending operator UAT' steps 1-7): install with the five README lines, check status, GET /, PATCH 6*20, non-root main PID, no SIGSYS in the journal, systemd-analyze security rs3"
    expected: "rs3 active under DynamicUser (not root), answers \"Welcome to rs3\" and \"120\", no status=31/SYS, exposure about 1.5"
    why_human: "Root-only directives (DynamicUser, ProtectSystem/Home, PrivateTmp/Devices, ProtectKernel*, ProtectControlGroups, ProtectClock, ProtectHostname, ProtectProc, empty capability sets) cannot be applied by the user manager on this host"
  - test: "Same UAT on an x86_64 CPU with PKU: run a PATCH (nested too) under SystemCallFilter=@system-service @pkey"
    expected: "PATCH answers \"120\"; no SIGSYS/EPERM failure from pkey_alloc/pkey_mprotect"
    why_human: "All seccomp evidence is from aarch64, which has no PKU; the @pkey allowance is untested where it matters"
  - test: "On the real server, confirm the README Update and Troubleshooting commands: sudo install over the running binary + restart; break /etc/rs3/rs3.conf, observe 'Start request repeated too quickly.', then sudo systemctl reset-failed rs3 && sudo systemctl restart rs3"
    expected: "Update replaces the running binary; the unit recovers after reset-failed + restart"
    why_human: "Needs sudo and the system manager; reset-failed is cited from systemd.unit 255, not live-run"
  - test: "Decide on the MemoryMax= sizing paragraph (Readme.md:397-404, review iteration-3 WR-01 fix marked 'requires human verification'). The verifier measured on this 4-CPU host with a 20 MB store: 1 GET /list +77 MB (3.9x), 4 concurrent GET /list +307 MB, 4 concurrent PATCH op_rs3_list +324 MB, 4 GET /list + 4 PATCH op_rs3_list together +400 MB (about 20x the store)"
    expected: "Accept the rule as 'rough', or reword '(up to one per CPU)' to 'up to one per CPU for GET /list and one per CPU for op_rs3_list()' (separate actix and JS worker pools), which tightens the bound to about MemoryMax / (1 + 8 x CPUs) in the worst case"
    why_human: "Documentation judgement on a sizing heuristic outside the must-haves; the paragraph already says 'rough' and 'well below', but its stated concurrency bound undercounts mixed load"
  - test: "Read Readme.md end to end as a newcomer (06-02-SUMMARY coverage D5)"
    expected: "A new developer/operator can build, configure, run, call, deploy and extend rs3 without reading src/"
    why_human: "Every command and quoted line is machine-checked (130-check replay), but readability is a judgement call"
  - test: "After the operator pushes modernize-toolchain, confirm the GitHub Actions run on ubuntu-26.04 (x86_64) is green with zero annotations (ROADMAP SC-5)"
    expected: "ci.yml job green, no annotations"
    why_human: "Only the local aarch64 D-17 gate could be run; .github/ and scripts/ are unchanged since 90d06ec, but the remote run for this HEAD has not been observed"
  - test: "Review the six judgment-tier prohibitions (06-01: DEPL-03 transparency, DEPL-03 safety, DEPL-01 safety; 06-02: DOCS-03 transparency, DOCS-02 transparency, DOCS-01 safety). Non-authoritative verifier verdict: all six hold (see report table)"
    expected: "Human confirms each prohibition holds"
    why_human: "unverified-prohibition — human review recommended (judgment-tier; autonomous verify records only a non-authoritative LLM-judge verdict)"
audit_acknowledged:
  milestone: v1.0
  at: 2026-10-21
  status: human_needed
---

# Phase 6: Native Deployment & Documentation Verification Report

**Phase Goal:** As a new developer or operator of rs3, I want to build it as a native binary, run it on a single Linux server under an example hardened systemd unit, and learn the toolchain, configuration, HTTP API, JS op syntax and behaviour changes from the README, so that I can build, deploy and extend the modernized server without reading the source.
**Verified:** 2026-10-21T11:43:41Z
**Status:** human_needed
**Re-verification:** No (initial verification)
**Mode:** mvp (user-story format validated: `user-story.validate` → `true`)

## User Flow Coverage

User story: «As a new developer or operator of rs3, I want to build it as a native binary, run it on a single Linux server under an example hardened systemd unit, and learn the toolchain, configuration, HTTP API, JS op syntax and behaviour changes from the README, so that I can build, deploy and extend the modernized server without reading the source.»

| Step | Expected | Evidence | Status |
|------|----------|----------|--------|
| Learn the toolchain | README states Rust 1.99.1 via rust-toolchain.toml, edition 2024, V8 first-build download, disk needs | Readme.md:13-29; rust-toolchain.toml `channel = "1.99.1"`, components rustfmt/clippy; Cargo.toml edition 2024 / rust-version 1.99 | ✓ |
| Build natively | `cargo build --release --locked` produces target/release/rs3 | Readme.md:36, 347; gate ran `cargo build --release --locked` exit 0; ldd shows only libc/libm/libgcc_s; floor GLIBC_2.34 (matches Readme.md:352-353) | ✓ |
| Configure and run locally | `env SERVER.HOST=127.0.0.1 SERVER.PORT=5000 cargo run ...` prints the banner; every startup error line documented | Replay started the binary with env -i + dotted names; 10 config error lines replayed byte-for-byte; bare dotted prefix → exit 127 replayed | ✓ |
| Call the HTTP API and JS ops | Route table, error table, op syntax, legacy mapping, worked fac example | `bash target/tmp/p6/readme-replay.sh` → `readme-replay: OK (130 checks)`, every documented body verbatim on one README line | ✓ |
| Install under the example hardened unit | Five install lines put binary, env file and unit in place | Install dry-run → `644 etc/rs3/rs3.conf,644 etc/systemd/system/rs3.service,755 usr/local/bin/rs3`; `systemd-analyze verify` empty, exit 0; exposure 1.5 OK | ✓ |
| Run under systemd | The committed unit's ExecStart + EnvironmentFile serve GET / and a V8 PATCH with the sandbox active | `unit-live.sh bridge` OK (NoNewPrivs 1, Seccomp 2); `kill` OK (nested PATCH, 422, 504, no SIGSYS); `startlimit`/`nofile` OK; verifier-run unit with MemoryMax=1G/MemorySwapMax=0 applied (memory.max=1073741824, swap.max=0) answers PATCH "120" | ✓ (user-manager subset) |
| Run as unprivileged user with root-only sandbox | DynamicUser non-root PID; ProtectSystem etc. tolerated by V8 | Not exercisable on this host (no sudo, userns blocked) | ⚠️ human |
| Outcome: build, deploy and extend without reading source | README carries everything above plus Limits, Behaviour changes, Development, License | All ten D-12 sections present in order; replay and unit runs green on HEAD | ✓ (readability: human) |

## Goal Achievement

### Observable Truths — ROADMAP Success Criteria (contract)

| # | Truth | Status | Evidence |
|---|-------|--------|----------|
| SC-1 | Dockerfile and docker_rs3/ deleted, no .dockerignore, no Docker references in Readme/docs/CI scripts; msc tag reachable; aws/, Vagrantfile, experiments/ untouched | ✓ VERIFIED | `ls` → all three absent; `git ls-tree -r HEAD -- docker_rs3 Dockerfile` → 0 (was 12+1 at 90d06ec); `git grep -n -i docker -- . ':!.planning' ':!aws' ':!Vagrantfile' ':!experiments'` → empty, rc 1; `msc` → 3ca26a1; `git diff --quiet 90d06ec HEAD -- aws Vagrantfile experiments` holds |
| SC-2 | Following only Readme.md a developer can set up 1.99.1/edition 2024, expect the slow V8 build, configure SERVER.HOST/PORT, build, run and test | ✓ VERIFIED | Readme.md Requirements/Quick start/Configuration; replay proves banner, config lines, dotted-name pitfall; facts cross-checked against rust-toolchain.toml, Cargo.toml, src/main.rs, src/config.rs |
| SC-3 | deploy/rs3.service: unprivileged user, EnvironmentFile host/port, restart on failure, V8-tolerated sandbox; systemd-analyze verify accepts it; README install section | ⚠️ PRESENT_BEHAVIOR_UNVERIFIED | Present + wired + partially exercised: verify empty/exit 0, exposure 1.5; env(1) bridge, Restart=on-failure + start limit, seccomp kill run all live-green under the user manager. The unprivileged DynamicUser and root-only sandbox directives under V8 cannot run here → operator UAT |
| SC-4 | README documents HTTP API with status codes, op_rs3_* syntax + legacy mapping + working example linking docs/js-ops.md, Behaviour changes; states MIT; Cargo.toml license = "MIT" | ✓ VERIFIED | Readme.md:107-336; link `[docs/js-ops.md](docs/js-ops.md)` at :278; fac example replayed (`PATCH /key/fac -> 200 "120"`); `cargo metadata` license MIT; Readme.md:522-524. LICENSE file is on `main` (`git show main:LICENSE` = MIT text) and main differs from the merge base only by LICENSE, so the merge brings it cleanly |
| SC-5 | CI stays green; ci-guards, check-workflow, test-ci-guards consistent and pass; TOOL-03 gate passes | ✓ VERIFIED | Verifier ran the full D-17 gate once: ci-guards all passed [a]-[h], check-workflow rc 0, test-ci-guards `99 cases, 0 missed`, fmt rc 0, clippy -D warnings clean, `216 passed; 0 failed`, release build, `ci-smoke: OK`. `.github/`, `scripts/`, `Cargo.lock` unchanged since 90d06ec. Remote x86_64 run for this HEAD not observed (human item) |

### Observable Truths — 06-01-PLAN must_haves

| # | Truth (abridged) | Status | Evidence |
|---|------------------|--------|----------|
| 1 | Tracer: unit-live.sh bridge from the committed unit | ✓ VERIFIED | Re-run on HEAD: banner tagged rs3, GET / 200, PATCH "120", main PID = target/release/rs3 with SERVER.HOST/PORT in environ, `NoNewPrivs: 1 Seccomp: 2`, `unit-live: OK bridge`; no rs3-p6-* unit left |
| 2 | Unit content (D-07/D-08, rulings 1, 2) | ✓ VERIFIED | Every listed directive line present (`grep -qxF` set), no `^MemoryDenyWriteExecute`/`^ProcSubset`, explaining comment present, 0 lines > 100 chars. Note: no longer byte-identical to the 06-RESEARCH text — review fixes WR-03/WR-04/iter-2 WR-01/iter-3 WR-01 added `MemoryMax=1G`, `MemorySwapMax=0`, commented `IPAddressDeny/Allow` and comments (recorded in 06-REVIEW-DISPOSITION). All enumerated requirements still hold |
| 3 | verify empty/exit 0; security ≤ 1.5 | ✓ VERIFIED | Re-run: verify output empty rc 0; `Overall exposure level for rs3.service: 1.5 OK :-)` rc 0 |
| 4 | rs3.conf: exactly RS3_HOST=127.0.0.1 and RS3_PORT=5000 under explaining comments | ✓ VERIFIED | Two uncommented lines exactly; comments explain the dot rule and no authentication (one extra comment line from WR-03) |
| 5 | license = "MIT" (+1 line), Cargo.lock identical, README License, no LICENSE on branch | ✓ VERIFIED | numstat Cargo.toml `1 0`; Cargo.lock no diff vs 90d06ec; LICENSE absent on branch, present on main |
| 6 | Deployment section commands and prose | ✓ VERIFIED | All ten command lines present one per line; bridge, DynamicUser/≥1024/CAP_NET_BIND_SERVICE, MemoryDenyWriteExecute and @pkey, GLIBC_2.34, install vs cp, restart empties store, security note |
| 7 | Install dry-run + install over a running binary | ✓ VERIFIED | Verifier dry-run listing matched; `unit-live.sh update` OK |
| 8 | Seccomp kill-action run | ✓ VERIFIED | Re-run `kill`: 15 directives, nested PATCH "120", 422 js_depth_limit, 504 after 5011 ms, NRestarts=0, clean stop without SIGSYS |
| 9 | Start limit on empty RS3_PORT | ✓ VERIFIED | Re-run `startlimit`: failed after NRestarts=5, "Start request repeated too quickly.", journal tagged rs3, Started before error, still failed 5 s later |
| 10 | Missing env file | ✓ VERIFIED | Re-run `nofile`: "Failed to load environment files: No such file or directory", failed after NRestarts=5 |
| 11 | Troubleshooting subsection | ✓ VERIFIED | Readme.md:447-489 quotes exactly the observed lines; Type=exec caveat; reset-failed + restart (cited); crash → 2 s restart with empty store |
| 12 | Pending operator UAT recorded, nothing claimed verified | ✓ VERIFIED | 06-01-SUMMARY.md "Pending operator UAT", result [pending] |
| 13 | DEPL-01 deletion and .gitignore (0 added, 1 deleted) | ✓ VERIFIED | numstat `.gitignore 0 1`; 12 docker_rs3 files + Dockerfile gone |
| 14 | MODERNIZATION_BRIEF ≤ 3 hunks, item 9 re-scoped | ✓ VERIFIED | 2 hunks; CI-only problem bullet; item 9 with "Re-scoped on 2026-10-21"; out-of-scope line lists experiments/, aws/, Vagrantfile |
| 15 | Container-tool grep empty; msc sentence; msc tag; MSc paths unchanged | ✓ VERIFIED | See SC-1; Readme.md:9-11 has both sentences |
| 16 | D-17 gate | ✓ VERIFIED | See SC-5 |
| 17 | 06-VALIDATION rows 06-01-01..03 green | ✓ VERIFIED | Rows green. (The "status stays draft" clause was superseded later by validate-phase, which owns it: now `validated`, `nyquist_compliant: true`) |

### Observable Truths — 06-02-PLAN must_haves

| # | Truth (abridged) | Status | Evidence |
|---|------------------|--------|----------|
| 1 | Tracer: readme-replay.sh OK, no rs3 left | ✓ VERIFIED | Re-run on HEAD: `readme-replay: OK (130 checks)`, rc 0, rs3 processes 0 before/after. Helper compares status/body/ctype/stderr exactly and greps each documented text in Readme.md (read: `expect`, `documented`, `cfgline`) |
| 2 | Requirements section | ✓ VERIFIED | Readme.md:13-29 |
| 3 | Quick start lines + dotted-name prose + env.example | ✓ VERIFIED | Readme.md:31-62 |
| 4 | Configuration: case-insensitive, sources, no defaults, BOM, error table, banner after bind, SIGTERM | ✓ VERIFIED | Readme.md:64-105; table lines replayed; SIGTERM → exit 0 replayed |
| 5 | HTTP API route/error tables, body shape, "unchanged since 2021" | ✓ VERIFIED | Readme.md:107-185; codes match src/http/error.rs:74-80 |
| 6 | Boundary 262144/262145, depth 4/5 | ✓ VERIFIED | Replay: `POST /insert 262145 bytes -> 413 ...`, k1..k4 "leaf", j1..j5 422 |
| 7 | Adjacency: byte order, overwrite | ✓ VERIFIED | Replay + Readme.md:132-152 |
| 8 | Empty store / empty insert / empty body | ✓ VERIFIED | Replay + README tables |
| 9 | Ordering: echo in body order vs sorted list | ✓ VERIFIED | Replay + Readme.md:141-150 |
| 10 | Precision: port 70000, 0.1+0.2 | ✓ VERIFIED | Replay `SERVER.PORT=70000 -> exit 1`; "0.30000000000000004" |
| 11 | Concurrency: writes kept after 504; Limits statements | ✓ VERIFIED | Replay `GET /key/w2 after the timeout -> 200 "kept"`; Readme.md:298-307 |
| 12 | JavaScript ops section | ✓ VERIFIED | Readme.md:187-278; legacy 422 bodies, fac, String(result) table, print streams replayed |
| 13 | Limits section | ✓ VERIFIED | Readme.md:280-307; matches `JsSettings::default` (src/js/mod.rs: max_patch_depth 4, timeout_ms 5000) |
| 14 | Behaviour changes (8 items, no 413/Content-Type) | ✓ VERIFIED | Readme.md:309-336; section grep for 413/content-type/262144 → 0; spot check `git show msc:src/main.rs` confirms banner-before-bind and unwrap |
| 15 | IN-03 + IN-07 in docs/js-ops.md | ✓ VERIFIED | Flattened grep: "raw, unescaped", `js stderr: ` escaped, RangeError stdout exception; the old faking claim gone (only the accurate stderr claim remains). (`verify.artifacts` reported "Missing pattern: raw, unescaped" — false negative: the phrase wraps across two lines) |
| 16 | IN-02 ops rows, after-timeout TypeError, siblings, worker queueing | ✓ VERIFIED | docs/js-ops.md:27-30 rows; flattened greps present |
| 17 | IN-05 comment only | ✓ VERIFIED | `git diff 90d06ec -- src/js/executor.rs` is +2/-1 comment lines; fmt clean |
| 18 | Development section | ✓ VERIFIED | Readme.md:491-520. Deviation via review WR-02: `python3 scripts/check-workflow.py` is no longer its own fenced line; README says it runs inside ci-guards.sh as guard (g) (confirmed scripts/ci-guards.sh:212,222) and lists test-ci-guards as local-only (ci.yml has no such step). More accurate than the plan text |
| 19 | Heading order, no container word | ✓ VERIFIED | `## Requirements|## Quick start|## Configuration|## HTTP API|## JavaScript ops|## Limits|## Behaviour changes|## Deployment|## Development|## License` |
| 20 | D-17 at end + replay + bridge on final tree; scope unchanged | ✓ VERIFIED | Gate, replay, bridge all re-run green on HEAD 0ee6593; `git diff --stat 90d06ec HEAD -- src Cargo.lock .github scripts aws Vagrantfile experiments env.example requests` → only src/js/executor.rs |
| 21 | 06-VALIDATION complete | ✓ VERIFIED | Six rows green, wave_0_complete true |

**Score:** 42/43 truths verified (1 present, behavior-unverified: SC-3's unprivileged-user and root-only sandbox part)

### Required Artifacts

| Artifact | Expected | Status | Details |
|----------|----------|--------|---------|
| `deploy/rs3.service` | Hardened example unit | ✓ VERIFIED | verify.artifacts pass; live-run (user manager) in 5 modes + verifier MemoryMax run |
| `deploy/rs3.conf` | Env file RS3_HOST/RS3_PORT | ✓ VERIFIED | Consumed by the live runs (derived copy) |
| `Cargo.toml` | license MIT | ✓ VERIFIED | cargo metadata → MIT |
| `Readme.md` | Ten sections | ✓ VERIFIED | 525 lines, replayed |
| `MODERNIZATION_BRIEF.md` | Item 9 re-scoped | ✓ VERIFIED | |
| `.gitignore` | Dead line removed | ✓ VERIFIED | |
| `docs/js-ops.md` | IN-02/03/07 | ✓ VERIFIED | tool false negative on wrapped phrase, confirmed by flattened grep |
| `src/js/executor.rs` | IN-05 comment | ✓ VERIFIED | |
| `06-VALIDATION.md` | Rows green | ✓ VERIFIED | |

### Key Link Verification

| From | To | Via | Status | Details |
|------|----|-----|--------|---------|
| deploy/rs3.service | deploy/rs3.conf | EnvironmentFile=/etc/rs3/rs3.conf | ✓ WIRED | Live: values arrive in /proc/<pid>/environ |
| deploy/rs3.service | src/config.rs SERVER_KEYS | env(1) bridge in ExecStart | ✓ WIRED | Live: `SERVER.HOST=127.0.0.1`, `SERVER.PORT=<port>` in main PID environ |
| Readme.md | deploy/rs3.service | install commands | ✓ WIRED | Dry-run reproduces the layout |
| deploy/rs3.service | src/js/mod.rs init_platform | `@system-service @pkey` | ✓ WIRED (aarch64) | V8 runs under the kill-action filter; x86_64 PKU → human |
| Readme.md | docs/js-ops.md | link | ✓ WIRED | Readme.md:278 |
| Readme.md error table | src/http/error.rs ApiError::code | codes/bodies | ✓ WIRED | Manual: error.rs:74-80 codes; bodies replayed (tool could not parse the descriptor `from`) |
| Readme.md Limits | src/js/mod.rs JsSettings::default | 4 / 5000 ms | ✓ WIRED | Manual: max_patch_depth 4, timeout_ms 5000 |
| Readme.md Configuration | src/main.rs error lines | `error:` lines | ✓ WIRED | Manual: main.rs:11,20,34,38,42; lines replayed |

### Data-Flow Trace (Level 4)

Not applicable in the UI sense (documentation + config phase). The equivalent trace — documented text ← live binary output — is the replay: every documented body/line is produced by `target/release/rs3` and then found verbatim in Readme.md (130 checks).

### Behavioral Spot-Checks

| Behavior | Command | Result | Status |
|----------|---------|--------|--------|
| Unit accepted by systemd | `systemd-analyze verify --recursive-errors=no deploy/rs3.service` | empty, rc 0 | ✓ PASS |
| Exposure score | `systemd-analyze security --offline=true --threshold=15 deploy/rs3.service` | 1.5 OK, rc 0 | ✓ PASS |
| Unit runs rs3 via bridge | `bash target/tmp/p6/unit-live.sh bridge` | OK bridge | ✓ PASS |
| V8 under kill-action seccomp | `bash target/tmp/p6/unit-live.sh kill` | OK kill | ✓ PASS |
| Start limit / missing file / update | `unit-live.sh startlimit|nofile|update` | all OK | ✓ PASS |
| README matches binary | `bash target/tmp/p6/readme-replay.sh` | OK (130 checks) | ✓ PASS |
| MemoryMax/MemorySwapMax applied, V8 still works | verifier scratch `target/tmp/verify06/memunit.sh` | memory.max=1073741824, swap.max=0, PATCH "120", Seccomp 2 | ✓ PASS |
| Cap kills and restarts with empty store (README claim) | verifier scratch `target/tmp/verify06/oom.sh` (same unit, MemoryMax lowered to 150M, 20 MB store, 4 concurrent /list) | "killed by the OOM killer", `Failed with result 'oom-kill'`, NRestarts=1, new MainPID, k1 → 404 | ✓ PASS |
| D-17 gate | full chain once | all rc 0 (216 tests) | ✓ PASS |
| Install dry-run | three `install -D` into target/tmp/verify06/root | `644,644,755` layout | ✓ PASS |

All transient units were stopped and reset; `systemctl --user list-units 'rs3-*'` empty and `pgrep -cx rs3` = 0 afterwards.

### Probe Execution

No `scripts/*/tests/probe-*.sh` exist and none are declared. The phase's runnable checks are the scratch helpers above, re-run by the verifier in its own process.

### Requirements Coverage

| Requirement | Source Plan | Description | Status | Evidence |
|-------------|-------------|-------------|--------|----------|
| DEPL-01 | 06-01 | Container deployment removed, no references | ✓ SATISFIED | SC-1 |
| DEPL-02 | 06-01 | README release build + install section | ✓ SATISFIED | Readme.md:338-364, dry-run |
| DEPL-03 | 06-01 | Hardened unit: unprivileged, EnvironmentFile, restart, V8-compatible sandbox | ? NEEDS HUMAN (partially satisfied) | User-manager subset live-green; non-root user + root-only directives + x86_64 PKU are operator UAT |
| DOCS-01 | 06-02 | Toolchain, V8 note, config, build/run/test | ✓ SATISFIED | SC-2 |
| DOCS-02 | 06-02 | Op syntax, mapping, example, js-ops link | ✓ SATISFIED | Readme.md:187-278 |
| DOCS-03 | 06-02 | HTTP API, status codes, Behaviour changes | ✓ SATISFIED | Readme.md:107-185, 309-336 |
| DOCS-04 | 06-01 | license = "MIT", README states MIT | ✓ SATISFIED | Cargo.toml:7, Readme.md:522-524 |

All seven phase IDs are claimed by a plan; REQUIREMENTS.md maps no other ID to Phase 6. No orphans.

### Prohibitions (judgment tier — non-authoritative verdicts, flagged for human review)

| Plan | Requirement | Prohibition | Verifier verdict | Flag |
|------|-------------|-------------|------------------|------|
| 06-01 | DEPL-03 | No claim that root-only directives / non-root user / x86_64 PKU were verified | Holds: both SUMMARYs list them as `[pending]`; README makes no such claim | unverified-prohibition — human review recommended |
| 06-01 | DEPL-03 | No exposure beyond loopback by default; no-auth warning present | Holds: rs3.conf `RS3_HOST=127.0.0.1`; Readme.md:432-445 | unverified-prohibition — human review recommended |
| 06-01 | DEPL-01 | No history rewrite, msc intact, MSc paths untouched, no push, no git config change | Holds: msc → 3ca26a1; aws/Vagrantfile/experiments no diff; branch ahead of origin by 47 (not pushed); no local user.* config | unverified-prohibition — human review recommended |
| 06-02 | DOCS-03 | Unchanged behaviour not presented as change; only observed bodies (except labelled 500 / dotenv lines) | Holds: 413/Content-Type/Promise outside Behaviour changes; 130-check replay; 500 and dotenv lines labelled as from code | unverified-prohibition — human review recommended |
| 06-02 | DOCS-02 | Never claims stdout is escaped, scripts cannot forge output, or limits are runtime-configurable | Holds: "raw, unescaped"; "Scripts are untrusted"; "cannot be configured at runtime" | unverified-prohibition — human review recommended |
| 06-02 | DOCS-01 | No push / git config / ci.yml / scripts edits; run examples bind 127.0.0.1 | Holds: .github and scripts unchanged since 90d06ec; examples use 127.0.0.1 (env.example uses localhost) | unverified-prohibition — human review recommended |

### Anti-Patterns Found

| File | Line | Pattern | Severity | Impact |
|------|------|---------|----------|--------|
| (all modified files) | — | TBD/FIXME/XXX/TODO/HACK/placeholder | — | None found |
| Readme.md | 397-404 | MemoryMax= sizing rule says concurrent `GET /list` or `op_rs3_list()` copies are "up to one per CPU" combined. The HTTP and JS pools are separate (actix default workers and `JsSettings.workers` both = available_parallelism), so mixed load reaches up to two per CPU. Measured here (4 CPUs, 20 MB store): 4 /list + 4 op_rs3_list PATCHes = +400 MB ≈ 20× store, versus the rule's 1 + 4×4 = 17× | ⚠️ Warning | A sizing heuristic outside the must-haves, already hedged ("rough", "well below"). An operator who sizes the store near the stated bound could still hit an OOM kill (and lose the store) under mixed load. Routed to human decision; the iteration-3 fix itself was marked "requires human verification" |
| deploy/rs3.service | 22-26 | MemoryMax/MemorySwapMax not carried by the unit-live helper's directive list | ℹ️ Info | Closed by the verifier's own runs (memunit.sh, oom.sh) under the user manager |
| target/tmp/p6/*.sh | — | Replay and unit-live proofs are uncommitted scratch | ℹ️ Info | Deliberate (plan). No regression guard keeps the README in step with the binary after this phase |

### Human Verification Required

### 1. Operator UAT: root-only unit on a real server

**Test:** Follow 06-01-SUMMARY "Pending operator UAT" steps 1-7 as root.
**Expected:** rs3 active under the DynamicUser (not root); "Welcome to rs3"; PATCH "120"; no `status=31/SYS`; exposure about 1.5.
**Why human:** No sudo here, and unprivileged user namespaces are blocked; the user manager refuses or ignores the root-only directives.

### 2. x86_64 with PKU under the seccomp filter

**Test:** Same as 1 on an x86_64 CPU with PKU, including a nested PATCH.
**Expected:** No SIGSYS or EPERM failure from the `pkey_*` calls.
**Why human:** All seccomp evidence is from aarch64, which has no PKU.

### 3. README Update / Troubleshooting commands with sudo

**Test:** Install over the running binary and restart; break the env file, observe the start limit, recover with `reset-failed` + `restart`.
**Expected:** As documented.
**Why human:** Needs the system manager and sudo; reset-failed is cited, not run.

### 4. MemoryMax= sizing paragraph

**Test:** Read Readme.md:397-404 against the measurements above.
**Expected:** Either accept the "rough" rule as is, or reword the concurrency bound to count GET /list and op_rs3_list() separately (about MemoryMax / (1 + 8 × CPUs) worst case).
**Why human:** A judgement call on documentation outside the must-haves.

### 5. README readability

**Test:** Read the README end to end as a newcomer.
**Expected:** A newcomer can build, configure, run, call, deploy and extend rs3 without reading the source.
**Why human:** Prose quality is a judgement call.

### 6. Remote CI on HEAD

**Test:** After the operator pushes, check the GitHub Actions run.
**Expected:** Green on ubuntu-26.04 with zero annotations.
**Why human:** The remote run cannot be observed from here.

### 7. Judgment-tier prohibitions

**Test:** Confirm the six verdicts in the Prohibitions table.
**Expected:** All hold.
**Why human:** These verdicts are non-authoritative.

### Gaps Summary

There are no blocking gaps. Every artifact exists, is substantive and is wired. Every key link holds, and the verifier re-ran all of these on HEAD 0ee6593:
- the full D-17 gate;
- the 130-check README replay;
- all five unit-live modes;
- two extra MemoryMax runs, including a real OOM kill followed by a restart with an empty store.

Seven of the eight user-flow steps are proven by the verifier's own runs. The remaining step is the non-root, root-only-sandbox part of DEPL-03/SC-3, which cannot be exercised without root on this host. It stays the pending operator UAT, so the status is `human_needed` rather than `passed`.

There is one warning-level documentation finding: the MemoryMax= sizing rule undercounts mixed `/list` plus `op_rs3_list` load by about 20% on this host. It is listed for a human decision, not as a gap.

Two plan must-haves were superseded by recorded review fixes (the unit is no longer the byte-exact research text, and the Development section wording changed). In both cases the substance of the truth still holds.

---

_Verified: 2026-10-21T11:43:41Z_
_Verifier: Claude (gsd-verifier)_
