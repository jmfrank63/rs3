---
status: testing
phase: 06-native-deployment-documentation
source: [06-VERIFICATION.md, 06-01-SUMMARY.md]
started: 2026-10-21T11:50:00Z
updated: 2026-10-21T11:50:00Z
---

## Current Test

number: 1
name: Operator UAT — the root-only unit on a real server
expected: |
  rs3 active under the DynamicUser (not root); GET / answers "Welcome to rs3"; PATCH of 6*20 answers "120";
  no status=31/SYS line in the journal; systemd-analyze security rs3 reports an exposure of about 1.5.
awaiting: user response

## Tests

### 1. Operator UAT — the root-only unit on a real server
expected: On a Linux server with systemd, as root, install with the five README Deployment lines (`sudo install -m 0755 target/release/rs3 /usr/local/bin/rs3`, `sudo install -D -m 0644 deploy/rs3.conf /etc/rs3/rs3.conf`, `sudo install -m 0644 deploy/rs3.service /etc/systemd/system/rs3.service`, `sudo systemctl daemon-reload`, `sudo systemctl enable --now rs3`). Then `systemctl status rs3` is active; `curl -s http://127.0.0.1:5000/` answers `"Welcome to rs3"`; `curl -s -X POST http://127.0.0.1:5000/insert -d '{"m":"6*20"}'` then `curl -s -X PATCH http://127.0.0.1:5000/key/m` answers `"120"`; `ps -o user= -p "$(systemctl show -p MainPID --value rs3)"` is not root; `journalctl -u rs3 | grep -c 'status=31/SYS'` is 0; `systemd-analyze security rs3` reports about 1.5. This proves the root-only directives (DynamicUser, ProtectSystem/Home, PrivateTmp/Devices, ProtectKernel*, ProtectProc, empty capability sets, MemoryMax/MemorySwapMax), which the user manager on the dev host refuses or ignores.
result: [pending]

### 2. The same unit on an x86_64 CPU with PKU (seccomp `@system-service @pkey`)
expected: Repeat test 1 on an x86_64 host whose CPU has memory protection keys, including a nested PATCH (store `n` = `Deno.core.ops.op_rs3_patch("m")` and PATCH `/key/n`). Both answer `"120"`, and there is no SIGSYS or EPERM failure from `pkey_alloc` / `pkey_mprotect`. All seccomp evidence so far is from aarch64, which has no PKU.
result: [pending]

### 3. README Update and Troubleshooting commands with sudo
expected: On the real server, `sudo install -m 0755 target/release/rs3 /usr/local/bin/rs3` over the running binary followed by `sudo systemctl restart rs3` replaces it; then break `/etc/rs3/rs3.conf` (e.g. `RS3_PORT=`), restart, observe `Start request repeated too quickly.` after 5 failed starts, fix the file, and recover with `sudo systemctl reset-failed rs3 && sudo systemctl restart rs3`. All as documented (reset-failed is cited from systemd.unit 255, not live-run).
result: [pending]

### 4. MemoryMax= sizing paragraph (decision)
expected: Read Readme.md "Service user and sandbox" (the MemoryMax= sizing paragraph) against the verifier's measurements on a 4-CPU host with a 20 MB store (1 GET /list +77 MB; 4 concurrent GET /list +307 MB; 4 concurrent PATCH op_rs3_list +324 MB; both together +400 MB, about 20x the store). Either accept the "rough" rule as is, or reword the concurrency bound to count GET /list (actix workers) and op_rs3_list() (JS workers) separately, which tightens it to about MemoryMax / (1 + 8 x CPUs) in the worst case.
result: [pending]

### 5. README readability for a newcomer
expected: Read Readme.md end to end as a newcomer; it is enough to build, configure, run, call, deploy and extend rs3 without reading the source.
result: [pending]

### 6. Remote CI on this HEAD
expected: After the operator pushes `modernize-toolchain`, the GitHub Actions `CI` run on ubuntu-26.04 is green with zero annotations (guards, fmt, clippy, 216 tests, release build, release smoke).
result: [pending]

### 7. Judgment-tier prohibitions (non-authoritative verifier verdicts)
expected: Confirm the six verdicts in 06-VERIFICATION.md "Prohibitions" hold: (1) no claim that the root-only directives, non-root user or x86_64 PKU were verified; (2) loopback by default with the no-auth warning present; (3) no history rewrite, `msc` intact, MSc-era paths untouched, no push, no git config change; (4) unchanged behaviour not presented as a change, and only observed bodies documented (the 500 and dotenv lines are labelled as taken from the code); (5) the docs never claim stdout is escaped, that scripts cannot forge output, or that the limits are runtime-configurable; (6) no ci.yml or scripts edits, and run examples bind 127.0.0.1.
result: [pending]

## Summary

total: 7
passed: 0
issues: 0
pending: 7
skipped: 0
blocked: 0

## Gaps
