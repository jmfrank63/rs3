# Phase 6: Native Deployment & Documentation - Pattern Map

**Mapped:** 2026-10-21
**Files analyzed:** 12 (2 deletions, 2 new deploy files, 6 edits, 2 scratch helpers)
**Analogs found:** 9 / 12 (the systemd unit, its env file and the deletions have no in-repo analog; use the RESEARCH.md text verbatim)

All analogs below are git-tracked (`git ls-files` verified). `.claude/` is gitignored local tooling and is not an analog.

## File Classification

| File | Action | Role | Data Flow | Closest Analog | Match Quality |
|------|--------|------|-----------|----------------|---------------|
| `Dockerfile` | delete | config | n/a | none | n/a |
| `docker_rs3/` (11 tracked files) | delete | config | n/a | none | n/a |
| `.gitignore` | edit (drop line 13 `/docker_rs3/.secrets/`) | config | n/a | itself | exact |
| `MODERNIZATION_BRIEF.md` | edit lines 37, 68-69, 72-74 | doc | n/a | itself; text in RESEARCH.md §MODERNIZATION_BRIEF re-wording | exact |
| `Cargo.toml` | edit (+`license = "MIT"` after line 6 `rust-version`) | config | n/a | itself (`[package]` lines 1-6) | exact |
| `docs/js-ops.md` | edit (IN-03, IN-07 lines 199-206; IN-02 table rows 25-28, lines 13, 101-102, 210, 222-225) | doc | n/a | itself | exact |
| `src/js/executor.rs` | edit comment lines 147-148 (IN-05) | utility | n/a | itself | exact |
| `deploy/rs3.service` | create | config (systemd unit) | process lifecycle | none in repo; RESEARCH.md §Recommended deploy/rs3.service (lines 343-403) | no analog |
| `deploy/rs3.conf` | create | config (env file) | file-I/O (read by PID 1) | `env.example` (shape) + RESEARCH.md §Code Examples (lines 532-540) | partial |
| `Readme.md` | rewrite | doc | n/a | `docs/js-ops.md` (house style); facts from RESEARCH.md §README facts | role-match |
| `target/tmp/.../unit-live.sh` (not committed) | create | test (scratch) | request-response | `scripts/ci-smoke.sh` | exact |
| `target/tmp/.../readme-replay.sh` (not committed) | create | test (scratch) | request-response | `scripts/ci-smoke.sh` | exact |

## Pattern Assignments

### `deploy/rs3.service` (config, no analog)

Copy the unit verbatim from `06-RESEARCH.md` lines 343-403 (verified `systemd-analyze verify --recursive-errors=no` empty/exit 0, exposure 1.5). Comment style: short `#` lines, max 100 columns, no word "Docker". Hard rules: no `MemoryDenyWriteExecute`, keep `@pkey`, `${RS3_HOST}` with braces, `EnvironmentFile=` without `-` prefix, include `StartLimitIntervalSec=60`/`StartLimitBurst=5`, `SyslogIdentifier=rs3`, `ProtectProc=invisible`, omit `ProcSubset=`.

Load-bearing lines:
```ini
EnvironmentFile=/etc/rs3/rs3.conf
ExecStart=/usr/bin/env SERVER.HOST=${RS3_HOST} SERVER.PORT=${RS3_PORT} /usr/local/bin/rs3
SystemCallFilter=@system-service @pkey
```

### `deploy/rs3.conf` (env file)

**Analog shape:** `env.example` (2 lines, `KEY=value`, no quoting):
```
SERVER.HOST=localhost
SERVER.PORT=5000
```
Use RESEARCH.md lines 532-540: a 5-line `#` comment header (why not `SERVER.*`, loopback/no-auth note), then `RS3_HOST=127.0.0.1` / `RS3_PORT=5000`. Never a `.env` suffix.

### `Readme.md` (doc, rewrite; currently 23 lines of 2021 text)

**Style analog:** `docs/js-ops.md`
- Title `# ...`, short intro paragraph, then `## Section` headings (js-ops.md lines 1-16).
- Pipe tables with backticked code cells (js-ops.md lines 21-28: `| Old call | New call | ... |`).
- Bold lead-ins inside bullets for limits (js-ops.md lines 210, 220: `- **Nesting depth limit: 4.** ...`, `- **Execution timeout: 5000 ms.** ...`).
- Prose wrapped at about 78 columns.

**Facts source:** RESEARCH.md lines 602-716 only (bodies copied from live output). Section order per CONTEXT D-12. Link `docs/js-ops.md`, do not duplicate it. Startup error strings must match `src/main.rs`:
```rust
eprintln!("error: invalid configuration: {e}");      // line 11
eprintln!("error: cannot start JS runtime: {e}");    // line 20
eprintln!("error: cannot bind {host}:{port}: {e}");  // line 34
println!("Starting Http server at host address: {host}, with port: {port}!"); // line 38
eprintln!("error: server failed: {e}");              // line 42
```
Shell examples: always `env SERVER.HOST=127.0.0.1 SERVER.PORT=5000 ...` (copy the `env` idiom from `scripts/ci-smoke.sh` line ~104: `env -i PATH="$PATH" SERVER.HOST=127.0.0.1 SERVER.PORT="$PORT" "$BIN"`). List 413 and any-Content-Type as unchanged facts, not behaviour changes (RESEARCH Open Question 3).

### `docs/js-ops.md` (doc edits)

Edit in place, same wrapping and bullet style. Current IN-03 text to replace (lines 199-206):
```
  call, prefixed with `js stderr: ` (for example `js stderr: "a\nb"`), so a
  script cannot fake server log lines. Treat everything a script prints, on
```
Replacement wording and the IN-07 sentence: RESEARCH.md line 593-594. IN-02 four fixes: RESEARCH.md lines 597-600 (line 210 "At most 4 scripts run in one PATCH chain" and lines 224-225 "Other routes and other PATCH requests keep being answered meanwhile" must disappear).

### `src/js/executor.rs` (comment only, lines 147-148)

Current:
```rust
                    // Spawned from the worker, so the watchdog descends from the
                    // thread that initialised V8, like the worker itself.
```
Replace with RESEARCH.md lines 585-587 (must contain `main thread that initialised V8`). Keep the 20-space indentation; no code change; `cargo fmt --check` must stay clean.

### `Cargo.toml`

Current `[package]` (lines 1-6):
```toml
[package]
name = "rs3"
version = "0.2.0"
authors = ["johannes"]
edition = "2024"
rust-version = "1.99"
```
Add `license = "MIT"` as line 7. `Cargo.lock` must stay byte-identical.

### `.gitignore`

Delete only line 13 `/docker_rs3/.secrets/`; keep the `/aws/...` block and the `# Local agent tooling` block intact.

### `MODERNIZATION_BRIEF.md`

Current lines 37, 68, 72 contain `Dockerfile uses \`rust:1.53\``, `9. Update the Dockerfile ...`, `` `docker_rs3/` ``. Replace with the exact proposed text in RESEARCH.md lines 566-575. Afterwards `grep -ci docker MODERNIZATION_BRIEF.md` -> 0.

### Scratch helpers under `target/tmp` (test, request-response; NOT committed)

**Analog:** `scripts/ci-smoke.sh`

Header + strict mode + root resolution (lines 1-2, ~36-38):
```bash
#!/usr/bin/env bash
set -euo pipefail
ROOT="${CI_SMOKE_ROOT:-$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)}"
BIN="${RS3_BIN:-target/release/rs3}"
PORT="${RS3_SMOKE_PORT:-$(python3 -c 'import socket; s=socket.socket(); s.bind(("127.0.0.1", 0)); print(s.getsockname()[1])')}"
```
Cleanup via trap (SIGTERM, then SIGKILL after 5 s):
```bash
cleanup() { ... kill "$PID"; for i in $(seq 1 50); do kill -0 "$PID" 2>/dev/null || break; sleep 0.1; done; kill -9 "$PID" ...; }
trap cleanup EXIT
```
For the user-manager unit, the trap is `systemctl --user stop "$NAME"` plus `--collect` on `systemd-run` (RESEARCH Pattern 1, lines 423-447).

Readiness poll + exact-body assert:
```bash
if body=$(curl -s --fail --max-time 2 "$BASE/"); then ready=1; break; fi
[[ "$body" == '"Welcome to rs3"' ]] || fail "GET / body: ${body}"
```
Request helper (status + body split):
```bash
req() {
  local out; local -a data=()
  if (( $# > 2 )); then data=(--data-binary "$3"); fi
  out=$(curl -sS --max-time 30 -w $'\n%{http_code}' -X "$1" ${data[@]+"${data[@]}"} "$BASE$2") || fail "$1 $2: transport error"
  CODE="${out##*$'\n'}"; BODY="${out%$'\n'*}"
}
```
- `unit-live.sh`: derive props from `deploy/rs3.service` (RESEARCH Pattern 1 script verbatim); assert banner via `journalctl --user -u "$NAME" -o cat`, `GET /`, PATCH `"120"`, `Seccomp: 2`; optional kill-action variant without `SystemCallErrorNumber`.
- `readme-replay.sh`: start the release binary with `env SERVER.HOST=127.0.0.1 SERVER.PORT=$PORT`, use `req` to compare every README body to RESEARCH tables (lines 644-671). Never reference the dotenv file by name.
- Scratch dirs live below `target/tmp/` (convention from `scripts/test-ci-guards.sh` lines 8-10; `/tmp` is nearly full).

## Shared Patterns

### Config via `env` for dotted names
**Source:** `scripts/ci-smoke.sh` (env -i launch line) and `src/config.rs` line 16 (`SERVER_KEYS`)
**Apply to:** README examples, unit ExecStart, scratch helpers. Bash cannot assign dotted names; always prefix with `env`.

### Exact-body assertions, loopback only
**Source:** `scripts/ci-smoke.sh`
**Apply to:** both scratch helpers and the README replay.

### Doc style
**Source:** `docs/js-ops.md`
**Apply to:** `Readme.md`, `docs/js-ops.md` edits, `MODERNIZATION_BRIEF.md` edits: ~78-column wrapping, pipe tables, backticked identifiers, no "Docker" anywhere new.

### Gate after every plan
D-17 list (CONTEXT lines 225-232) plus `systemd-analyze verify --recursive-errors=no deploy/rs3.service` (empty output, exit 0) and `git grep -n -i docker -- . ':!.planning' ':!aws' ':!Vagrantfile' ':!experiments'` (empty).

## No Analog Found

| File | Role | Reason | Use instead |
|------|------|--------|-------------|
| `deploy/rs3.service` | systemd unit | no deployment config in repo after Docker removal | RESEARCH.md lines 343-403 verbatim |
| `deploy/rs3.conf` | env file | only `env.example` (dotted names, not systemd-safe) | RESEARCH.md lines 532-540 |
| `Dockerfile`, `docker_rs3/` | deletions | `git rm -r` only | n/a |

## Metadata

**Analog search scope:** all git-tracked files outside `.planning/` (scripts/, src/, docs/, root config)
**Files scanned:** 8
**Pattern extraction date:** 2026-10-21
