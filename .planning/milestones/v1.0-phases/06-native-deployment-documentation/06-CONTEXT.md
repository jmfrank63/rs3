# Phase 6: Native Deployment & Documentation - Context

**Gathered:** 2026-10-21
**Status:** Ready for planning
**Mode:** Smart discuss (autonomous, headless). Every grey area was auto-accepted with its Recommended answer.

<domain>
## Phase Boundary

User decision (2026-10-21): rs3 compiles to a native binary and runs on one Linux server, so
Docker goes away. This phase:

- deletes the root `Dockerfile` and the `docker_rs3/` Docker/Ansible multi-container setup. The
  MSc-era version stays at git tag `msc`. No `.dockerignore` is added.
- removes the remaining Docker references from the docs. The requirements and roadmap were
  already re-worded in 2aa4f06: DOCK-01..03 became DEPL-01..03, and DOCS-04 was added.
- ships native deployment for a single Linux server:
  - release build instructions (`cargo build --release --locked`);
  - an example hardened systemd unit `deploy/rs3.service`, with an example environment file;
  - a short install section.
- rewrites `Readme.md`: toolchain, build/run/test, configuration, HTTP API, JS op syntax with
  the legacy-to-new mapping (linking `docs/js-ops.md`), behaviour changes against the 2021
  version, native deployment, and the MIT license (`license = "MIT"` in `Cargo.toml`).

Requirements: DEPL-01, DEPL-02, DEPL-03, DOCS-01, DOCS-02, DOCS-03, DOCS-04.

Not in this phase:
- packaging (.deb/.rpm), an installer script, TLS/auth proxy configs, a CI job that runs the
  unit;
- code changes to configuration loading. An underscore env alias stays v2 OPS-02.
- `aws/`, `Vagrantfile` and `experiments/` stay untouched. The stale
  `experiments/web_scrape_s3_api/Cargo.lock` was already removed in 5c28dde.

</domain>

<decisions>
## Implementation Decisions

### Docker removal and history
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

### systemd unit (`deploy/rs3.service`)
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

### Install and verification
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
- **D-11 Verification without root:** The dev host has no sudo, and Ubuntu 24.04 blocks
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

### README (`Readme.md`, keep the file name)
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
- **D-14 Behaviour changes since 2021:** The user's list plus the verified extras:
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
- **D-16 Review carry-overs:** Small doc/comment fixes:
  - 03-REVIEW IN-03: `docs/js-ops.md` must not claim that stdout output cannot fake log lines.
    stdout prints are raw, and stderr prints are escaped.
  - 05-REVIEW IN-02: fill the gaps in the `docs/js-ops.md` ops table and limits wording.
  - 05-REVIEW IN-05: the watchdog spawn comment in `src/js/executor.rs` names the wrong thread as
    the V8 initialiser.

### CI consistency
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

</decisions>

<canonical_refs>
## Canonical References

**Downstream agents MUST read these before planning or implementing.**

### Scope
- `.planning/ROADMAP.md` §Phase 6: re-scoped success criteria 1–5, plus notes (systemd dotted
  names, MemoryDenyWriteExecute, PKU `pkey_*`, LICENSE on `main`)
- `.planning/REQUIREMENTS.md`: DEPL-01..03, DOCS-01..04, Out of Scope (container images
  dropped; `experiments/`, `aws/`, `Vagrantfile`)
- `.planning/PROJECT.md` Key Decisions: "Drop Docker" row (2026-10-21)
- `MODERNIZATION_BRIEF.md`: original brief (D-03 edits it)

### Behaviour the README must describe accurately
- `src/main.rs` (startup errors, banner), `src/config.rs` (keys, precedence, dotenv errors)
- `src/http/mod.rs`, `src/http/kv.rs`, `src/http/error.rs` (routes, bodies, error codes, the
  413 limit)
- `src/js/mod.rs` (`JsSettings`: `max_patch_depth` 4, `timeout_ms` 5000),
  `src/js/executor.rs`, `src/js/ops.rs`
- `docs/js-ops.md` (full op contract; the README summarises and links it)
- `requests/*.js` / `requests/*.txt` / `requests/*.http` (examples), `env.example`
- Phase decisions: `02-CONTEXT.md` + SUMMARYs (errors, `/list`, config), `03-CONTEXT.md` +
  SUMMARYs (ops, PATCH), `05-CONTEXT.md` + SUMMARYs (limits)
- Review dispositions with carry-overs: `03-REVIEW-DISPOSITION.md` (IN-03),
  `05-REVIEW-DISPOSITION.md` (IN-02, IN-05)

### CI contract (must stay consistent)
- `.github/workflows/ci.yml`, `scripts/check-workflow.py` (CANONICAL + structure),
  `scripts/ci-guards.sh` (a)–(h), `scripts/test-ci-guards.sh`, `scripts/ci-smoke.sh`
- `.planning/phases/04-ci-gate-on-x86-64/04-CONTEXT.md` (D-01 amended to ubuntu-26.04)

</canonical_refs>

<code_context>
## Existing Code Insights

### Reusable Assets
- `scripts/ci-smoke.sh`: the release smoke pattern (free loopback port, readiness poll, exact
  body asserts, trap cleanup). It is the model for the user-manager live run of the unit.
- `docs/js-ops.md` has these sections: Migrating from the legacy API, Errors, PATCH results,
  Runtime model, Arguments and encoding, Sandbox and known limits, Worked example, Stored
  scripts after upgrading. The README links it rather than duplicating it.
- `env.example` (`SERVER.HOST=localhost`, `SERVER.PORT=5000`) and `requests/fac.js` (reads
  `num` through `op_rs3_get`) are the quick-start and example material.

### Established Patterns
- Exact-body assertions, loopback-only smokes, and no `SERVER.` in workflow text.
- Startup failures print one `error: …` line and exit 1 (`src/main.rs`): `invalid
  configuration`, `cannot start JS runtime`, `cannot bind`, `server failed`.
- The banner is `Starting Http server at host address: {host}, with port: {port}!`, printed
  after the bind.

### Integration Points
- Delete `Dockerfile` and `docker_rs3/`; edit `.gitignore` (one line).
- New `deploy/rs3.service` and `deploy/rs3.conf`.
- Rewrite `Readme.md`.
- `Cargo.toml` (+`license`), `docs/js-ops.md` (IN-03, IN-02), `src/js/executor.rs` (IN-05
  comment only), `MODERNIZATION_BRIEF.md` (D-03).

</code_context>

<specifics>
## Specific Ideas

- **Git identity (user instruction):** commit with the env vars
  `GIT_AUTHOR_NAME="Johannes Maria Frank"`, `GIT_AUTHOR_EMAIL="jmfrank63@gmail.com"`,
  `GIT_COMMITTER_NAME="Johannes Maria Frank"` and `GIT_COMMITTER_EMAIL="jmfrank63@gmail.com"`.
  **Never modify git config.** Commit messages end with
  `Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>`.
- **Never `git push`.** The operator pushes. Stay on `modernize-toolchain`.
- Keep `target/` inside the project dir (`/tmp` is about 96% full), with scratch under
  `target/tmp`. The release binary at `target/release/rs3` is current, built after the last
  source change.
- A project hook blocks naming the dotenv file in shell commands. README prose may mention it,
  but every executed smoke uses env-var config (`env SERVER.HOST=… SERVER.PORT=…`).
- The user manager works for the live run (`systemd-run --user …`; `journalctl --user -u <unit>`).
  Stop and collect transient units afterwards (`systemctl --user stop`, `--collect`).

</specifics>

<deferred>
## Deferred Ideas

- v2 OPS-02: an underscore env alias (`SERVER__HOST`) and default host/port. This would remove
  the systemd `env(1)` bridge.
- A CI job that installs and starts the unit with sudo on the x86_64 runner, for end-to-end
  proof of the root-only directives and PKU under the seccomp filter.
- Packaging (.deb/.rpm), an installer script, and a reverse-proxy/TLS example.
- Release profile tuning (v2 OPS-04). Structured logging (v2 OPS-01) would also address
  05-REVIEW IN-01/IN-07/IN-08.

</deferred>

---

*Phase: 06-native-deployment-documentation*
*Context gathered: 2026-10-21 (smart discuss, auto-accepted recommendations)*
