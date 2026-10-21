# Phase 6: Native Deployment & Documentation - Research

**Researched:** 2026-10-21
**Domain:** systemd service hardening for an embedded-V8 Rust binary, README/API documentation, repo cleanup under a strict CI contract
**Confidence:** HIGH. Every unit directive, response body, error line and CI interaction below was checked on this host (systemd 255, aarch64) against the current release binary. The x86_64 behaviour and the root-only directives cannot be run here; they are flagged and routed to operator UAT.

<user_constraints>
## User Constraints (from CONTEXT.md)

### Locked Decisions

#### Docker removal and history
- **D-01:** Delete `Dockerfile` and all of `docker_rs3/`. Remove the now-dead
  `/docker_rs3/.secrets/` line from `.gitignore`. Do not add a `.dockerignore`. A grep of
  `scripts/` and `.github/` found no reference to either, so guards (a)–(h),
  `check-workflow.py` and the self-test are unaffected.
- **D-02:** Point to the 2021 version with one neutral sentence in the README, without the word
  "Docker". For example: "The original 2021 MSc version, including its multi-container
  deployment setup, is preserved at git tag `msc`."
- **D-03:** Update `MODERNIZATION_BRIEF.md` in place, minimally, because it is the milestone's
  input brief and lists the Docker item as a requirement:
  - re-word item 9 to the native deployment requirement, with a dated note ("re-scoped on
    2026-10-21; see `.planning/PROJECT.md` Key Decisions");
  - drop the "Dockerfile uses `rust:1.53`" clause from "The problem" and keep the CI part;
  - drop `docker_rs3/` Ansible content from the out-of-scope line.

  Do not rewrite anything else.
- **D-04:** `.planning/research/*` and earlier phase artifacts are dated records. Leave them
  untouched. Acceptance check: after the phase,
  `git grep -n -i docker -- . ':!.planning' ':!aws' ':!Vagrantfile' ':!experiments'` prints
  nothing.

#### systemd unit (`deploy/rs3.service`)
- **D-05 Config bridge (verified on systemd 255 on this host):** systemd ignores variable names
  that contain a dot. In `Environment=` / `EnvironmentFile=` it logs "Ignoring invalid
  environment assignment 'SERVER.HOST=…'". The unit therefore uses:
  - `EnvironmentFile=/etc/rs3/rs3.conf` holding `RS3_HOST=` / `RS3_PORT=`;
  - `ExecStart=/usr/bin/env SERVER.HOST=${RS3_HOST} SERVER.PORT=${RS3_PORT} /usr/local/bin/rs3`.

  systemd expands `${VAR}` into exactly one argument. A transient unit with this exact
  ExecStart printed `Starting Http server at host address: 127.0.0.1, with port: 18611!`, and
  answered `GET /` → `"Welcome to rs3"`, PATCH `6*20` → `"120"` and a recursive factorial →
  `"3628800"`.

  An empty or missing value makes rs3 exit 1 with one `error:` line (verified):
  - `cannot bind :18612: …`
  - `invalid type: string "", expected an integer for key server.port`
  - `missing configuration field "server"`

  So a broken file shows up in `journalctl`, and `Restart=on-failure` stops at systemd's start
  limit. No code change. Comments in both the unit and the README explain the bridge in one or
  two sentences.
- **D-06 Unprivileged user:** use `DynamicUser=yes` with `User=rs3`. rs3 keeps no state and owns
  no files, so no `useradd` step is needed. Note in a comment that systemd uses a pre-existing
  static `rs3` user instead if one exists. Grant no capabilities, so the service can only bind
  ports of 1024 and above. The example environment file binds `127.0.0.1:5000`.
- **D-07 Sandboxing:** use a strict set:
  - `NoNewPrivileges=yes`, `ProtectSystem=strict`, `ProtectHome=yes`, `PrivateTmp=yes`,
    `PrivateDevices=yes`;
  - `ProtectKernelTunables=yes`, `ProtectKernelModules=yes`, `ProtectKernelLogs=yes`,
    `ProtectControlGroups=yes`, `ProtectClock=yes`, `ProtectHostname=yes`;
  - `RestrictNamespaces=yes`, `RestrictRealtime=yes`, `RestrictSUIDSGID=yes`,
    `LockPersonality=yes`, `RemoveIPC=yes`;
  - `CapabilityBoundingSet=`, `AmbientCapabilities=`;
  - `RestrictAddressFamilies=AF_UNIX AF_INET AF_INET6`, `SystemCallArchitectures=native`,
    `SystemCallFilter=@system-service @pkey`, `SystemCallErrorNumber=EPERM`, `UMask=0077`.

  Hard rules, each with evidence from this host:
  - **Never `MemoryDenyWriteExecute=yes`.** With it, a transient unit core-dumped (SIGTRAP) on
    the first PATCH, because V8's JIT needs writable+executable memory.
  - **`@pkey` must be explicit.** systemd 255's `@system-service` does not include `pkey_alloc`,
    `pkey_free` or `pkey_mprotect`, and V8 calls them on PKU-capable x86_64 CPUs. Without them
    the filter's default action would kill the process.
  - The seccomp/address-family/personality subset above was live-tested under the user manager
    with V8 executing scripts.

  `ProtectProc=` / `ProcSubset=` are Claude's discretion after research: leave them out unless
  research shows that V8, glibc and Rust std read nothing they would hide. Every directive must
  pass `systemd-analyze verify` on systemd 255.
- **D-08 Lifecycle:**
  - `[Unit]`: `Description=`, `Documentation=https://github.com/jmfrank63/rs3`,
    `After=network-online.target`, `Wants=network-online.target`;
  - `[Service]`: `Type=exec`, `Restart=on-failure`, `RestartSec=2`;
  - `[Install]`: `WantedBy=multi-user.target`.

  Keep the unit short. Comments cover the bridge, why there is no `MemoryDenyWriteExecute`,
  why `@pkey` is listed, and that the service has no capabilities (port of 1024 or above).

#### Install and verification
- **D-09 Files:**
  - `deploy/rs3.service`;
  - `deploy/rs3.conf`, an example environment file with `RS3_HOST=127.0.0.1` and
    `RS3_PORT=5000`, plus a comment explaining why the names are not `SERVER.*`.

  No installer script and no packaging. Do not name the example file with a `.env` suffix,
  because the project hook treats such names as secrets.
- **D-10 README "Deployment" section:**
  - build with `cargo build --release --locked`, which produces `target/release/rs3`. The binary
    dynamically links only glibc/libm/libgcc_s, with V8 statically linked. Build on the server,
    or on a machine with the same or an older glibc.
  - install with `sudo install -m 0755 target/release/rs3 /usr/local/bin/rs3`,
    `sudo install -D -m 0644 deploy/rs3.conf /etc/rs3/rs3.conf` and
    `sudo install -m 0644 deploy/rs3.service /etc/systemd/system/rs3.service`, then run
    `sudo systemctl daemon-reload` and `sudo systemctl enable --now rs3`.
  - check with `systemctl status rs3`, `curl -s http://127.0.0.1:5000/` and `journalctl -u rs3`.
  - update by rebuilding, re-installing the binary and running `systemctl restart rs3`.
  - security note: rs3 has no authentication, and anyone who can reach the port can store and
    run JavaScript. Keep the loopback default, or put something you control in front (a reverse
    proxy with auth/TLS). TLS is not built in.
- **D-11 Verification without root.** The dev host has no sudo, and Ubuntu 24.04 blocks
  unprivileged user namespaces (`kernel.apparmor_restrict_unprivileged_userns = 1`). A user-manager
  unit with `ProtectSystem=` etc. fails with `218/CAPABILITIES`, even with `PrivateUsers=yes`.
  Therefore:
  - (a) `systemd-analyze verify deploy/rs3.service` must be clean;
  - (b) record `systemd-analyze security --offline=true deploy/rs3.service` (exposure score);
  - (c) live-run the unit's ExecStart and EnvironmentFile under the user manager
    (`systemd-run --user`, with the user-mode-compatible directives) against the release binary,
    and assert the banner, `GET /` and a PATCH through V8;
  - (d) make the root-only directives an operator UAT item on a real server: install per
    README, then `GET /`, PATCH `6*20` → `"120"`, check that the main PID's user is not root,
    and record `systemd-analyze security rs3`;
  - (e) no new CI job and no new guard. ci.yml, `check-workflow.py` and the self-test stay
    untouched, so CI stays green with zero annotations.

  The README install commands may be dry-checked under a scratch prefix (for example
  `install -D` into `target/tmp/root/…`) without sudo.

#### README (`Readme.md`, keep the file name)
- **D-12 Structure:**
  1. title + overview: what rs3 is, the 2021 Birkbeck MSc origin and the `msc` tag (D-02);
  2. Requirements: Rust 1.99.1 via `rust-toolchain.toml` (rustup installs it), edition 2024,
     Linux glibc x86_64/aarch64. The first build downloads the V8 prebuilt static lib (about
     35 MB; needs network plus curl or python), so expect a slow first build and a debug
     `target/` of about 2.6 GB;
  3. Quick start: clone `https://github.com/jmfrank63/rs3`, build, run, test. Dotted names are
     not valid shell assignments, so run with `env SERVER.HOST=127.0.0.1 SERVER.PORT=5000 cargo
     run`, or copy `env.example` to a dotenv file;
  4. Configuration:
     - only `SERVER.HOST` / `SERVER.PORT` are read (case-insensitive, dotted), from the process
       env or a dotenv file in the working directory or a parent;
     - the env wins over the file, and there are no defaults;
     - a malformed or BOM-prefixed dotenv file stops startup (`error: invalid configuration: …`,
       exit 1), and a bind failure gives `error: cannot bind …`, exit 1;
     - the banner prints only after a successful bind;
  5. HTTP API: a route table with exact success bodies (legacy JSON-string envelope), and an
     error table with status, `error` code and cause: 400 `malformed_json`, 404 `not_found`,
     413 payload over 256 KiB (plain-text actix body), 422 `invalid_input` / `js_error` /
     `js_depth_limit`, 500 `internal`, 504 `js_timeout`. The body shape is
     `{"error","message"}`;
  6. JavaScript ops: the syntax, the five ops, the legacy `rs3_*` → `op_rs3_*` table, a worked
     example (insert `fac.js` + `num`, then PATCH), the stringification rule
     (`String(result)`; a Promise gives `"[object Promise]"`), and a link to `docs/js-ops.md`;
  7. Limits: depth 4 (422 `js_depth_limit` with the key chain) and timeout 5000 ms
     (504 `js_timeout`). Both are hard stops, set in code and not configurable at runtime.
     There is no heap limit (v2 JSHD-01);
  8. Behaviour changes (D-14);
  9. Deployment (D-10);
  10. Development: the TOOL-03 gate commands, `bash scripts/ci-guards.sh`,
      `bash scripts/test-ci-guards.sh`, `bash scripts/ci-smoke.sh`, and a one-paragraph CI summary;
  11. License: MIT, see `LICENSE`. The file is on `main` (9d1596b) and arrives with the merge.
- **D-13 Accuracy:**
  - every command and curl example is replayed against the release binary with env-var config.
    Never name the dotenv file in a shell command.
  - response bodies are copied from real output.
  - facts are cross-checked against `src/` and `docs/js-ops.md`.
  - the Docker-era research verified many README facts
    (`git show 718065e:.planning/phases/06-docker-image-documentation/06-RESEARCH.md`). It may
    seed research, but re-verify everything.
- **D-14 Behaviour changes since 2021.** The user's list plus the verified extras:
  - `/list` is sorted by key, with keys and values JSON-escaped. The legacy `" : "` spacing is
    kept, and the `,]` quirk is gone.
  - a missing key in GET/DELETE/PATCH gives 404 JSON `not_found` (also on an empty store),
    instead of `"[]"` or a panic.
  - `POST /insert` returns 400 `malformed_json` or 422 `invalid_input`, inserts nothing on
    error, and accepts any Content-Type.
  - payloads over 256 KiB get 413.
  - ops are renamed `rs3_*` → `op_rs3_*` and called as `Deno.core.ops.op_rs3_*(...)`. There is no
    `Deno.core.ops()` bootstrap and no `opSync`. A missing key throws a catchable `Error`, and bad
    input a `TypeError`.
  - PATCH errors: 422 `js_error`, 422 `js_depth_limit`, 504 `js_timeout`, 500 `internal`.
  - the depth limit and timeout are new.
  - a Promise result gives `"[object Promise]"`.
  - a malformed or BOM-prefixed dotenv file stops startup, and only `SERVER.*` is read.
  - the banner prints only after the bind, and a startup failure gives one error line with
    exit 1.
- **D-15 Manifest:** add `license = "MIT"` to `[package]` in `Cargo.toml`. Nothing else changes.
  `Cargo.lock` must stay byte-identical.
- **D-16 Review carry-overs.** Small doc/comment fixes:
  - 03-REVIEW IN-03: `docs/js-ops.md` must not claim that stdout output cannot fake log lines.
    stdout prints are raw, and stderr prints are escaped.
  - 05-REVIEW IN-02: fill the gaps in the `docs/js-ops.md` ops table and limits wording.
  - 05-REVIEW IN-05: the watchdog spawn comment in `src/js/executor.rs` names the wrong thread as
    the V8 initialiser.

#### CI consistency
- **D-17:** CI must stay green with zero annotations. `.github/workflows/ci.yml` is not edited.
  At the end of every plan these must exit 0 locally:
  - `bash scripts/ci-guards.sh`
  - `python3 scripts/check-workflow.py`
  - `bash scripts/test-ci-guards.sh`
  - `cargo fmt --check`
  - `cargo clippy --all-targets --locked -- -D warnings`
  - `cargo test --locked`
  - `cargo build --release --locked`
  - `bash scripts/ci-smoke.sh`

  There must be no compiler warnings and no lint-suppression attributes.

### Claude's Discretion
- Plan split and ordering; the README prose, tone and table layout.
- Exact comment wording in the unit and the env file.
- Whether to include `ProtectProc=` / `ProcSubset=` (only with research evidence).
- How the user-manager live run is scripted (a throwaway command in the plan, or a small
  helper under `target/tmp`). It is not committed as a CI script.

### Deferred Ideas (OUT OF SCOPE)
- v2 OPS-02: an underscore env alias (`SERVER__HOST`) and default host/port. This would remove
  the systemd `env(1)` bridge.
- A CI job that installs and starts the unit with sudo on the x86_64 runner, for end-to-end
  proof of the root-only directives and PKU under the seccomp filter.
- Packaging (.deb/.rpm), an installer script, and a reverse-proxy/TLS example.
- Release profile tuning (v2 OPS-04). Structured logging (v2 OPS-01) would also address
  05-REVIEW IN-01/IN-07/IN-08.
</user_constraints>

<phase_requirements>
## Phase Requirements

| ID | Description | Research Support |
|----|-------------|------------------|
| DEPL-01 | Obsolete container deployment removed: `Dockerfile` and `docker_rs3/` deleted (MSc version stays at tag `msc`), no `.dockerignore`, and docs, requirements and CI scripts no longer refer to it | §Docker removal inventory: the exact tracked files and lines, the `MODERNIZATION_BRIEF.md` re-wording, proof that no guard or script references them, `msc` is a lightweight tag on `origin`, the Vagrantfile caveat |
| DEPL-02 | `Readme.md` documents the native release build and a short install section for one Linux server | §README facts (Deployment): the release build, the glibc floor (`GLIBC_2.34` on this aarch64 build), why `install` works over a running binary and `cp` does not, the install commands dry-run under a scratch prefix, the `Type=exec` caveat for `systemctl status` |
| DEPL-03 | Example hardened systemd unit `deploy/rs3.service`: unprivileged user, host/port from an `EnvironmentFile`, restart on failure, V8-compatible sandboxing | §Recommended unit (verified `systemd-analyze verify`-clean, exposure 1.5, live-run under the user manager, kill-action seccomp run, MDWE crash reproduced, start-limit finding, `ProtectProc` evidence) |
| DOCS-01 | README: toolchain (1.99.1 / 2024), first-build V8 note, configuration, build/run/test | §README facts: config error lines replayed live, shell syntax of dotted names, prebuilt V8 URL/size/downloader, rustup auto-install |
| DOCS-02 | README: JS op syntax, the old→new mapping, an example, a link to `docs/js-ops.md` | §README facts (JS ops): mapping table, the worked `fac` example replayed, stringification table replayed |
| DOCS-03 | README: HTTP API with status codes and a "Behaviour changes" section versus 2021 | §README facts (HTTP API) with every body copied from live output; §Behaviour changes audit against `git show msc:…` (two D-14 bullets are **not** changes: see Open Question 3) |
| DOCS-04 | `Cargo.toml` `license = "MIT"`; README states MIT | §CI consistency: `Cargo.lock` sha256 identical, `cargo metadata` reports `MIT`, guards (a)–(h) pass on a scratch copy |
</phase_requirements>

## Project Constraints (from CLAUDE.md)

`./.claude/CLAUDE.md` is local, gitignored and older than the decision to drop Docker. **Ignore its builder/runtime-image rows** (PROJECT.md Key Decisions, 2026-10-21). The directives that still apply:

- Toolchain: Rust 1.99.1 (`rust-toolchain.toml`), edition 2024, `rust-version = "1.99"`.
- Released dependencies only: no git, path or pre-release dependencies. This phase adds **no** dependencies.
- One binary crate with modular store / JS runtime / HTTP layers and injected state. This phase changes no code apart from one comment (IN-05).
- Use `deno_core::v8`, never a direct `v8` dependency (guard (e)).
- Work on branch `modernize-toolchain` and commit once per plan with clear messages. The git identity comes from the `GIT_AUTHOR_*`/`GIT_COMMITTER_*` env vars, never from git config, and nothing is pushed.
- GSD workflow enforcement: edits go through `/gsd-execute-phase`.
- `actions-rs/*` is forbidden; `dtolnay/rust-toolchain@1.99.1`, `actions/checkout@v7` and `Swatinem/rust-cache@v2` are pinned by `scripts/check-workflow.py` CANONICAL (ci.yml is not edited in this phase).
- Project hook: never name the dotenv file in a shell command, and never give an example file a `.env` suffix.

## Summary

This phase is mostly documentation and one configuration artefact. No library is added. The risky parts are the systemd unit and keeping the README exactly true. Every candidate directive was run on this host: `systemd-analyze verify --recursive-errors=no` prints nothing and exits 0 for the recommended unit, and `systemd-analyze security --offline=true` scores it **1.5 OK** (1.7 for the exact D-07 set). The unit's own ExecStart, EnvironmentFile and the user-mode-compatible directives were then run as a transient user unit against `target/release/rs3`. Seccomp was active (`Seccomp: 2`, `NoNewPrivs: 1`), and every route, the 422/504 paths and a graceful SIGTERM stop worked. A second run used the seccomp **kill** action (no `SystemCallErrorNumber=`). It exercised startup, all routes, nested PATCH, the depth limit, a 5000 ms timeout and SIGTERM without a single SIGSYS. A negative control without `@network-io` died with `status=31/SYS`, which proves the filter really bites.

Research found three things the locked decisions did not anticipate:
1. **The start limit is never reached.** With `Restart=on-failure`, `RestartSec=2` and systemd's default start limit (5 starts per 10 s), a broken env file restarted **forever**: 13 restarts in 30 s, still `activating/auto-restart`. D-05 says it "stops at systemd's start limit", which is false as written. Adding `StartLimitIntervalSec=60` and `StartLimitBurst=5` in `[Unit]` makes it stop after 5 restarts ("Start request repeated too quickly", state `failed`). Verified.
2. **Journal lines are tagged `env[pid]`, not `rs3`,** because the identifier is taken from argv[0] of `/usr/bin/env`. `SyslogIdentifier=rs3` fixes this (verified). `journalctl -u rs3` works either way.
3. **Two D-14 "behaviour changes" are not changes.** The 2021 `/insert` used actix-web 4.0.0-beta.8's `String` extractor. That extractor already had the 262 144-byte limit (so 413 above it) and no Content-Type check. A Promise result also stringified the same way. Document 413 and any-Content-Type as unchanged facts, not as changes.

Under the user manager, `ProtectSystem=` and `ProtectProc=` **silently do nothing**: the unit got its own user namespace but kept the host mount namespace. `PrivateDevices=` and `CapabilityBoundingSet=` fail with `218/CAPABILITIES`, and `DynamicUser=` fails with `217/USER`. The namespace/capability directives therefore stay an operator UAT item on a real server, as D-11(d) planned.

**Primary recommendation:** Ship `deploy/rs3.service` exactly as in "Recommended deploy/rs3.service": the D-05..D-08 set plus `StartLimitIntervalSec=60`, `StartLimitBurst=5`, `SyslogIdentifier=rs3` and `ProtectProc=invisible`; omit `ProcSubset=pid`. Prove it with `systemd-analyze verify --recursive-errors=no` (empty output, exit 0), `systemd-analyze security --offline=true --threshold=15`, and the unit-derived user-manager live run. Write the README only from the live outputs recorded below.

## Architectural Responsibility Map

| Capability | Primary Tier | Secondary Tier | Rationale |
|------------|-------------|----------------|-----------|
| Host/port configuration on a server | OS service manager (systemd `EnvironmentFile` + `env(1)` bridge) | rs3 config loader (`src/config.rs`, unchanged) | systemd cannot carry dotted names; rs3 reads only `SERVER.HOST`/`SERVER.PORT`. The bridge lives in the unit, so no code changes. |
| Privilege drop and sandbox | systemd (DynamicUser, seccomp, namespaces) | — | rs3 has no privilege logic; the unit owns it |
| Restart and failure policy | systemd (`Restart=`, start limit) | rs3 (one `error:` line, exit 1) | rs3 fails fast and explicitly; systemd decides whether to retry |
| Logging | journald (stdout/stderr streams) | rs3 (`println!`/`eprintln!`) | there is no log framework (OPS-01 deferred) |
| Network exposure and auth | operator (loopback default, reverse proxy) | — | rs3 has no auth or TLS by design |
| Developer documentation | `Readme.md` (summary) | `docs/js-ops.md` (full op contract) | the README links to it, not duplicating it |
| Build artefact | cargo (`--release --locked`) | V8 prebuilt download (v8 crate build.rs) | native binary; glibc dynamic, V8 static |

## Standard Stack

No new libraries. The tools this phase depends on:

| Tool | Version (this host) | Purpose | Notes |
|------|---------------------|---------|-------|
| systemd / `systemd-analyze` | 255 (255.4-1ubuntu8.18) [VERIFIED: `systemctl --version`] | `verify`, `security --offline=true`, `syscall-filter`, user-manager live runs | `--recursive-errors=no` suppresses warnings from other units on the host (see Pitfall 6) |
| `env(1)` (`/usr/bin/env`, coreutils) | present [VERIFIED: live runs] | dotted-name bridge in ExecStart | merged-/usr distributions all have `/usr/bin/env` |
| `install(1)` (coreutils) | present [VERIFIED] | README install commands | replaces a running binary; `cp` fails with "Text file busy" (verified) |
| cargo / rustc | 1.99.1 via `rust-toolchain.toml` [VERIFIED: `rustup show active-toolchain`] | build, TOOL-03 gate | rustup 1.29.1 auto-installs the pin |
| curl | 8.5.0 | README replay, smoke | also the V8 prebuilt download fallback |
| python3 + PyYAML | 3.12.3 + 6.0.1 | guards (tomllib needs Python 3.11 or later), check-workflow `--structure` | PyYAML is mandatory locally for guard (g) |

**Installation:** none.

## Package Legitimacy Audit

Not applicable. This phase installs no external packages (no crates, npm or PyPI packages); `Cargo.lock` stays byte-identical. **Packages removed:** none. **Packages flagged:** none.

## Architecture Patterns

### System Architecture Diagram

```
 operator: sudo install ... ; systemctl enable --now rs3
            |
            v
 systemd (PID 1, root) --reads--> /etc/rs3/rs3.conf  (RS3_HOST=127.0.0.1, RS3_PORT=5000)
            |                       [file read in the manager, before the executor is spawned]
            |  allocates DynamicUser "rs3", builds the sandbox (mount ns, seccomp, no caps)
            v
 /usr/bin/env SERVER.HOST=${RS3_HOST} SERVER.PORT=${RS3_PORT} /usr/local/bin/rs3
            |  (${VAR} expands to exactly one argument; env execs rs3 in the same PID)
            v
 rs3: Settings::load()  -- dotenv lookup from cwd "/" upward (normally none) + process env (wins)
            |-- error --> "error: invalid configuration: ..." on stderr, exit 1 --> journald
            |                 --> Restart=on-failure (2 s) ... start limit 5/60 s --> failed
            v
 init_platform -> JsExecutor::start (rs3-js-N workers + -wd watchdogs) -> bind(host, port)
            |-- bind error --> "error: cannot bind host:port: ..." exit 1 (same restart path)
            v
 banner "Starting Http server at host address: H, with port: P!" (stdout -> journald, tag rs3)
            v
 HTTP clients (loopback / reverse proxy) --> actix workers --> store / JS pool (V8 JIT: needs W+X)
```

### Recommended file changes

```
Dockerfile                      # delete
docker_rs3/                     # delete (11 tracked files; no untracked/ignored leftovers locally)
.gitignore                      # delete line 13: /docker_rs3/.secrets/
MODERNIZATION_BRIEF.md          # lines 37, 68-69, 72 re-worded (exact text below)
deploy/rs3.service              # new (text below)
deploy/rs3.conf                 # new (text below)
Readme.md                       # rewrite (facts below)
Cargo.toml                      # + license = "MIT" (after rust-version)
docs/js-ops.md                  # IN-03, IN-02 (and optionally IN-07) wording
src/js/executor.rs              # IN-05 comment only (lines 147-148)
```

### Recommended `deploy/rs3.service`

This is the exact text tested here: `systemd-analyze verify --recursive-errors=no` printed nothing and exited 0; `systemd-analyze security --offline=true` gave `→ Overall exposure level for rs3.service: 1.5 OK :-)`; no line is longer than 100 characters; and the unit-derived live run printed `banner ok`, `GET / ok`, `PATCH ok` and `NoNewPrivs: 1 Seccomp: 2`. The four lines beyond D-05..D-08 are marked `(+)` in the table that follows.

```ini
# Example hardened systemd unit for rs3. Install: see "Deployment" in Readme.md.
[Unit]
Description=rs3 key/value store with embedded JavaScript
Documentation=https://github.com/jmfrank63/rs3
After=network-online.target
Wants=network-online.target
# Give up after 5 failed starts in 60 s (a broken /etc/rs3/rs3.conf), instead
# of restarting every 2 s forever.
StartLimitIntervalSec=60
StartLimitBurst=5

[Service]
Type=exec
# systemd drops variable names that contain a dot, so /etc/rs3/rs3.conf holds
# RS3_HOST/RS3_PORT and env(1) hands them to rs3 as SERVER.HOST/SERVER.PORT.
EnvironmentFile=/etc/rs3/rs3.conf
ExecStart=/usr/bin/env SERVER.HOST=${RS3_HOST} SERVER.PORT=${RS3_PORT} /usr/local/bin/rs3
# Tag journal lines "rs3" rather than "env".
SyslogIdentifier=rs3
Restart=on-failure
RestartSec=2

# Unprivileged: a transient "rs3" user (an existing static rs3 user is reused).
# No capabilities, so the port must be 1024 or above. For a lower port, set
# CapabilityBoundingSet= and AmbientCapabilities= to CAP_NET_BIND_SERVICE.
DynamicUser=yes
User=rs3
CapabilityBoundingSet=
AmbientCapabilities=
NoNewPrivileges=yes

# Sandboxing. Never add MemoryDenyWriteExecute=yes: V8's JIT needs writable
# and executable memory, and rs3 crashes at the first PATCH.
ProtectSystem=strict
ProtectHome=yes
PrivateTmp=yes
PrivateDevices=yes
ProtectKernelTunables=yes
ProtectKernelModules=yes
ProtectKernelLogs=yes
ProtectControlGroups=yes
ProtectClock=yes
ProtectHostname=yes
ProtectProc=invisible
RestrictNamespaces=yes
RestrictRealtime=yes
RestrictSUIDSGID=yes
LockPersonality=yes
RemoveIPC=yes
RestrictAddressFamilies=AF_UNIX AF_INET AF_INET6
SystemCallArchitectures=native
# @pkey: V8 uses memory protection keys on x86_64 CPUs that have them, and
# @system-service does not include pkey_alloc/pkey_free/pkey_mprotect.
SystemCallFilter=@system-service @pkey
SystemCallErrorNumber=EPERM
UMask=0077

[Install]
WantedBy=multi-user.target
```

| Directive | Status | Evidence |
|-----------|--------|----------|
| `StartLimitIntervalSec=60`, `StartLimitBurst=5` (+) | **Recommended; needed for D-05's stated behaviour** | Without them a `RS3_PORT=` file restarted 13 times in 30 s and stayed `ActiveState=activating SubState=auto-restart`, with `StartLimitIntervalUSec=10s StartLimitBurst=5` (each cycle takes about 2.25 s, so 5 starts never land in 10 s). With them: `NRestarts=5`, `rs3-research-bad2.service: Start request repeated too quickly.`, `ActiveState=failed`. [VERIFIED: user-manager runs] |
| `SyslogIdentifier=rs3` (+) | Recommended | Without it the journal shows `env[3948922]: error: invalid configuration: …`; with it, `rs3[3949034]: …`. The systemd docs say: "If not set, defaults to the process name of the executed process." [VERIFIED + CITED: freedesktop systemd.exec 255] |
| `ProtectProc=invisible` (+) | Recommended (discretion item, see the evidence section) | `verify` clean; exposure 1.7 → 1.5. Every /proc path rs3 opened was under `/proc/self` or `/proc/sys`. Not testable under the user manager (silently ignored there) → operator UAT |
| `ProcSubset=pid` | **Omit** | It would hide `/proc/sys/vm/overcommit_memory`, which glibc reads at runtime (observed). Only a 0.1 exposure gain. |
| `MemoryDenyWriteExecute=yes` | **Never** | Reproduced: `rs3-research-mdwe.service: Main process exited, code=dumped, status=5/TRAP` on the first PATCH; `GET /` had answered before that. [VERIFIED] |
| `SystemCallFilter=@system-service @pkey` | Keep | `@pkey` = `pkey_alloc pkey_free pkey_mprotect`; `systemd-analyze syscall-filter @system-service \| grep -c pkey` → `0`. Kill-action run with V8 executing: no SIGSYS. [VERIFIED] |
| `~@privileged @resources` (extra deny list) | Not added | It passed a kill-action run on aarch64 (everything answered), but that is beyond D-07 and unproven on x86_64 |
| `PrivateUsers=`, `PrivateNetwork=`, `IPAddressDeny=` | Not added | `PrivateNetwork` breaks serving; the rest are unprovable here |

### Pattern 1: The user-manager live run is derived from the committed unit file

**What:** A throwaway helper under `target/tmp` (not committed). It reads `deploy/rs3.service`, keeps only the directives that actually work in a user manager, and swaps `/usr/local/bin/rs3` for the absolute `target/release/rs3`. It writes a scratch env file with a free port, runs `systemd-run --user`, and asserts the banner, `GET /`, PATCH `"120"` and `Seccomp: 2`. Because the tested directives come from the real file, the run cannot drift from it.

**User-manager-compatible directives (evidence on this host):** `Type`, `Restart`, `RestartSec`, `StartLimitIntervalSec`, `StartLimitBurst`, `SyslogIdentifier`, `NoNewPrivileges`, `SystemCallArchitectures`, `SystemCallFilter`, `SystemCallErrorNumber`, `RestrictAddressFamilies`, `LockPersonality`, `RestrictRealtime`, `RestrictSUIDSGID`, `RestrictNamespaces`, `UMask`, plus `EnvironmentFile` and the ExecStart command. Not usable: `PrivateDevices=` and `CapabilityBoundingSet=` → `status=218/CAPABILITIES`; `DynamicUser=` → `status=217/USER`. `ProtectSystem=strict` / `ProtectProc=invisible` / `ProcSubset=pid` start but **do nothing**: `/proc/PID/ns/mnt` equals the host's `mnt:[4026531832]`, `/` stays `rw`, and `/proc` has no `hidepid`. Never count them as tested. [VERIFIED: systemd-run --user runs]

**Example (verified; it printed `banner ok`, `GET / ok`, `PATCH ok`, `NoNewPrivs: 1 Seccomp: 2`):**
```bash
#!/usr/bin/env bash
# target/tmp/... helper, not committed. Usage: unit-live.sh deploy/rs3.service
set -euo pipefail
UNIT_FILE=$1
ROOT=/home/t3code/rs3/Projects/rs3
SCRATCH=$ROOT/target/tmp/p6-live; mkdir -p "$SCRATCH"
NAME=rs3-unit-live-$$
PORT=$(python3 -c 'import socket; s=socket.socket(); s.bind(("127.0.0.1", 0)); print(s.getsockname()[1])')
printf 'RS3_HOST=127.0.0.1\nRS3_PORT=%s\n' "$PORT" > "$SCRATCH/$NAME.conf"
keys='Type|Restart|RestartSec|StartLimitIntervalSec|StartLimitBurst|SyslogIdentifier|NoNewPrivileges|SystemCallArchitectures|SystemCallFilter|SystemCallErrorNumber|RestrictAddressFamilies|LockPersonality|RestrictRealtime|RestrictSUIDSGID|RestrictNamespaces|UMask'
props=()
while IFS= read -r line; do props+=(-p "$line"); done < <(grep -E "^($keys)=" "$UNIT_FILE")
exec_line=$(grep -E '^ExecStart=' "$UNIT_FILE"); exec_line=${exec_line#ExecStart=}
read -ra cmd <<<"${exec_line//\/usr\/local\/bin\/rs3/$ROOT/target/release/rs3}"
systemd-run --user --unit="$NAME" --collect -q -p EnvironmentFile="$SCRATCH/$NAME.conf" "${props[@]}" "${cmd[@]}"
trap 'systemctl --user stop "$NAME" 2>/dev/null || true; rm -f "$SCRATCH/$NAME.conf"' EXIT
B=http://127.0.0.1:$PORT
for _ in $(seq 1 50); do curl -s -o /dev/null "$B/" && break; sleep 0.2; done
journalctl --user -u "$NAME" --no-pager -o cat | grep -q "Starting Http server at host address: 127.0.0.1, with port: $PORT!" && echo "banner ok"
[[ $(curl -s "$B/") == '"Welcome to rs3"' ]] && echo "GET / ok"
curl -s -o /dev/null -X POST "$B/insert" -d '{"m":"6*20"}'
[[ $(curl -s -X PATCH "$B/key/m") == '"120"' ]] && echo "PATCH ok"
P=$(systemctl --user show -p MainPID --value "$NAME"); grep -E '^(Seccomp|NoNewPrivs):' /proc/$P/status | paste -sd' '
```
For a stricter proof, run it once more with `SystemCallErrorNumber` removed from `keys`. The filter then kills on violation, so a SIGSYS shows in the journal as `code=dumped, status=31/SYS`. Exercise a timeout (`while(true){}` → 504 after 5000 ms) and a nested PATCH in that run too. This exact exercise passed here.

### Pattern 2: README facts come from replayed output, never from memory

Every body in the README tables is listed in "README facts" below, as copied from `curl -s -i` against `target/release/rs3` started with `env SERVER.HOST=127.0.0.1 SERVER.PORT=<free port>`. The executor replays the README's command blocks the same way (D-13).

### Anti-Patterns to Avoid
- **Writing `SERVER.HOST=… cargo run` in the README.** It is not an assignment in any POSIX shell: `bash: line 1: SERVER.HOST=127.0.0.1: command not found` (exit 127); zsh gives `zsh:1: command not found: SERVER.HOST=127.0.0.1`. Use `env SERVER.HOST=… SERVER.PORT=… cargo run`. [VERIFIED]
- **Putting `SERVER.HOST=` into `EnvironmentFile`/`Environment=`.** The file form logs `Ignoring invalid environment assignment 'SERVER.HOST=127.0.0.1': …` and rs3 then fails with `missing configuration field "server"`. `systemd-run -p Environment=SERVER.HOST=…` is refused outright with `Invalid environment block.` [VERIFIED]
- **Writing `$RS3_HOST` without braces.** systemd splits `$VAR` on whitespace and keeps `${VAR}` as one argument; keep the braces.
- **`cp` for updates.** `cp` over the running binary fails with `cp: cannot create regular file '…': Text file busy`; `install -m 0755` succeeds (it replaces the file). [VERIFIED]
- **Using the word "Docker" anywhere new** (README, deploy/ comments, brief). D-04's grep would then fail.
- **Claiming user-manager runs proved the namespace directives.** They silently no-op there (see Pattern 1).

## Don't Hand-Roll

| Problem | Don't Build | Use Instead | Why |
|---------|-------------|-------------|-----|
| Dotted env names under systemd | A wrapper shell script, or a code change to config loading | `ExecStart=/usr/bin/env SERVER.HOST=${RS3_HOST} …` | One line, no `/bin/sh` (so no `@system-service` exec concerns), no code change (OPS-02 deferred) |
| Unit validation | Custom INI linting | `systemd-analyze verify --recursive-errors=no` | It exits 1 on an unknown key, an unknown syscall group, a bad address family or a non-absolute ExecStart (all planted and verified) |
| Hardening score | Manual checklist | `systemd-analyze security --offline=true --threshold=15 <file>` | Exit 0 at 1.5 and exit 1 with `--threshold=14` (verified), so it is an assertable gate |
| Syscall discovery | strace (absent) or guessing | seccomp kill-action live run (no `SystemCallErrorNumber=`) | A SIGSYS shows as `status=31/SYS` in the journal (negative control verified) |
| Service user | `useradd` step / installer | `DynamicUser=yes` + `User=rs3` | rs3 owns no files; the docs say "If a statically allocated user or group of the configured name already exists, it is used" [CITED: freedesktop systemd.exec 255] |
| Restart throttling | Custom backoff | `StartLimitIntervalSec=` / `StartLimitBurst=` | Verified behaviour (see the table above) |

**Key insight:** every sandbox property of this unit can be asserted with stock systemd tooling on an unprivileged host, except the namespace and capability directives. Those need PID 1, so they belong to the operator UAT. Don't fake them with a user-manager run.

## Runtime State Inventory

This phase removes files rather than renaming them. The inventory is answered for completeness:

| Category | Items Found | Action Required |
|----------|-------------|------------------|
| Stored data | None. The store is in-memory and starts empty on every boot. | none |
| Live service config | None: `.github/workflows/ci.yml` and `scripts/*` never reference `Dockerfile`/`docker_rs3` (verified by grep). The remote tag `msc` exists on `origin` (`git ls-remote --tags origin` → `3ca26a1… refs/tags/msc`). | none |
| OS-registered state | None: `systemctl list-unit-files 'rs3*'` (system and `--user`) is empty on the dev host. All research transient units were stopped and collected. | none |
| Secrets/env vars | `.gitignore:13` `/docker_rs3/.secrets/` is dead after the deletion. No `docker_rs3/.secrets` directory exists locally (`git status --ignored` shows only `.claude/`, `.gsd/`, `target`). | delete the `.gitignore` line (code edit) |
| Build artifacts | None tied to Docker. `target/release/rs3` must be rebuilt after the `Cargo.toml` `license` edit (`CARGO_PKG_LICENSE` changes, so cargo rebuilds the rs3 crate only; `Cargo.lock` is unchanged). | rebuild before any live run (the D-17 gate already does) |

**Excluded-path side effect:** `Vagrantfile` (lines 70, 73, 99) syncs `docker_rs3/config` and runs `ansible.playbook = "docker_rs3/playbook_dev.yml"`. After the deletion that Vagrantfile is dangling on this branch. Constraints say leave it untouched; it still works at tag `msc`. The README can note that `Vagrantfile`, `aws/` and `experiments/` are MSc-era material for the `msc` version (without the word Docker).

## Common Pitfalls

### Pitfall 1: The start limit is never reached with `RestartSec=2`
**What goes wrong:** A broken `/etc/rs3/rs3.conf` makes rs3 exit 1 every ~2.25 s forever, filling the journal, and the unit never shows `failed`.
**Why it happens:** The default `StartLimitIntervalSec=10s`/`StartLimitBurst=5` needs 6 starts within 10 s. Each cycle takes 2 s of RestartSec plus a few ms, so 5 starts span about 9 s and the window keeps sliding.
**How to avoid:** Put `StartLimitIntervalSec=60` and `StartLimitBurst=5` in `[Unit]` (verified: stops after 5 restarts, about 11 s).
**Warning signs:** `systemctl status rs3` shows `activating (auto-restart)` with a growing restart counter.

### Pitfall 2: `Type=exec` reports success before rs3 has bound its port
**What goes wrong:** `systemctl enable --now rs3` returns 0 even when rs3 then exits 1 (bad config, port in use). It also returns 0 if `/usr/local/bin/rs3` is missing, because `Type=exec` only checks that `/usr/bin/env` was exec'd.
**How to avoid:** The README check step must be `systemctl status rs3` plus `curl -s http://127.0.0.1:5000/` (expects `"Welcome to rs3"`), and `journalctl -u rs3` on failure. Verified journal order: `Started …` comes first, then `error: invalid configuration: …`.

### Pitfall 3: Missing variable or missing file in the env file
**Observed (systemd 255):**
- A missing `RS3_PORT` line gives `Referenced but unset environment variable evaluates to an empty string: RS3_PORT`, then `error: invalid configuration: invalid type: string "", expected an integer for key \`server.port\` in the environment`.
- A missing `RS3_HOST` line gives the same "Referenced but unset … RS3_HOST" warning, then `error: cannot bind :18640: failed to lookup address information: Name or service not known`.
- A missing file (no `-` prefix) gives `Failed to load environment files: No such file or directory`, then `Failed to spawn 'start' task`, with `Result=resources`. It still obeys the start limit: `NRestarts=5` and then `Start request repeated too quickly.`.

[VERIFIED: user-manager runs] Keep `EnvironmentFile=` **without** the `-` prefix so that a missing file is loud.

### Pitfall 4: The dotenv lookup under systemd
**What goes wrong:** rs3 looks for a dotenv file in the working directory and every parent (traced: `statx` on `<cwd>/.env`, then every ancestor up to `/.env`). Under the system manager the working directory defaults to `/`, so rs3 probes `/.env`. Under the user manager the default is `$HOME`. The docs say: "defaults to the root directory when systemd is running as a system instance and the respective user's home directory if run as user." [CITED: freedesktop systemd.exec 255]
**Impact:** The env always wins over the file. A malformed file at `/` would stop startup, but that is an edge case and needs no directive. Worth one sentence in the README configuration section ("under the example unit only the environment is used").

### Pitfall 5: GitHub Actions annotations and the guards
**What goes wrong:** Nothing in this phase touches what CI annotates (compiler warnings, clippy, failing steps). The risks are a stray `#[allow]` (guard (a)) or a rustfmt change from the IN-05 comment edit. Comments are not wrapped by default rustfmt, but keep lines ≤ 100 columns anyway.
**How to avoid:** Run the full D-17 list after each plan.

### Pitfall 6: `systemd-analyze verify` noise from other units on the host
**What goes wrong:** Plain `systemd-analyze verify deploy/rs3.service` also prints warnings about unrelated units, because it loads the dependency graph (`multi-user.target`, …). On this host: `unified-monitoring-agent_*.service is marked executable`, `trusttunnel.service: Failed to open …: Permission denied`, `flashcards-api-a.service:22: Unknown key name 'WantedBim'`. Its exit code was still 0 here, but on another host it might not be.
**How to avoid:** Use `systemd-analyze verify --recursive-errors=no deploy/rs3.service` and assert **empty output and exit 0**. With that flag, a planted typo (`LockPersonalty=yes`) prints `…/rs3.service:35: Unknown key name 'LockPersonalty' in section 'Service', ignoring.` and exits 1. [VERIFIED]

### Pitfall 7: A script's stdout output lands in the journal unescaped
**What goes wrong:** `Deno.core.print(text)` writes raw to stdout, and journald turns each line into an entry tagged `rs3`, so a script can forge lines that look like rs3's own. A catch-and-retry stack-exhaustion loop (05-REVIEW IN-07) produced **799 stdout lines** (`<unknown>:-1: Uncaught RangeError: Maximum call stack size exceeded`, `ext:core/00_primordials.js:6413: …`) and 798 stderr lines from **one** PATCH. [VERIFIED: release binary, `deep` script]
**How to avoid:** Document it (the IN-03 rewrite). The README security note already says that scripts are untrusted.

### Pitfall 8: x86_64 is not proven under the seccomp filter
**What goes wrong:** All seccomp evidence is from aarch64 (no PKU). On x86_64, V8's protected platform calls `pkey_*` (covered by `@pkey`) and `arch_prctl` (in `@default`, which `@system-service` includes; verified by group listing). `map_shadow_stack` is in no group except `@known`. It would only be called if glibc enabled CET shadow stacks, which requires a SHSTK-marked binary [ASSUMED: rustc does not mark binaries by default]. With `SystemCallErrorNumber=EPERM` a stray call gets EPERM instead of a kill.
**How to avoid:** Make the operator UAT on an x86_64 server explicit. A CI sudo job is deferred.

## Code Examples

### `deploy/rs3.conf` (verified: parsed by systemd with its comment lines; the unit served `"Welcome to rs3"` and logged `rs3[3952106]: Starting Http server at host address: 127.0.0.1, with port: 18661!` with the port swapped)
```
# Environment file for deploy/rs3.service, installed as /etc/rs3/rs3.conf.
# systemd ignores names that contain a dot, so these are not SERVER.HOST /
# SERVER.PORT; the unit's ExecStart passes them on to rs3 under those names.
# rs3 has no authentication: keep 127.0.0.1 unless something you control
# (a reverse proxy with auth/TLS) sits in front of it.
RS3_HOST=127.0.0.1
RS3_PORT=5000
```
The manager reads this file itself: `exec_context_load_environment()` is called in `exec_spawn()` before `sd-executor` is spawned, so the file can be `root:root` 0644 (D-10) or 0600. [CITED: github.com/systemd/systemd v255 src/core/execute.c] The docs say: "The files listed with this directive will be read shortly before the process is executed." [CITED: freedesktop systemd.exec 255]

### Install dry-run (verified under a scratch prefix, no sudo)
```bash
R=target/tmp/p6-root
install -D -m 0755 target/release/rs3 $R/usr/local/bin/rs3
install -D -m 0644 deploy/rs3.conf   $R/etc/rs3/rs3.conf
install -D -m 0644 deploy/rs3.service $R/etc/systemd/system/rs3.service
find $R -type f -printf '%m %p\n'   # 755 …/usr/local/bin/rs3, 644 …/rs3.conf, 644 …/rs3.service
```

### `MODERNIZATION_BRIEF.md` re-wording (D-03; afterwards `grep -ci docker MODERNIZATION_BRIEF.md` → 0)
Current text [VERIFIED: MODERNIZATION_BRIEF.md:37-38, 68-69, 72-74]:
```
- Dockerfile uses `rust:1.53`; CI uses the archived `actions-rs/*` actions with
  `continue-on-error: true` everywhere.
...
9. Update the Dockerfile to a current `rust` image (cargo-chef still fine) and a slim
   runtime image.
...
Out of scope for this milestone (leave untouched): `experiments/`, `aws/`, `docker_rs3/`
Ansible content, `Vagrantfile` — except where they directly reference the Rust version
and changing that is trivial.
```
Proposed text:
```
- CI uses the archived `actions-rs/*` actions with `continue-on-error: true` everywhere.
...
9. Ship rs3 as a native binary on a single Linux server: document the release build
   (`cargo build --release --locked`) and provide an example hardened systemd unit.
   (Re-scoped on 2026-10-21; see `.planning/PROJECT.md` Key Decisions.)
...
Out of scope for this milestone (leave untouched): `experiments/`, `aws/`, `Vagrantfile`
— except where they directly reference the Rust version and changing that is trivial.
```

### IN-05 comment fix (`src/js/executor.rs:147-148`, comment only)
Current [VERIFIED: src/js/executor.rs:147-148]:
```rust
                    // Spawned from the worker, so the watchdog descends from the
                    // thread that initialised V8, like the worker itself.
```
Correct facts: `init_platform()` runs on the main thread (`src/main.rs`: `rs3::js::init_platform();` comes before `JsExecutor::start`). The chain is main → worker → watchdog. Proposed:
```rust
                    // Spawned from the worker, so the watchdog descends from the
                    // main thread that initialised V8 (main calls init_platform),
                    // through the worker.
```
Fix text as given by the review: "so the watchdog descends from the main thread that initialised V8, through the worker" [VERIFIED: 05-REVIEW.md:74-78].

### IN-03 fix (`docs/js-ops.md:199-206`)
Current [VERIFIED: docs/js-ops.md:199-206]: "… `Deno.core.print(text, true)` writes to the server's stderr instead. There rs3 escapes the text and writes one line per call, prefixed with `js stderr: ` (for example `js stderr: "a\nb"`), so a script cannot fake server log lines. Treat everything a script prints, on either stream, as untrusted. Error messages and stack traces of failed scripts are logged to stderr in the same escaped form."
Facts [VERIFIED: src/js/ops.rs:372-383 and a live run]: stdout is written byte-for-byte (`hello-stdout` showed up as its own line in the server's stdout), and stderr is `js stderr: "x\ny"`. Proposed wording: "`Deno.core.print(text)` writes `text` unchanged (raw, unescaped) to the server's stdout. A script can therefore print anything there, including text that looks like rs3's own output such as the startup banner; under systemd each line becomes a journal entry. `Deno.core.print(text, true)` writes to stderr instead. There rs3 escapes the text and writes one line per call, prefixed with `js stderr: ` (for example `js stderr: "a\nb"`), so a script cannot fake rs3's stderr lines."
**Recommended in the same edit (IN-07, deferred but documented as a limit):** after "Error messages and stack traces of failed scripts are logged to stderr in the same escaped form", add: "One exception: when a nested `op_rs3_patch` cannot start for lack of stack, V8 itself prints an unprefixed `Uncaught RangeError: Maximum call stack size exceeded` line to stdout, once per failed start." Otherwise that sentence stays false (Pitfall 7).

### IN-02 fixes (`docs/js-ops.md`; review text [VERIFIED: 05-REVIEW.md:52-60, 05-REVIEW.iter2.md:121-129])
1. Ops table rows 25-28: the "Throws when" cells for `op_rs3_list`/`op_rs3_get`/`op_rs3_insert`/`op_rs3_delete` mention only the nested host fault. Add: "after the depth limit stopped the PATCH: the same depth error; after the timeout: returns an empty string and touches nothing (see Errors)".
2. The exception to "every op returns an empty string" after a timeout (lines 13, 101-102, 222-223): an object or symbol argument still throws its `TypeError`, because argument conversion (`#[scoped] key: StrArg`) runs before `live_ctx` checks the timeout [VERIFIED: src/js/ops.rs:209-210 `fn op_rs3_get(state: &mut OpState, #[scoped] key: StrArg) -> Result<String, JsErrorBox> {` / `let Some(ctx) = live_ctx(state)? else {`; live_ctx at :177-188 checks `ctx.job.timed_out()` first].
3. Line 210: "At most 4 scripts run in one PATCH chain" → "The nesting depth is at most 4 scripts … Sibling `op_rs3_patch` calls at the same level are not counted; only the timeout bounds them."
4. Lines 224-225: "Other routes and other PATCH requests keep being answered meanwhile" → "Other routes keep being answered, and other PATCH requests too while a JS worker is free (there is one worker per available CPU); otherwise they wait in the queue."

## README facts (all replayed on 2026-10-21 against `target/release/rs3`, built 01:17 after the last `src` change at 01:09)

### Requirements / first build
- Toolchain: `rust-toolchain.toml` = `channel = "1.99.1"`, `components = ["rustfmt", "clippy"]`, `profile = "minimal"` [VERIFIED: rust-toolchain.toml:1-4]; `Cargo.toml` `edition = "2024"`, `rust-version = "1.99"` [VERIFIED: Cargo.toml:5-6]. rustup 1.29.1 auto-installs the pin on the first `cargo` call; `rustup toolchain install` with no argument installs "by default the active toolchain" (help text). [VERIFIED]
- V8: the `v8` 150.4.0 build script downloads `https://github.com/denoland/rusty_v8/releases/download/v150.4.0/librusty_v8_simdutf_release_<target>.a.gz`. It tries `deno`, then `python3 ./tools/download_file.py`, then `curl -L -f -s` [VERIFIED: v8-150.4.0/build.rs:703-716, 791-850]. Sizes: aarch64 37 577 326 bytes and x86_64 38 799 023 bytes (≈36–37 MiB) [VERIFIED: `curl -sIL` content-length]; unpacked `librusty_v8.a` is 184 763 082 bytes. The **same release prebuilt** is used for both debug and release builds, but it is downloaded separately into `target/debug/gn_out/obj` and `target/release/gn_out/obj` (keyed by a `.sum` file holding the URL). Offline or mirrored builds: `RUSTY_V8_ARCHIVE` (a path or URL to the `.a.gz`) or `RUSTY_V8_MIRROR` (base URL); a cache at `$CARGO_HOME/.rusty_v8/<url with non-alphanumerics replaced by _>` is also honoured. [VERIFIED: build.rs:703-716, 762-790]
- Disk: on this host `target/debug` is 3.9 GB (after many test builds) and `target/release` 985 MB [VERIFIED: du]. The "about 2.6 GB debug" figure is from CLAUDE.md and was not re-measured from a clean build [ASSUMED]; suggested README wording: "several GB (2.5–4 GB for a debug build with tests, about 1 GB for release)".
- Binary: `ldd target/release/rs3` → `libgcc_s.so.1`, `libm.so.6`, `libc.so.6` (plus vdso/ld) only; the highest required symbol is `GLIBC_2.34` (`objdump -T target/release/rs3 | grep -o 'GLIBC_[0-9.]*' | sort -uV | tail -1`), built on glibc 2.39, aarch64. [VERIFIED] The x86_64 floor was not measured.

### Quick start (verified commands)
```bash
git clone https://github.com/jmfrank63/rs3 && cd rs3
cargo build --release --locked
env SERVER.HOST=127.0.0.1 SERVER.PORT=5000 cargo run --release --locked
# Starting Http server at host address: 127.0.0.1, with port: 5000!
curl -s http://127.0.0.1:5000/          # "Welcome to rs3"
cargo test --locked
```
(The replay used a free port. `cargo run --release --locked` with env config served `"Welcome to rs3"`.) Alternative: copy `env.example` (`SERVER.HOST=localhost`, `SERVER.PORT=5000` [VERIFIED: env.example]) to a dotenv file named `.env` in the repo root and run `cargo run`. Prose only; agents never run that.

### Configuration (source: src/config.rs:1-117; error lines replayed with `env -i`)
- Only `server.host` / `server.port` after ASCII-lowercasing (`const SERVER_KEYS: [&str; 2] = ["server.host", "server.port"];` [VERIFIED: src/config.rs:16]), so `SERVER.HOST`, `server.host` and `Server.Port` all work (verified live: `server.host=127.0.0.1 Server.Port=18613` started). `SERVER_HOST`/`SERVER_PORT` (underscore) are **not** read.
- The env overrides the dotenv file. The dotenv file is searched in the cwd, then its parents (traced); a missing one is ignored; any other dotenv problem (unparsable line, BOM prefix, unreadable) is fatal with a message starting `error: invalid configuration: .env: ` (src/config.rs:79-82, test `bom_prefixed_dotenv_is_error` at :376-381).
- No defaults. Verified error lines (all exit 1):

| Input | stderr line |
|-------|-------------|
| nothing set | `error: invalid configuration: missing configuration field "server"` |
| host only | `error: invalid configuration: missing configuration field "server.port"` |
| port only | `error: invalid configuration: missing configuration field "server.host"` |
| `SERVER_HOST`/`SERVER_PORT` only | `error: invalid configuration: missing configuration field "server"` |
| `SERVER.PORT=abc` | ``error: invalid configuration: invalid type: string "abc", expected an integer for key `server.port` in the environment`` |
| `SERVER.PORT=` | ``error: invalid configuration: invalid type: string "", expected an integer for key `server.port` in the environment`` |
| `SERVER.PORT=70000` | ``error: invalid configuration: invalid type: 64-bit unsigned integer `70000`, expected an unsigned 16 bit integer for key `server.port` `` |
| `SERVER.HOST=` | `error: cannot bind :18612: failed to lookup address information: Name or service not known` |
| port in use | `error: cannot bind 127.0.0.1:18614: Address already in use (os error 98)` |
| port 80, unprivileged | `error: cannot bind 127.0.0.1:80: Permission denied (os error 13)` |

- The banner `Starting Http server at host address: {host}, with port: {port}!` is printed only after a successful bind [VERIFIED: src/main.rs; `println!` after `.bind(...)`]. `SERVER.HOST=localhost` works. SIGTERM shuts down gracefully (the user-manager stop gave `Result=success`).
- The JS limits are not configurable: `JsSettings` doc "Built in code and never read from the environment" [VERIFIED: src/js/mod.rs:209-213].

### HTTP API (live `curl -s -i`; every success has `content-type: application/json`)

| Request | Status | Body (exact) |
|---------|--------|--------------|
| `GET /` | 200 | `"Welcome to rs3"` |
| `GET /ip` | 200 | `"{'IP' : '127.0.0.1'}"` (the configured host) |
| `POST /insert` `-d '{"greeting":"hello","m":"6*20"}'` | 200 | `"{\"greeting\":\"hello\",\"m\":\"6*20\"}"` (echo in body order) |
| `GET /key/greeting` | 200 | `"hello"` |
| `GET /list` (empty store) | 200 | `"[]"` |
| `GET /list` | 200 | `"[{\"greeting\" : \"hello\"},{\"m\" : \"6*20\"}]"` (sorted by key) |
| `GET /list` with an escaped value (`{"q":"say \"hi\"\nnext"}`, sent as `text/plain`) | 200 | `"[{\"greeting\" : \"hello\"},{\"m\" : \"6*20\"},{\"q\" : \"say \\\"hi\\\"\\nnext\"}]"` |
| `PATCH /key/m` | 200 | `"120"` |
| `DELETE /key/greeting` | 200 | `"hello"` (old value) |

Errors: `application/json`, `{"error":"<code>","message":"<detail>"}` (src/http/error.rs:63-104):

| Status | `error` | Trigger | Body (exact, live) |
|--------|---------|---------|--------------------|
| 400 | `malformed_json` | `-d '{"a"'` | `{"error":"malformed_json","message":"malformed JSON: EOF while parsing an object at line 1 column 4"}` |
| 404 | `not_found` | GET/DELETE/PATCH `/key/greeting` on an empty store | `{"error":"not_found","message":"key not found: greeting"}` |
| 422 | `invalid_input` | `-d '[1]'` | `{"error":"invalid_input","message":"invalid input: insert body must be a JSON object"}` |
| 422 | `invalid_input` | `-d '{"a":1}'` | `{"error":"invalid_input","message":"invalid input: value for key a must be a string"}` |
| 422 | `js_error` | `throw new Error("boom")` | `{"error":"js_error","message":"Uncaught Error: boom"}` |
| 422 | `js_error` | `let a = ;` | `{"error":"js_error","message":"Uncaught SyntaxError: Unexpected token ';'"}` |
| 422 | `js_error` | `Deno.core.ops();` (legacy bootstrap) | `{"error":"js_error","message":"Uncaught TypeError: Deno.core.ops is not a function"}` |
| 422 | `js_depth_limit` | `me` = `Deno.core.ops.op_rs3_patch("me")` | `{"error":"js_depth_limit","message":"PATCH nesting depth limit (4) exceeded: me -> me -> me -> me -> me"}` |
| 504 | `js_timeout` | `while(true){}` (answered after 5010 ms) | `{"error":"js_timeout","message":"script exceeded the 5000 ms execution limit"}` |
| 500 | `internal` | JS host fault (not reproducible on demand) | `{"error":"internal","message":"internal error"}` (from code: `#[error("internal error")]`, src/http/error.rs:32-34) |
| 413 | — (actix) | body > 262 144 bytes (exactly 262 144 → 200; 262 145 → 413) | `payload reached size limit`, `content-type: text/plain; charset=utf-8`, `connection: close` |
| 404 | — (actix) | unknown route or method (`POST /`, `PUT /key/a`, `GET /nope`) | empty body, `content-length: 0` |

Note: `let ;` is **not** a syntax error in sloppy mode (it gives `Uncaught ReferenceError: let is not defined`), so use `let a = ;` in any example. The 256 KiB limit is `const DEFAULT_CONFIG_LIMIT: usize = 262_144; // 2^18 bytes (~256kB)` [VERIFIED: actix-web-4.17.0/src/types/payload.rs:334]. `not_implemented` no longer exists.

### JS ops (DOCS-02)
- Syntax: `Deno.core.ops.op_rs3_get("k")`; there is no bootstrap call. The five ops are `op_rs3_list`, `op_rs3_get`, `op_rs3_insert`, `op_rs3_delete`, `op_rs3_patch` [VERIFIED: src/js/ops.rs:418 `ops = [op_rs3_list, op_rs3_get, op_rs3_insert, op_rs3_delete, op_rs3_patch],`].
- Mapping (legacy from `git show msc:src/bindings/mod.rs` `register_op("rs3_list"|"rs3_get"|"rs3_insert"|"rs3_delete"|"rs3_patch", op_sync(…))` and `msc:requests/fac.js` `Deno.core.ops(); … Deno.core.opSync("rs3_get", "num")`):

| 2021 | Now |
|------|-----|
| `Deno.core.ops();` | delete it (it now throws `TypeError: Deno.core.ops is not a function`) |
| `Deno.core.opSync("rs3_list", "")` | `Deno.core.ops.op_rs3_list()` |
| `Deno.core.opSync("rs3_get", key)` | `Deno.core.ops.op_rs3_get(key)` |
| `Deno.core.opSync("rs3_insert", json)` | `Deno.core.ops.op_rs3_insert(json)` (a JSON object as a string) |
| `Deno.core.opSync("rs3_delete", key)` | `Deno.core.ops.op_rs3_delete(key)` |
| `Deno.core.opSync("rs3_patch", key)` | `Deno.core.ops.op_rs3_patch(key)` |

- Worked example (verified):
```bash
curl -s -X POST http://127.0.0.1:5000/insert -d '{"num":"5"}'          # "{\"num\":\"5\"}"
curl -s -X POST http://127.0.0.1:5000/insert -d @requests/fac.txt       # 200, echoes {"fac": ...}
curl -s -X PATCH http://127.0.0.1:5000/key/fac                          # "120"
```
(`curl -d @file` strips the newlines of `requests/fac.txt`; the body still parses. With `num` = `10` under the systemd unit the answer was `"3628800"`.)
- Stringification (live): `6*20` → `"120"`, `let a=1;` → `"undefined"`, `({a:1})` → `"[object Object]"`, `Promise.resolve(1)` → `"[object Promise]"`. `Deno.core.print("x\ny", true)` → stderr `js stderr: "x\ny"`; `Deno.core.print("hello-stdout\n")` → raw stdout.
- `docs/js-ops.md` already covers the migration table, errors, PATCH results, runtime model, arguments, sandbox and limits, the worked example and stored scripts after upgrading. The README summarises and links to it.

### Limits (src/js/mod.rs:240-253 `max_patch_depth: 4,` / `timeout_ms: 5000,` [VERIFIED])
Depth 4 counts the PATCHed key, so a script may nest `op_rs3_patch` three times. The timeout is counted from the moment a worker picks the job up, and a backstop answers at about +2 s for host stalls. Both are hard stops even when caught, neither rolls back writes, and there is no heap limit: unbounded allocation can abort the process. Under the unit, `Restart=on-failure` then restarts rs3 with an empty store; repeated aborts trip the start limit.

### Behaviour changes audit (2021 = `git show msc:src/{services,bindings,config}/mod.rs`, `msc:src/main.rs`, `msc:Cargo.lock`)

| D-14 bullet | True? | 2021 evidence |
|-------------|-------|---------------|
| `/list` sorted, escaped, `" : "` kept, `,]` quirk gone | **Yes** | legacy `format!("{{\"{}\" : \"{}\"}}", k, v)` unescaped, flurry iteration order, then `out.replace(",]", "]")` in the handler (corrupts any value containing `,]`) |
| Missing key → 404 (even on an empty store) instead of `"[]"` or a panic | **Yes** | `get`: `if MAP.is_empty() { "[]" } else { MAP.get(..).unwrap() }`; `delete`: `.remove(..).unwrap()`. A legacy PATCH of a missing key on an empty store ran the script `[]` [ASSUMED: answered `""`] |
| `/insert` 400/422, nothing inserted on error | **Yes** | legacy `serde_json::from_str(..).unwrap()` and a per-value `unwrap()` inside the insert loop (earlier keys were already written, then a panic) |
| `/insert` "accepts any Content-Type" | **Unchanged**, not a change | legacy `String` extractor: `PayloadConfig` with no mimetype, so `check_mimetype` passes everything [VERIFIED: actix-web-4.0.0-beta.8/src/types/payload.rs:141-165]. A new nuance: the body must be UTF-8 whatever the `charset` parameter (the legacy extractor decoded per charset) |
| 413 over 256 KiB | **Unchanged**, not a change | legacy `String` extractor `limit: DEFAULT_CONFIG_LIMIT` = `262_144` [VERIFIED: same file :267-271] |
| ops renamed, no bootstrap/`opSync`, catchable `Error`/`TypeError` | **Yes** | legacy ops `unwrap()` inside the op (process abort) |
| PATCH 422/504/500 | **Yes** | legacy `execute_script(..).unwrap()` → panic |
| depth limit and timeout are new | **Yes** | none in legacy |
| Promise → `"[object Promise]"` | **Probably unchanged** | legacy `to_rust_string_lossy` also performs ToString [ASSUMED]; document it as behaviour, not as a change |
| malformed/BOM dotenv stops startup; only `SERVER.*` read | **Yes** | legacy `dotenv().ok()` ignored errors; `Environment::new()` merged every env var |
| banner after bind; one error line, exit 1 | **Yes** | legacy `println!` banner **before** `.bind(..)?`; `Config::from_env().unwrap()` panic |
| `/insert` echo order | Unchanged | legacy `serde_json` 1.0.67 depended on `indexmap` (preserve_order) [VERIFIED: msc Cargo.lock] |

## State of the Art

| Old Approach | Current Approach | When Changed | Impact |
|--------------|------------------|--------------|--------|
| Docker + Ansible multi-container (`docker_rs3/`) | Native binary + hardened systemd unit | user decision 2026-10-21 | no image, no container runtime |
| `User=` + `useradd` | `DynamicUser=yes` (implies `ProtectSystem=strict`, `ProtectHome=read-only`, `PrivateTmp=`, `RemoveIPC=`, `NoNewPrivileges=`, `RestrictSUIDSGID=`) [CITED: freedesktop systemd.exec 255] | systemd 235+ | no account management |
| Hand-listed syscall allow-lists | `@system-service` group (+ `@pkey` for V8) | — | maintained by systemd |

**Deprecated/outdated:** `PermissionsStartOnly=`, `Type=simple` for services that can fail at exec (prefer `Type=exec`).

## Assumptions Log

| # | Claim | Section | Risk if Wrong |
|---|-------|---------|---------------|
| A1 | The root-only directives (`ProtectSystem=strict`, `PrivateDevices=yes`, `ProtectProc=invisible`, `CapabilityBoundingSet=`, `DynamicUser=yes`, `ProtectKernel*`, `ProtectControlGroups=yes`, …) do not break V8/rs3 under the real system manager | Recommended unit | rs3 fails to start or crashes on a server; caught by operator UAT (D-11d). Mitigation: the trace shows only `/proc/self/*`, `/proc/sys/vm/overcommit_memory`, `/sys/fs/cgroup/**/cpu.max` (read-only is fine), `/sys/devices/system/cpu/online` and `/dev/urandom` (kept by PrivateDevices: `"/dev/null\0" "/dev/zero\0" "/dev/full\0" "/dev/random\0" "/dev/urandom\0" "/dev/tty\0"` [CITED: systemd v255 src/core/namespace.c]) |
| A2 | x86_64 needs no syscalls outside `@system-service @pkey` (e.g. no `map_shadow_stack`) | Pitfall 8 | EPERM to a syscall; probably harmless, worst case a V8 abort. Operator UAT on x86_64 |
| A3 | The debug `target/` is "about 2.6 GB" from clean | README facts | cosmetic; use a range |
| A4 | A legacy PATCH of a missing key on an empty store answered `""`; a legacy Promise stringified as `"[object Promise]"` | Behaviour audit | wording of two minor "changes" bullets only |
| A5 | x86_64 glibc floor is similar (`GLIBC_2.34`-ish) | README facts | README advice ("same or older glibc") stays correct either way |

## Open Questions (RESOLVED)

1. **Add `StartLimitIntervalSec=60` / `StartLimitBurst=5` beyond D-08?**
   - What we know: without them D-05's "stops at systemd's start limit" is false (verified: endless restarts).
   - Recommendation (RESOLVED): add them. This implements D-05's stated intent. If the planner prefers the strict D-08 list, the README and unit comments must not claim that the unit stops at the start limit.
2. **`SyslogIdentifier=rs3` and `ProtectProc=invisible` (unit additions)?**
   - Recommendation (RESOLVED): include both. They are one line each and verified clean; `ProtectProc` is the discretion item, chosen on the access-trace evidence. Omit `ProcSubset=pid`.
3. **D-14 bullets "413 over 256 KiB" and "accepts any Content-Type" are not changes.**
   - Recommendation (RESOLVED): list both under the API docs as facts ("unchanged from 2021"), not under "Behaviour changes". Add "the body must be UTF-8 (a charset parameter is ignored)" as the real nuance. Keep "a Promise gives `[object Promise]`" as documented behaviour (not awaited; APIV-02).
4. **IN-07 sentence in `docs/js-ops.md`?**
   - Recommendation (RESOLVED): fix it in the same paragraph as IN-03 (one added sentence). Otherwise the rewritten paragraph keeps a false claim.
5. **Dangling `Vagrantfile` references.**
   - Recommendation (RESOLVED): leave the Vagrantfile untouched (constraint). One README sentence can say that `Vagrantfile`, `aws/` and `experiments/` belong to the MSc version at tag `msc`.

## Environment Availability

| Dependency | Required By | Available | Version | Fallback |
|------------|------------|-----------|---------|----------|
| systemd / systemd-analyze | DEPL-03 verify/security/live run | ✓ | 255 | — |
| user manager (lingering) | live run | ✓ | `Linger=yes`, `is-system-running` = running | — |
| sudo / root | real install of the unit | ✗ | — | operator UAT on a server (D-11d) |
| `/usr/bin/env`, `install` | unit, README | ✓ | coreutils | — |
| curl | replay, smoke | ✓ | 8.5.0 | — |
| python3 + PyYAML | guards (g), self-test | ✓ | 3.12.3 / 6.0.1 | — |
| gcc/gdb | (research trace only) | ✓ | — | not needed by plans |
| strace | — | ✗ | — | seccomp kill-action run |
| x86_64 host | PKU under seccomp | ✗ | — | operator UAT / deferred CI job |
| `/tmp` space | — | ~96% full (2.2 GB free) | — | keep scratch under `target/tmp` (`/home` has 88 GB free) |

**Missing dependencies with no fallback:** root on a real server, for the namespace/capability directives. This is an operator UAT item by design.

## Validation Architecture

### Test Framework
| Property | Value |
|----------|-------|
| Framework | cargo test (Rust 1.99.1 built-in harness, 216 tests per the latest CI run) + shell gates |
| Config file | none (Cargo.toml; `clippy.toml` for the test allowances) |
| Quick run command | `bash scripts/ci-guards.sh && python3 scripts/check-workflow.py && systemd-analyze verify --recursive-errors=no deploy/rs3.service` |
| Full suite command | the D-17 list: `bash scripts/ci-guards.sh`, `python3 scripts/check-workflow.py`, `bash scripts/test-ci-guards.sh`, `cargo fmt --check`, `cargo clippy --all-targets --locked -- -D warnings`, `cargo test --locked`, `cargo build --release --locked`, `bash scripts/ci-smoke.sh` |

### Phase Requirements → Test Map
| Req ID | Behavior | Test Type | Automated Command | File Exists? |
|--------|----------|-----------|-------------------|-------------|
| DEPL-01 | Docker files gone, no `.dockerignore`, no references | static | `test ! -e Dockerfile && test ! -e docker_rs3 && test ! -e .dockerignore && ! git grep -n -i docker -- . ':!.planning' ':!aws' ':!Vagrantfile' ':!experiments'` | n/a |
| DEPL-01 | `msc` tag present; excluded paths untouched | static | `git rev-parse -q --verify refs/tags/msc && git diff --quiet 4ff77dc -- aws Vagrantfile experiments` | n/a |
| DEPL-02 | README has the release build + install section | static | `grep -F 'cargo build --release --locked' Readme.md && grep -F 'install -m 0755 target/release/rs3 /usr/local/bin/rs3' Readme.md && grep -F 'systemctl enable --now rs3' Readme.md` | ❌ Readme rewrite |
| DEPL-02 | install commands work | dry-run | the `install -D` triple into `target/tmp/p6-root` (Code Examples) | n/a |
| DEPL-03 | unit valid on systemd 255 | static | `out=$(systemd-analyze verify --recursive-errors=no deploy/rs3.service 2>&1); rc=$?; [ $rc -eq 0 ] && [ -z "$out" ]` | ❌ deploy/rs3.service |
| DEPL-03 | exposure recorded and bounded | static | `systemd-analyze security --offline=true --threshold=15 deploy/rs3.service \| grep 'Overall exposure'` (1.5; use 17 without ProtectProc) | ❌ |
| DEPL-03 | required directives present, MDWE absent | static | `grep -qx 'DynamicUser=yes' deploy/rs3.service && grep -qx 'EnvironmentFile=/etc/rs3/rs3.conf' … && grep -qx 'SystemCallFilter=@system-service @pkey' … && ! grep -q '^MemoryDenyWriteExecute' deploy/rs3.service` | ❌ |
| DEPL-03 | bridge + seccomp work with V8 | live (user manager) | unit-derived helper (Pattern 1) → `banner ok`, `GET / ok`, `PATCH ok`, `Seccomp: 2`; plus a kill-action variant with a 504 | helper in target/tmp (Wave 0) |
| DEPL-03 | root-only directives, non-root user, x86_64 PKU | **manual** (operator UAT) | on a server: install per README; `curl -s http://127.0.0.1:5000/`; PATCH `6*20` → `"120"`; `ps -o user= -p $(systemctl show -p MainPID --value rs3)` ≠ `root`; `systemd-analyze security rs3` | n/a |
| DOCS-01 | toolchain, V8 note, config, build/run/test | static + replay | `grep -F '1.99.1' Readme.md`, `grep -F 'env SERVER.HOST=' Readme.md`; replay quick start against the release binary | ❌ |
| DOCS-02 | op syntax, mapping, example, link | static + replay | `grep -F 'Deno.core.ops.op_rs3_get' Readme.md && grep -F 'docs/js-ops.md' Readme.md && grep -F 'opSync' Readme.md`; replay the fac example → `"120"` | ❌ |
| DOCS-03 | API + status codes + behaviour changes | static + replay | `grep -F 'js_depth_limit' Readme.md && grep -F 'js_timeout' Readme.md && grep -F 'malformed_json' Readme.md && grep -i 'behaviour changes' Readme.md`; every body in the README equals the live bodies above | ❌ |
| DOCS-04 | MIT license | static | `grep -qx 'license = "MIT"' Cargo.toml && grep -F 'MIT' Readme.md && git diff --exit-code -- Cargo.lock` | ❌ |
| D-16 | carry-overs | static | `! grep -F 'so a script cannot fake server log lines' docs/js-ops.md`; `grep -F 'main thread that initialised V8' src/js/executor.rs`; `! grep -F 'At most 4 scripts run in one PATCH chain' docs/js-ops.md` | n/a |
| D-17 | CI unchanged and green | full suite | D-17 list; `git diff --quiet 4ff77dc -- .github scripts` | ✅ |

### Sampling Rate
- **Per task commit:** quick run command (guards, check-workflow, unit verify).
- **Per plan end:** the full D-17 list. Rebuild the release binary before any live run once `Cargo.toml` changes.
- **Phase gate:** the full suite plus the unit-derived live run plus the README replay, all green before `/gsd-verify-work`; the operator UAT items recorded in 06-UAT.md.

### Wave 0 Gaps
- [ ] `target/tmp/…/unit-live.sh`: the unit-derived user-manager live run (Pattern 1). Not committed.
- [ ] README replay: a throwaway script or a list of commands under `target/tmp` that starts `target/release/rs3` with `env SERVER.HOST=127.0.0.1 SERVER.PORT=<free>` and diffs every documented body. Not committed.
- No cargo test changes are needed (no behaviour change).

## Security Domain

### Applicable ASVS Categories (Level 1)

| ASVS Category | Applies | Standard Control |
|---------------|---------|-----------------|
| V1 Architecture | yes | least privilege via systemd (DynamicUser, no caps, seccomp, read-only FS) |
| V2 Authentication | no (by design; documented) | README: loopback default, reverse proxy with auth/TLS in front |
| V3 Session Management | no | — |
| V4 Access Control | yes (deployment) | bind `127.0.0.1` in the example; `RestrictAddressFamilies`; no inbound auth, so network placement is the control |
| V5 Input Validation | yes (existing) | 400/422/413 already implemented; unchanged |
| V6 Cryptography | no | TLS not built in; terminate it at the proxy |
| V7 Error Handling and Logging | yes | one `error:` line + exit 1; script stdout is raw (documented); `SyslogIdentifier=rs3` |
| V14 Configuration | yes | env file root-owned 0644 under `/etc/rs3`, read by PID 1; no secrets in it |

### Known Threat Patterns for this deployment

| Pattern | STRIDE | Standard Mitigation |
|---------|--------|---------------------|
| Remote JS execution by anyone who reaches the port | Elevation / Tampering | loopback default; reverse proxy with auth; README security note; the V8 sandbox has no fs/net ops |
| Escape from V8 into the host | Elevation | DynamicUser, no capabilities, `NoNewPrivileges`, seccomp `@system-service @pkey`, `ProtectSystem=strict`, `PrivateDevices`, `RestrictNamespaces` |
| Heap-exhaustion abort / crash loop | DoS | `Restart=on-failure` + start limit; no heap limit yet (JSHD-01, documented) |
| Log forging / flooding via `Deno.core.print` or stack-exhaustion retries | Repudiation / DoS | document it (IN-03/IN-07); journald rate limiting; OPS-01 deferred |
| A tampered env file redirects the bind | Tampering | `/etc/rs3/rs3.conf` root-owned; installed with `install -m 0644` |
| Binding a privileged port without need | Elevation | no `CAP_NET_BIND_SERVICE` by default (documented opt-in) |

## Sources

### Primary (HIGH confidence)
- This host, 2026-10-21: `systemd-analyze verify` / `security --offline=true` / `syscall-filter`; `systemd-run --user` transient units (bridge, kill-action seccomp, `~@privileged @resources`, MDWE crash, start-limit, missing file/variables, dotted names, namespace no-op proof); `curl -s -i` replay of every route and error against `target/release/rs3`; a gdb `catch syscall` trace of every openat/newfstatat/faccessat/readlinkat/statx call during startup, all routes, nested PATCH, the depth limit and the timeout; `ldd`/`objdump -T`; `install` vs `cp` over a running binary.
- Repo files read this session: `src/main.rs`, `src/config.rs:1-117,376-381`, `src/http/mod.rs:1-55`, `src/http/kv.rs:1-58`, `src/http/error.rs`, `src/store/mod.rs:1-97`, `src/store/legacy.rs:1-162`, `src/js/mod.rs:1-297`, `src/js/executor.rs:1-377`, `src/js/ops.rs:177-188,196-290,355-430 (extension at :416-430)`, `docs/js-ops.md`, `scripts/ci-guards.sh`, `scripts/check-workflow.py:1-80`, `scripts/test-ci-guards.sh:1-80,204-223`, `scripts/ci-smoke.sh:1-60`, `.github/workflows/ci.yml`, `.gitignore`, `MODERNIZATION_BRIEF.md`, `Readme.md`, `Cargo.toml`, `rust-toolchain.toml`, `env.example`, `requests/fac.{js,txt}`, 03/05 REVIEW + DISPOSITION files.
- Legacy: `git show msc:src/{main.rs,services/mod.rs,bindings/mod.rs,config/mod.rs}`, `msc:requests/fac.js`, `msc:Cargo.{toml,lock}`.
- Crate sources: `v8-150.4.0/build.rs`, `actix-web-4.17.0/src/types/payload.rs`, `actix-http-3.19.0/src/error.rs`, and `actix-web-4.0.0-beta.8/src/types/payload.rs` (crates.io tarball, grep only).
- systemd v255 source: `src/core/namespace.c` (private /dev nodes), `src/core/execute.c` (`exec_context_load_environment` in `exec_spawn`).

### Secondary (MEDIUM confidence)
- freedesktop.org systemd.exec (man/255): DynamicUser implications and static-user reuse, WorkingDirectory default, ProtectProc/ProcSubset caveats, EnvironmentFile timing and `-` prefix, SyslogIdentifier default, SystemCallFilter/ErrorNumber semantics, user-instance limitations.
- Seed: `git show 718065e:.planning/phases/06-docker-image-documentation/06-RESEARCH.md` (README outline and carry-over wording). Every kept fact was re-verified above.

### Tertiary (LOW confidence)
- x86_64 syscall behaviour under the filter (A2) and the legacy edge cases (A4). Both are marked [ASSUMED].

## Metadata

**Confidence breakdown:**
- Standard stack: HIGH. No new dependencies; tool versions probed.
- Architecture (unit): HIGH for everything a user manager can exercise; MEDIUM for the root-only directives (sound evidence, but the proof is operator UAT).
- README facts: HIGH. Every body, error line and command was replayed live.
- Pitfalls: HIGH. Each one was reproduced on this host.

**Research date:** 2026-10-21
**Valid until:** 2026-11-20 (systemd 255 and the pinned stack are stable; re-check if the server runs a different systemd major)
