# Phase 6: Docker Image & Documentation - Research

**Researched:** 2026-10-21
**Domain:** Container packaging (cargo-chef, distroless), a CI workflow contract extension, and README/docs accuracy
**Confidence:** HIGH. Every Docker-side claim was checked against registry APIs or upstream source, and the build was simulated locally with cargo-chef 0.1.78. Every README fact was run against the release binary. The only unverified items are the remote CI run itself and a few timings and sizes; these are flagged as ASSUMED.

<user_constraints>
## User Constraints (from CONTEXT.md)

### Locked Decisions

#### Dockerfile (replaces the legacy root `Dockerfile`)
- **D-01:** Three cargo-chef stages (planner → cook → build) on `lukemathwalker/cargo-chef:latest-rust-1.99.1-trixie`. Do not use a `-slim` builder, because the `v8` build.rs needs curl to download the prebuilt static lib. `cargo chef cook --release --locked --recipe-path recipe.json`, then `cargo build --release --locked --bin rs3`. The V8 download is baked into the cooked dependency layer.
- **D-02:** Runtime stage `gcr.io/distroless/cc-debian13:nonroot`. Debian 13 matches the trixie builder's glibc; never pair a trixie builder with a bookworm runtime. Copy only the `rs3` binary (e.g. to `/usr/local/bin/rs3`). Set `USER nonroot:nonroot` explicitly, even though the tag already defaults to it. Use exec-form `ENTRYPOINT ["/usr/local/bin/rs3"]` and `EXPOSE 8080`.
- **D-03:** Image env defaults `ENV SERVER.HOST=0.0.0.0` and `ENV SERVER.PORT=8080`, overridable with `docker run -e SERVER.PORT=…`. Dotted names are valid Docker env keys, and rs3 reads them from the process env (Phase 2 D-16 allowlist). The binary itself still has no built-in defaults; the image supplies them. Document this.
- **D-04:** The build must not depend on files outside the context allowlist (D-05). If `rust-toolchain.toml` is excluded, the builder's preinstalled 1.99.1 is used and rustup does not try to fetch the `rustfmt`/`clippy` components during the image build. Research must confirm the behaviour either way and choose (Claude's discretion after research).

#### .dockerignore
- **D-05:** **Allowlist** style: `*` first, then re-include only what the build needs (`!Cargo.toml`, `!Cargo.lock`, `!src/`, plus anything research finds is required). This keeps `target/` (about 2.6 GB+), `.git`, `.planning`, `.claude`, `experiments/`, `aws/`, `docker_rs3/`, `requests/`, `scripts/`, docs and the `Vagrantfile` out of the context (DOCK-03).

#### Image verification without local Docker
- **D-06:** Add a second CI job, `docker`, to `.github/workflows/ci.yml` (`needs: ci`, `runs-on: ubuntu-26.04`, read-only permissions, `timeout-minutes` set). It runs `bash scripts/docker-smoke.sh`, which:
  - builds the image (`docker build -t rs3:ci .`);
  - runs it detached with `-p 127.0.0.1:<port>:8080` (image env defaults, plus an explicit `-e SERVER.HOST=0.0.0.0` per ROADMAP SC-2);
  - waits for readiness and asserts `curl /` returns exactly `"Welcome to rs3"`;
  - asserts the container user is non-root (`docker inspect --format '{{.Config.User}}'` is non-empty and not root/0, and/or the process UID from the host side);
  - optionally PATCHes a stored `6*20` and expects `"120"`, which proves V8 works in the slim runtime image;
  - removes the container in a trap.

  Docker is preinstalled on GitHub's ubuntu runners.
- **D-07:** The Phase 4 workflow contract must be kept in the same change: update `scripts/check-workflow.py` (the `CANONICAL` line list and the structure check, which now expects two jobs, `ci` and `docker`, with exact steps), guard (g) in `scripts/ci-guards.sh` where needed, and `scripts/test-ci-guards.sh` (planted cases still caught; the new job's lines pinned). The single-workflow-file rule stays. No `continue-on-error`, no `SERVER.` in the workflow text: the smoke script, not the workflow, sets `SERVER.*`. `${{ }}` stays limited to the existing concurrency expression. All guards and the self-test must pass locally.
- **D-08:** Local validation, since there is no Docker:
  - (a) a static Dockerfile check, a new guard or a small script called from `ci-guards.sh` (Claude's discretion), asserting:
    - FROM images exactly as in D-01/D-02, with no `-slim` builder;
    - `--locked` on cook and build;
    - `USER nonroot`;
    - exec-form `ENTRYPOINT`;
    - no `latest` runtime tag except the documented cargo-chef builder tag;
    - `.dockerignore` starts with `*`;
  - (b) `bash -n scripts/docker-smoke.sh`;
  - (c) a dry check that the files `.dockerignore` re-includes are exactly what `cargo build --release --locked` needs. For example, copy only the allowlisted files into `target/tmp/ctx` and build there offline.
- **D-09:** Record the remote CI run as a pending human UAT item (`06-UAT.md`): push `modernize-toolchain`, confirm both `ci` and `docker` jobs are green, and record the image size. Note that this one push also satisfies 04-UAT item 1 and 05-UAT item 1. Also offer the manual alternative: `docker build -t rs3 . && docker run --rm -p 8080:8080 -e SERVER.HOST=0.0.0.0 rs3`, then `curl localhost:8080/` on any machine with Docker.

#### README (`Readme.md`)
- **D-10:** Rewrite `Readme.md` (keep the file name) with these sections:
  - Overview, with the MSc origin kept as a short note.
  - Requirements: Rust 1.99.1 via `rust-toolchain.toml` (rustup auto-installs it), edition 2024; the **first build downloads the ~40 MB V8 prebuilt** (curl + network needed; slow first link, about 2.6 GB debug `target/`).
  - Quick start: build, run with `SERVER.HOST`/`SERVER.PORT` from env or a dotenv file (dotted names; `env.example`), test. Use `cargo test` and the full TOOL-03 gate commands.
  - Configuration: only `server.host`/`server.port` are read; no defaults in the binary; process env overrides the dotenv file; a malformed or BOM-prefixed dotenv file stops startup with exit 1.
  - HTTP API: every route with its success body and status codes 200/400/404/413/422/500/504 and the error codes `not_found`, `malformed_json`, `invalid_input`, `js_error`, `js_depth_limit`, `js_timeout`, `internal`, `not_implemented` if still reachable.
  - JavaScript ops: the `Deno.core.ops.op_rs3_*` syntax, the old→new mapping table (summary), a worked example (fac.js via `/insert` + PATCH), the stringification rules, and a link to `docs/js-ops.md` for the full contract.
  - Limits: depth 4, timeout 5000 ms, both hard stops; heap not covered.
  - Behaviour changes (D-11).
  - Development: CI workflow and guards (`scripts/ci-guards.sh`, `scripts/test-ci-guards.sh`, `scripts/ci-smoke.sh`).
  - Docker: build/run commands, non-root distroless, image env defaults.

  Every command and example in the README must be accurate against the code (DOCS-01..03). Run each quick-start/curl example locally where possible: env-var config only, since a project hook blocks naming the dotenv file.
- **D-11:** The "Behaviour changes" section (DOCS-03) must cover:
  - `GET`/`DELETE`/`PATCH` on a missing key → 404 JSON, also on an empty store, instead of `"[]"` or a panic;
  - `POST /insert` → 400 for malformed JSON, 422 for a non-object or non-string value, nothing inserted, and any content type accepted;
  - `/list` keys and values JSON-escaped (legacy `" : "` spacing preserved), entries sorted by key, and the `,]` quirk gone;
  - ops renamed `rs3_*` → `op_rs3_*` and called as `Deno.core.ops.op_rs3_*(...)`, with no `Deno.core.ops()` bootstrap and no `opSync`; a missing key throws a catchable `Error`;
  - PATCH errors → 422 `js_error` / 422 `js_depth_limit` / 504 `js_timeout` / 500 `internal`;
  - a Promise result stringifies to `"[object Promise]"`;
  - malformed or BOM-prefixed dotenv files now fail startup, and only `SERVER.*` keys are read;
  - payloads over 256 KiB → 413 (plain-text actix body);
  - the startup banner prints only after a successful bind.
- **D-12:** Doc carry-overs from earlier reviews:
  - 03-REVIEW IN-03: `docs/js-ops.md` must not claim stdout prints cannot fake log lines; stdout prints are raw and stderr prints are escaped.
  - 05-REVIEW IN-02: fill the gaps in the `docs/js-ops.md` ops table and limits wording.
  - 05-REVIEW IN-05: fix the code comment naming the wrong V8 init thread.

  All are small doc or comment fixes.

### Claude's Discretion
- Exact Dockerfile layout (stage names, WORKDIR, cache mounts if compatible with plain `docker build`), whether to include `rust-toolchain.toml` in the context (D-04), the smoke port, the README prose and ordering, and whether the static Dockerfile check lives in `ci-guards.sh` (as a new guard (i)) or a separate script.

### Deferred Ideas (OUT OF SCOPE)
- Release profile tuning (LTO, strip): v2 OPS-04.
- Image publishing, multi-arch builds, SBOM/signing: future.
- SHA-pinned base images and actions: v2 OPS-03.

Also out of scope (CONTEXT domain): musl/Alpine images, multi-arch publishing or registries. `docker_rs3/`, `aws/` and the `Vagrantfile` are left untouched.
</user_constraints>

<phase_requirements>
## Phase Requirements

| ID | Description | Research Support |
|----|-------------|------------------|
| DOCK-01 | Dockerfile builds with a current `rust` toolchain image (cargo-chef caching, `--locked`) whose Debian release matches the runtime image | §Dockerfile (verified tags, trixie ↔ debian13 glibc 2.41 on both sides); `cargo chef cook` forwards `--locked` (cargo-chef 0.1.78 source); the 3-stage flow was simulated locally (cook 74 s, final build recompiles only `rs3`); guard (i) |
| DOCK-02 | Runtime image is slim (distroless cc), non-root, and the container serves `GET /` when run with `SERVER.HOST=0.0.0.0` | distroless `cc-debian13:nonroot` config `User="65532"`, `/etc/passwd` has `nonroot:x:65532`, and it ships every library the binary needs (ldd: libgcc_s, libm, libc); `scripts/docker-smoke.sh` (checked against a fake docker shim); `docker` CI job; remote run = UAT |
| DOCK-03 | `.dockerignore` excludes `target/` and other non-build files | Allowlist `*` + `!Cargo.toml` `!Cargo.lock` `!src/`. A context simulation with moby/patternmatcher semantics keeps 16 of 10 206 files. A release build from exactly those files succeeds. Guard (i) pins the allowlist |
| DOCS-01 | README: toolchain 1.99.1 / edition 2024, first-build V8 note, build/run/test | §README Outline; quick start run on port 18556 with `env SERVER.HOST=… SERVER.PORT=… cargo run --release --locked`; bash cannot `export` dotted names (verified) |
| DOCS-02 | README: new JS op syntax, old→new mapping, example | §README Outline: mapping table, fac.js walk-through and an inline `sq` example, all run live (`"120"`, `"25"`, `"key not found: nope"`) |
| DOCS-03 | README: HTTP error status codes + "Behaviour changes" | Every status and body below was captured live: 400, 404, 413 (plain text), 422 ×3 codes, 504; 500 from `src/http/error.rs`; unknown route or method gives an empty 404 |
</phase_requirements>

## Project Constraints (from CLAUDE.md)

- Toolchain: Rust **1.99.1**, edition 2024. Released dependencies only, no git or beta. Single binary crate.
- Platform: dev host is Linux aarch64; CI runs on x86_64 `ubuntu-26.04`.
- Same HTTP routes and semantics; response bodies stay compatible where tests assert them.
- Branch `modernize-toolchain`; commit per plan with clear messages. Git identity comes from env vars (`GIT_AUTHOR_NAME="Johannes Maria Frank"` …); **never modify git config, never push.** Commit messages end with `Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>`.
- Docker: builder `lukemathwalker/cargo-chef:latest-rust-1.99.1-trixie`, **never a `-slim` builder**; runtime `gcr.io/distroless/cc-debian13:nonroot`; the builder's glibc must match the runtime's (trixie ↔ Debian 13).
- Use `deno_core::v8`. No direct `v8` dependency, no `actions-rs/*`, no `cargo add deno_error`.
- GSD workflow: edits happen only through GSD execute commands.
- Environment: `/tmp` is about 96% full, so keep scratch work under `target/tmp`. A project hook blocks commands that *name* the dotenv file, so use env vars for every executed smoke.
- Every plan ends with: the TOOL-03 gate (`cargo build`, `cargo test`, `cargo clippy --all-targets -- -D warnings`, `cargo fmt --check`), `bash scripts/ci-guards.sh` and `bash scripts/test-ci-guards.sh` all green.

## Summary

The Docker side is low risk now. All five external facts the plan depends on were confirmed:
1. The builder tag `lukemathwalker/cargo-chef:latest-rust-1.99.1-trixie` exists for amd64 and arm64 (Docker Hub, updated 2026-10-16). It is `rust:1.99.1-trixie` plus cargo-chef 0.1.78, built from the full buildpack-deps base, so curl is present. Its rustup uses `--profile minimal`, so rustfmt and clippy are **not** installed.
2. `gcr.io/distroless/cc-debian13:nonroot` exists for amd64 and arm64. Its config sets `User: "65532"`; its `/etc/passwd` has `nonroot:x:65532:65532`; it ships libc6 `2.41-12+deb13u4`, the same Debian 13 glibc as the builder.
3. `cargo chef cook` accepts `--release` and `--locked` and forwards both to `cargo build`. `cargo chef prepare` accepts **only** `--recipe-path` and `--bin`, with no `--locked`.
4. D-04 is resolved by a real probe. With `rust-toolchain.toml` visible, a minimal-profile 1.99.1 install with rustup 1.29.1 printed `info: downloading 2 components` and installed clippy and rustfmt on the first `cargo --version`. Without the file it did nothing. **Recommendation: exclude `rust-toolchain.toml` from the context** and pin the version through the builder tag, guarded against drift.
5. Copying exactly `Cargo.toml`, `Cargo.lock` and `src/` and running the three stages with a locally installed cargo-chef 0.1.78 succeeded: prepare, `cook --release --locked`, then `build --release --locked --bin rs3`, which recompiled only `rs3`. The V8 static library (38 MB gz, 185 MB unpacked) is downloaded into `target/release/gn_out/obj` during cook, so it is baked into the cached layer.

The workflow contract extension was prototyped in a scratch copy under `target/tmp/proto`: a second `docker` job, `CANONICAL` plus a per-job structure check, a new guard (i) for the Dockerfile and `.dockerignore`, and 31 new planted cases. With it, `ci-guards.sh` passed all of (a) to (i) and `test-ci-guards.sh` reported **130 cases, 0 missed** (up from 99). `scripts/docker-smoke.sh` was run end to end against a fake `docker` shim that starts the real release binary. Its happy path and its two non-root negative paths behaved as designed.

The README facts were captured live from the release binary on ports 18550 to 18569. One pitfall changes how the README must be written: **bash cannot `export SERVER.HOST=…` or use `SERVER.HOST=… cmd` prefix assignments** (`command not found` / `not a valid identifier`), so every quick-start command must use `env SERVER.HOST=… SERVER.PORT=… cargo run …`.

**Primary recommendation:** Copy the prototype artifacts in §Code Examples verbatim. Keep `rust-toolchain.toml` out of the build context and guard the tag ↔ channel equality in guard (i). Write README examples with `env …` and loopback-only `-p 127.0.0.1:…` publishing.

## Architectural Responsibility Map

| Capability | Primary Tier | Secondary Tier | Rationale |
|------------|-------------|----------------|-----------|
| Dependency caching and the V8 prebuilt download | Build (Docker builder stage, cargo-chef cook layer) | — | The cook layer depends only on `recipe.json`, which changes only when manifests change |
| Toolchain version in the image | Build (builder image tag) | Repo guard (i) | The context excludes `rust-toolchain.toml`; guard (i) keeps the tag equal to its channel |
| Runtime: glibc, non-root user, no shell | Container runtime (distroless cc-debian13:nonroot) | — | Supplies only what the binary links; the user is fixed by the image and `USER` |
| Bind address and port defaults | Image config (`ENV SERVER.*`) | Binary config loader (env wins over dotenv) | The binary has no defaults by design (Phase 2); the image supplies them |
| Build-context hygiene | `.dockerignore` (allowlist) | Guard (i) | Keeps `target/` (multi-GB), `.git`, the dotenv file and secrets out of `COPY . .` |
| Image verification | CI `docker` job → `scripts/docker-smoke.sh` | Guard (i) static check locally | No Docker on the dev host |
| Workflow integrity | `scripts/check-workflow.py` (CANONICAL + structure) via guard (g) | `scripts/test-ci-guards.sh` | Phase 4 contract, extended to two jobs |
| Developer documentation | `Readme.md` | `docs/js-ops.md` (full JS contract) | The README summarises and links; js-ops.md stays the detailed reference |

## Standard Stack

No new Rust crates are added in this phase. These are the external build and runtime artifacts:

### Core
| Artifact | Version / Tag | Purpose | Verification |
|----------|---------------|---------|--------------|
| `lukemathwalker/cargo-chef` | `latest-rust-1.99.1-trixie` (cargo-chef 0.1.78 on `rust:1.99.1-trixie`) | Builder for the planner and builder stages | [VERIFIED: Docker Hub API, tag updated 2026-10-16, amd64/arm64/386/arm]; also [VERIFIED: tag `0.1.78-rust-1.99.1-trixie` exists]; image recipe = `FROM rust` + `cargo install cargo-chef --locked --version $CHEF_TAG` [CITED: github.com/LukeMathWalker/cargo-chef docker/Dockerfile] |
| `rust:1.99.1-trixie` (base of the above) | rustup 1.29.1, `--profile minimal`, `FROM buildpack-deps:trixie` | Toolchain 1.99.1; no rustfmt or clippy; curl present (buildpack-deps) | [CITED: github.com/rust-lang/docker-rust stable/trixie/Dockerfile]; slim variant installs only `ca-certificates gcc libc6-dev wget` [CITED: same repo, slim/Dockerfile] |
| `gcr.io/distroless/cc-debian13` | `nonroot` | Runtime: glibc 2.41, libgcc_s, libstdc++, libm, CA certs; uid 65532 | [VERIFIED: gcr.io registry API: index has linux/amd64 + linux/arm64/v8; config `User='65532'`, `WorkingDir=/home/nonroot`, no Entrypoint/Cmd; layer `etc/passwd` line `nonroot:x:65532:65532:nonroot:/home/nonroot:/sbin/nologin`; `libc6 2.41-12+deb13u4`; os-release `Debian GNU/Linux 13 (trixie)`] |
| cargo-chef | 0.1.78 (2026-08-12, latest) | `prepare` / `cook` | [VERIFIED: crates.io API]; flags read from source `src/main.rs` (installed locally under `target/tmp`, since deleted) |
| Docker on the CI runner | Docker Client/Server 29.4.2, Buildx 0.38.0 on `ubuntu-26.04` | Runs the `docker` job | [CITED: github.com/actions/runner-images images/ubuntu/Ubuntu2604-Readme.md] |

### Alternatives Considered
| Instead of | Could Use | Tradeoff |
|------------|-----------|----------|
| `latest-rust-1.99.1-trixie` (locked by D-01) | `0.1.78-rust-1.99.1-trixie` | Also pins cargo-chef, so builds are more reproducible. D-01 locks `latest-…`, and digest/SHA pinning is deferred to OPS-03. Mention it in the README as the reproducible option; do not switch |
| Including `rust-toolchain.toml` in the context | Exclude it (recommended) | Including it makes rustup download rustfmt and clippy in the planner and builder stages on every cold build (verified probe). It buys nothing, because the tag already pins 1.99.1 |
| `USER nonroot:nonroot` (locked by D-02) | `USER 65532:65532` | Kubernetes `runAsNonRoot` cannot verify a non-numeric user and refuses the pod. Docker itself resolves the name from the image's `/etc/passwd`. Keep D-02; document the numeric UID in the README (Open Question 1) |
| BuildKit cache mounts (`RUN --mount=type=cache`) | Plain layers (recommended) | Cache mounts hold build output outside the layers, which defeats the "V8 baked into the cooked layer" property and needs BuildKit-specific syntax. cargo-chef's layer cache is enough |

**Installation:** none (no `cargo add`, no `npm`/`pip`).

## Package Legitimacy Audit

This phase installs no packages from npm, PyPI or crates.io into the project. The container base images and the in-image cargo-chef were checked against their authoritative registries:

| Package / Image | Registry | Age | Downloads | Source Repo | Verdict | Disposition |
|---------|----------|-----|-----------|-------------|---------|-------------|
| cargo-chef 0.1.78 (installed *inside* the builder image by the image's own Dockerfile) | crates.io | 0.1.78 published 2026-08-12; crate since 2020 | widely used (not re-measured) | github.com/LukeMathWalker/cargo-chef | OK (registry + source read) | Approved, already the CLAUDE.md stack |
| lukemathwalker/cargo-chef:latest-rust-1.99.1-trixie | Docker Hub | tag refreshed 2026-10-16 | — | github.com/LukeMathWalker/cargo-chef (docker/Dockerfile) | OK | Approved |
| gcr.io/distroless/cc-debian13:nonroot | gcr.io | current | — | github.com/GoogleContainerTools/distroless | OK | Approved |

**Packages removed due to [SLOP] verdict:** none
**Packages flagged as suspicious [SUS]:** none

## Architecture Patterns

### System Architecture Diagram

```
 git checkout (CI) / repo (dev)
        │
        ▼  .dockerignore allowlist: *  !Cargo.toml  !Cargo.lock  !src/
 build context (16 files: Cargo.toml, Cargo.lock, src/**)
        │
        ├──► [chef] FROM lukemathwalker/cargo-chef:latest-rust-1.99.1-trixie   WORKDIR /app
        │        │
        │        ├──► [planner] COPY . .  →  cargo chef prepare  →  recipe.json
        │        │                                                    │
        │        └──► [builder] COPY recipe.json ◄────────────────────┘
        │                 │  cargo chef cook --release --locked   (deps + V8 38 MB download → cached layer)
        │                 │  COPY . .
        │                 └► cargo build --release --locked --bin rs3  (only rs3 recompiles)
        │                          │  /app/target/release/rs3 (≈71.5 MB, links libgcc_s/libm/libc)
        ▼                          ▼
 [runtime] FROM gcr.io/distroless/cc-debian13:nonroot
        COPY rs3 → /usr/local/bin/rs3 ; ENV SERVER.HOST=0.0.0.0 SERVER.PORT=8080
        USER nonroot:nonroot ; EXPOSE 8080 ; ENTRYPOINT ["/usr/local/bin/rs3"]
        │
        ▼
 docker run -p 127.0.0.1:P:8080 -e SERVER.HOST=0.0.0.0  ──►  rs3 (uid 65532) binds 0.0.0.0:8080
        │                                                       │ reads env (no dotenv in /home/nonroot)
        ▼                                                       ▼
 scripts/docker-smoke.sh: curl GET / == "Welcome to rs3"; Config.User ≠ root; docker top uid ≠ 0;
                          POST /insert m=6*20; PATCH /key/m == "120"; GET /key/nope == 404 JSON
```

### Recommended file changes

```
Dockerfile                    # rewrite (legacy rust:1.53 + ARG DISTRO version replaced)
.dockerignore                 # new: allowlist
scripts/docker-smoke.sh       # new: build + run + exact-body asserts + trap cleanup
.github/workflows/ci.yml      # + docker job (needs: ci)
scripts/check-workflow.py     # CANONICAL + JOBS spec + job_errors()
scripts/ci-guards.sh          # + guard (i) (Dockerfile/.dockerignore/docker-smoke.sh parse)
scripts/test-ci-guards.sh     # fresh() copies Dockerfile + .dockerignore; +31 planted cases
Readme.md                     # rewrite (D-10, D-11)
docs/js-ops.md                # D-12 IN-03 + IN-02 wording
src/js/executor.rs            # D-12 IN-05: comment only (lines 147-148)
.planning/phases/06-…/06-UAT.md  # D-09 remote-run item
```

### Pattern 1: cargo-chef "chef / planner / builder / runtime"
**What:** The upstream-documented layout. A `chef` base stage is shared by the `planner` (prepare) and the `builder` (cook, then build in the same stage, as separate cached layers), followed by a slim `runtime`. Upstream's README calls this "three stages: the first computes the recipe file, the second caches our dependencies and builds the binary, the third is our runtime environment" [CITED: github.com/LukeMathWalker/cargo-chef README]. It satisfies D-01's planner → cook → build order. A separate cook stage would need `COPY --from` of both `target/` and `$CARGO_HOME`, as the legacy Dockerfile did, which is more fragile and gains no caching.
**When to use:** always here.

### Pattern 2: Static contract guard plus a remote real run
**What:** Guard (i) runs in `ci-guards.sh`, locally and as the first CI step, and checks the Dockerfile and `.dockerignore` structurally. The `docker` job does the real build. This is the same split as Phase 4 (canonical lines locally, remote run as UAT).

### Pattern 3: The smoke owns its config
**What:** `docker-smoke.sh` sets `-e SERVER.HOST=0.0.0.0`; the workflow text never contains `SERVER.` (D-05 (g) / D-07). This mirrors `ci-smoke.sh`, which uses `env -i … SERVER.HOST=…`.

### Anti-Patterns to Avoid
- **`COPY . .` without an allowlist `.dockerignore`:** this sends the 2.6 to 4 GB `target/`, `.git` and **the dotenv file** into the context, and the dotenv file can then end up in an image layer.
- **Shell-form `ENTRYPOINT rs3` or `CMD` in distroless:** there is no `/bin/sh`, so the container fails to start.
- **`cargo install --path .` (legacy Dockerfile):** it ignores `Cargo.lock` unless `--locked` is given, and it builds into `$CARGO_HOME/bin`. Use `cargo build --release --locked --bin rs3`.
- **`ARG DISTRO` / `FROM ${DISTRO}` (legacy):** the runtime becomes untestable and can mismatch glibc. Guard (i) rejects it.
- **`-p 8080:8080` in docs:** it publishes an unauthenticated JS-execution endpoint on all interfaces. Use `-p 127.0.0.1:8080:8080` by default.

## Don't Hand-Roll

| Problem | Don't Build | Use Instead | Why |
|---------|-------------|-------------|-----|
| Dependency-layer caching in Docker | Copy dummy `main.rs` manifests by hand | cargo-chef `prepare`/`cook` | It handles lib + bin skeletons, version masking of the local crate and the lockfile ([VERIFIED: skeleton/version_masking.rs], cook succeeded with `--locked`) |
| A runtime base with glibc and a non-root user | `debian:trixie-slim` + `useradd` | distroless `cc-debian13:nonroot` | The user already exists (65532), there is no shell or package manager, and the glibc matches |
| Dockerfile linting | Custom regex linting beyond the contract | BuildKit build checks (on by default in `docker build`; optional `# check=error=true`) for style; guard (i) only for the project contract | No hadolint on the host; BuildKit checks ship with Docker 29 on the runner |
| A free port in the smoke | Hard-coded 8080 on the host | `python3 socket.bind(("127.0.0.1", 0))` (as in `ci-smoke.sh`) | Avoids collisions on shared runners |

## Runtime State Inventory

Not a rename or migration phase, so this section is omitted. (The Dockerfile rewrite replaces a file; no stored data, OS registrations or secrets reference it. Nothing is published to a registry.)

## Common Pitfalls

### Pitfall 1: `rust-toolchain.toml` in the build context triggers component downloads
**What goes wrong:** Each cold builder stage downloads rustfmt and clippy (about 10 to 20 MB), and the build gains a dependency on static.rust-lang.org that it does not need.
**Why:** The image installs 1.99.1 with `--profile minimal`. rustup 1.29.1's proxy path (`local_toolchain` → `maybe_ensure_active_toolchain` → `ensure_installed`) installs the components the toolchain file lists when `components_exist` is false, because auto-install defaults to on unless `RUSTUP_AUTO_INSTALL=0` [VERIFIED: rustup 1.29.1 src/config.rs:435-442, 841-890; src/cli/proxy_mode.rs:35-44]. cargo-chef also copies `rust-toolchain(.toml)` into the cook skeleton when it is present [VERIFIED: cargo-chef 0.1.78 skeleton/read.rs:180-195, skeleton/mod.rs:100-105].
**Probe (positive falsification, run in this session with a minimal 1.99.1 in a scratch RUSTUP_HOME):**
```
== without toolchain file ==
cargo 1.99.1 (5f94df478 2026-08-27)          # components: cargo, rust-std, rustc
== with toolchain file ==
info: syncing channel updates for 1.99.1-aarch64-unknown-linux-gnu
info: latest update on 2026-10-15 for version 1.99.1 (58cf8f9c3 2026-10-13)
info: downloading 2 components
cargo 1.99.1 (5f94df478 2026-08-27)          # components now include clippy, rustfmt
```
**How to avoid:** keep it out of the allowlist (D-04 decision). Guard (i) requires the builder tag to be `latest-rust-<channel>-trixie`, with `<channel>` read from `rust-toolchain.toml`.
**Warning signs:** `info: downloading 2 components` in the docker build log.

### Pitfall 2: Dotted env names in shells
**What goes wrong:** `export SERVER.HOST=…` → `not a valid identifier`; `SERVER.HOST=127.0.0.1 cargo run` → `SERVER.HOST=127.0.0.1: command not found` [VERIFIED: run in this session].
**How to avoid:** use `env SERVER.HOST=127.0.0.1 SERVER.PORT=5000 cargo run --release --locked` in the README. `docker run -e SERVER.HOST=…` is fine: the Docker CLI only rejects an empty name [CITED: github.com/docker/cli opts/env.go `ValidateEnv`]. Dockerfile `ENV` keys only need to be non-empty [CITED: github.com/moby/buildkit frontend/dockerfile/instructions/parse.go `parseKvps`].

### Pitfall 3: Binding loopback inside the container
**What goes wrong:** With `SERVER.HOST=127.0.0.1` or `localhost`, the port publish reaches nothing, so curl from the host gets a connection reset.
**How to avoid:** the image default is `SERVER.HOST=0.0.0.0` (D-03); the README explains that the host-side safety comes from `-p 127.0.0.1:…`.

### Pitfall 4: `.dockerignore` and the self-test copy
**What goes wrong:** `test-ci-guards.sh`'s `fresh()` copies an explicit file list. If guard (i) lands but `fresh()` does not copy `Dockerfile` and `.dockerignore`, the **control** fails and the whole self-test aborts. If `fresh()` lists `.dockerignore` before the file exists, `cp` fails ("copying the tree … failed").
**How to avoid:** create `.dockerignore`, add guard (i) and extend `fresh()` in the **same plan and commit**.

### Pitfall 5: Planting a toolchain bump in the self-test
**What goes wrong:** Changing `channel` in the scratch `rust-toolchain.toml` makes every `cargo` call in guards (d)/(e) try to install a toolchain that does not exist. Guard (d)'s error branch (`d_errs+=…"$(cargo tree … 2>&1 >/dev/null)"`) then exits the script under `set -e` before `[i]` prints. The case is missed even though the guard script exits non-zero [VERIFIED: observed in the prototype run].
**How to avoid:** plant the drift on the Dockerfile side (`latest-rust-1.99.0-trixie`), as in the prototype. Side note (pre-existing, out of scope): guards (d)/(e) exit silently rather than printing `FAIL [d]` when `cargo tree` itself fails. They still fail closed (rc ≠ 0).

### Pitfall 6: `cargo chef prepare` has no `--locked`
**What goes wrong:** A guard rule of "every cargo RUN has `--locked`" flags the prepare line, and adding `--locked` to prepare makes clap reject the unknown flag.
**How to avoid:** exempt `cargo chef prepare` (the `Prepare` struct has only `recipe_path` and `bin`) [VERIFIED: cargo-chef 0.1.78 src/main.rs:51-62].

### Pitfall 7: Expected cook-stage warnings
`warning: Cargo.toml: 'edition' is set on library 'rs3' which is deprecated` (and the same for the binary) appears during `cargo chef cook`. It comes from cargo-chef's generated skeleton manifest and is harmless [VERIFIED: local cook log]. Do not "fix" it in `Cargo.toml`.

### Pitfall 8: Non-numeric `USER` and Kubernetes
`USER nonroot:nonroot` works in Docker (the name resolves through the image's `/etc/passwd`). Kubernetes `runAsNonRoot: true` refuses images whose user is not numeric [ASSUMED]. Deployment to Kubernetes is out of scope; record the numeric UID 65532 in the README.

### Pitfall 9: Unauthenticated remote JS execution
`PATCH /key/{key}` runs stored JavaScript. Anyone who can reach the port can insert and run scripts in bare V8: no fs or network access, depth 4, timeout 5 s, **no heap limit**, so an allocation loop can abort the process (documented in js-ops.md). The README Docker section must say so and use loopback publishing.

### Pitfall 10: Image size
The binary is about 71.5 MB, unstripped, release profile [VERIFIED: local build]. distroless cc adds about 11 MB compressed layers [VERIFIED: layer sizes from the registry manifest, summed]. Expect an image of about 90 to 100 MB [ASSUMED]. Stripping and LTO are deferred (OPS-04). Record the real size in 06-UAT (the smoke prints it).

## Code Examples

All artifacts below were built and checked in a scratch copy (`target/tmp/proto`). Guards: `ci-guards.sh` all of [a] to [i] ok. Self-test: `130 cases, 0 missed`. Smoke: run against a fake docker shim. Copy them verbatim.

### Dockerfile (verified: guard (i) ok; stages simulated with cargo-chef 0.1.78)
Optional first line, with the line-1 placement it requires: `# check=error=true`. See Open Question 3. If you use it, it must be the very first line, before any other comment.
```dockerfile
# rs3 container image (DOCK-01, DOCK-02). Checked by guard (i) in scripts/ci-guards.sh and
# built and smoke-tested by the `docker` CI job (scripts/docker-smoke.sh).
#
# Builder: cargo-chef on the full rust:1.99.1-trixie image. Not a -slim image: the v8 crate's
# build script downloads the prebuilt V8 static library (about 40 MB) with python3 or curl,
# and -slim has neither. The Rust version in this tag must equal the channel in
# rust-toolchain.toml (guard (i)); the toolchain file itself stays out of the build context,
# so rustup does not download rustfmt and clippy here.
FROM lukemathwalker/cargo-chef:latest-rust-1.99.1-trixie AS chef
WORKDIR /app

# Stage 1: compute the dependency recipe from Cargo.toml and Cargo.lock.
FROM chef AS planner
COPY . .
RUN cargo chef prepare --recipe-path recipe.json

# Stage 2: build the dependencies (and download V8) in their own cached layer, then the binary.
FROM chef AS builder
COPY --from=planner /app/recipe.json recipe.json
RUN cargo chef cook --release --locked --recipe-path recipe.json
COPY . .
RUN cargo build --release --locked --bin rs3

# Stage 3: runtime. distroless/cc ships glibc, libgcc_s and libm, all the binary links
# (V8's libc++ is static). Debian 13 matches the trixie builder's glibc: never pair it
# with a bookworm (debian12) runtime.
FROM gcr.io/distroless/cc-debian13:nonroot AS runtime
COPY --from=builder /app/target/release/rs3 /usr/local/bin/rs3
# The binary has no built-in defaults; the image supplies them. Override with
# docker run -e SERVER.PORT=... (dotted names are valid container environment keys).
ENV SERVER.HOST=0.0.0.0 \
    SERVER.PORT=8080
USER nonroot:nonroot
EXPOSE 8080
ENTRYPOINT ["/usr/local/bin/rs3"]
```

### .dockerignore (verified: 16 of 10 206 files survive under moby/patternmatcher semantics; release build from exactly these files succeeded)
```
# Build-context allowlist (DOCK-03): ignore everything, then re-include only what
# `cargo build --release --locked` needs. Keep in sync with the Dockerfile COPY lines.
*
!Cargo.toml
!Cargo.lock
!src/
```
How `!src/` re-includes `src/**`: patternmatcher's `MatchesOrParentMatches` tests every parent directory of a file against each pattern, and later patterns override earlier ones. So `src/http/mod.rs` first matches `*` through its parent `src`, then matches `!src` through the same parent, which re-includes it. Trailing slashes are cleaned (`filepath.Clean`) [VERIFIED: github.com/moby/patternmatcher patternmatcher.go:128-163; CITED: docs.docker.com/build/concepts/context]. The Dockerfile and `.dockerignore` are still sent to the builder even when excluded, but they cannot be `COPY`'d [CITED: docs.docker.com/build/concepts/context].

### scripts/docker-smoke.sh (verified: `bash -n`; full run against a fake docker that starts the real release binary: OK; `FAKE_USER=root` → FAIL image user; `FAKE_UID=0` → FAIL UID)
```bash
#!/usr/bin/env bash
# Docker image smoke (D-06): the `docker` CI job runs it, and so can any machine with Docker.
# Builds the image from the repo root, runs it detached on a loopback port and asserts, each
# with the exact status code and the exact body bytes:
#   image config User        -> set and not root (not empty, root or 0)
#   GET /                    -> 200 "Welcome to rs3"
#   container process UID    -> not 0
#   POST /insert m           -> 200  (m = 6*20)
#   PATCH /key/m             -> 200 "120"  (V8 runs inside the distroless runtime image)
#   GET /key/nope            -> 404 {"error":"not_found","message":"key not found: nope"}
# The container keeps the image's env defaults (SERVER.PORT=8080) and gets
# SERVER.HOST=0.0.0.0 explicitly, as in the README. Config is set here, never in the workflow.
# Env: DOCKER_SMOKE_ROOT (default: this script's parent directory), RS3_IMAGE (default rs3:ci),
# RS3_SMOKE_PORT (default: a free 127.0.0.1 port).
set -euo pipefail
ROOT="${DOCKER_SMOKE_ROOT:-$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)}"
cd "$ROOT"
IMAGE="${RS3_IMAGE:-rs3:ci}"
PORT="${RS3_SMOKE_PORT:-$(python3 -c 'import socket; s=socket.socket(); s.bind(("127.0.0.1", 0)); print(s.getsockname()[1])')}"
BASE="http://127.0.0.1:${PORT}"
CID=""

# cleanup: remove the container (running or not); the image stays for inspection.
cleanup() {
  if [[ -n "$CID" ]]; then docker rm -f "$CID" >/dev/null 2>&1 || true; fi
}
trap cleanup EXIT

# fail MSG: report MSG with the container's state and log, exit 1.
fail() {
  echo "docker-smoke: FAIL: $*" >&2
  if [[ -n "$CID" ]]; then
    docker inspect --format 'container: status={{.State.Status}} exit={{.State.ExitCode}} oom={{.State.OOMKilled}}' "$CID" >&2 || true
    echo "--- container log ---" >&2
    docker logs "$CID" >&2 2>&1 || true
  fi
  exit 1
}

command -v docker >/dev/null 2>&1 || fail "docker not found"
if (exec 3<>"/dev/tcp/127.0.0.1/${PORT}") 2>/dev/null; then fail "port ${PORT} already in use"; fi

docker build -t "$IMAGE" . || fail "docker build failed"
echo "docker-smoke: ok   docker build -t ${IMAGE} ."

user=$(docker image inspect --format '{{.Config.User}}' "$IMAGE") || fail "docker image inspect failed"
case "$user" in
  "" | root | 0 | root:* | 0:*) fail "image user is '${user}'; the runtime image must set a non-root USER" ;;
esac
echo "docker-smoke: ok   image user ${user}"

CID=$(docker run -d -p "127.0.0.1:${PORT}:8080" -e SERVER.HOST=0.0.0.0 "$IMAGE") || fail "docker run failed"

# Readiness: poll until GET / answers; fail fast if the container exits.
ready=0
body=""
deadline=$((SECONDS + 60))
while (( SECONDS < deadline )); do
  running=$(docker inspect --format '{{.State.Running}}' "$CID" 2>/dev/null || echo false)
  [[ "$running" == "true" ]] || fail "container exited during startup"
  if body=$(curl -s --fail --max-time 2 "$BASE/"); then ready=1; break; fi
  sleep 1
done
(( ready )) || fail "server not ready on ${BASE} after 60s"
[[ "$body" == '"Welcome to rs3"' ]] || fail "GET / body: ${body}"
echo "docker-smoke: ok   GET / -> \"Welcome to rs3\""

uid=$(docker top "$CID" -o uid | awk 'NR == 2 { print $1 }') || fail "docker top failed"
[[ "$uid" =~ ^[0-9]+$ && "$uid" != 0 ]] || fail "container process UID is '${uid}' (must be a non-zero number)"
echo "docker-smoke: ok   container process UID ${uid}"

# req METHOD PATH [BODY]: one request; sets CODE (last output line) and BODY (the rest).
req() {
  local out
  local -a data=()
  if (( $# > 2 )); then data=(--data-binary "$3"); fi
  out=$(curl -sS --max-time 30 -w $'\n%{http_code}' -X "$1" ${data[@]+"${data[@]}"} "$BASE$2") \
    || fail "$1 $2: transport error"
  CODE="${out##*$'\n'}"
  BODY="${out%$'\n'*}"
}

req POST /insert '{"m":"6*20"}'
[[ "$CODE" == "200" && "$BODY" == '"{\"m\":\"6*20\"}"' ]] || fail "POST /insert -> ${CODE} ${BODY}"
echo "docker-smoke: ok   POST /insert -> 200"

req PATCH /key/m
[[ "$CODE" == "200" && "$BODY" == '"120"' ]] || fail "PATCH /key/m -> ${CODE} ${BODY}"
echo "docker-smoke: ok   PATCH /key/m -> 200 \"120\""

req GET /key/nope
[[ "$CODE" == "404" && "$BODY" == '{"error":"not_found","message":"key not found: nope"}' ]] \
  || fail "GET /key/nope -> ${CODE} ${BODY}"
echo "docker-smoke: ok   GET /key/nope -> 404 not_found"

[[ "$(docker inspect --format '{{.State.Running}}' "$CID")" == "true" ]] || fail "container died"
size=$(docker image inspect --format '{{.Size}}' "$IMAGE")
echo "docker-smoke: OK (${IMAGE}, port ${PORT}, user ${user}, uid ${uid}, image size ${size} bytes)"
```
`docker top "$CID" -o uid` works because the docker CLI stops parsing its own flags after the container argument (`flags.SetInterspersed(false)`, `Use: "top CONTAINER [ps OPTIONS]"`) [VERIFIED: github.com/docker/cli cli/command/container/top.go]. Make the script executable (`chmod +x`, like the existing scripts), although CI calls it with `bash`.

### .github/workflows/ci.yml: append (verified with `check-workflow.py` --lines and --structure)
```yaml
  # Builds the Docker image and smoke-tests it (scripts/docker-smoke.sh): GET /, non-root
  # user, PATCH through V8 in the distroless runtime. Runs only after the ci job passed.
  docker:
    name: docker image build and smoke (x86_64)
    needs: ci
    runs-on: ubuntu-26.04
    timeout-minutes: 30
    steps:
      - name: Checkout
        uses: actions/checkout@v7

      - name: Docker image smoke
        run: bash scripts/docker-smoke.sh
```
Job-level `permissions` stay forbidden by the structure check, so read-only comes from the existing top-level `permissions: contents: read` (D-06 "read-only permissions"). No `uses:` beyond checkout, no Rust toolchain or cache: the build happens inside Docker.

### scripts/check-workflow.py (verified diff; `python3 scripts/check-workflow.py` exit 0 on the new ci.yml)
The ci job keeps its bare `job:` / `step N:` error labels, so every existing self-test expectation string still matches. The docker job's labels are `job docker:` / `docker step N:`. Also update the module docstring (lines 1-26) to say the structure check now covers the two jobs `ci` and `docker`.
```diff
--- scripts/check-workflow.py	2026-10-20 23:00:16.221485172 +0000
+++ target/tmp/proto/scripts/check-workflow.py	2026-10-21 01:37:10.519938742 +0000
@@ -73,6 +73,16 @@
     "        run: cargo build --release --locked",
     "      - name: Release smoke",
     "        run: bash scripts/ci-smoke.sh",
+    "  docker:",
+    "    name: docker image build and smoke (x86_64)",
+    "    needs: ci",
+    "    runs-on: ubuntu-26.04",
+    "    timeout-minutes: 30",
+    "    steps:",
+    "      - name: Checkout",
+    "        uses: actions/checkout@v7",
+    "      - name: Docker image smoke",
+    "        run: bash scripts/docker-smoke.sh",
 ]
 # D-05 (g), on the functional lines (comments may say "server." in plain English).
 D05_BANNED = re.compile(r"continue-on-error|server\.", re.IGNORECASE)
@@ -99,6 +109,9 @@
 CONCURRENCY_KEYS = {"group", "cancel-in-progress"}
 CONCURRENCY_GROUP = "ci-${{ github.ref }}"
 JOB_KEYS = {"name", "runs-on", "timeout-minutes", "steps"}
+DOCKER_JOB_KEYS = JOB_KEYS | {"needs"}
+DOCKER_USES = ["actions/checkout@v7"]
+DOCKER_RUNS = ["bash scripts/docker-smoke.sh"]
 STEP_KEYS = {"name", "uses", "with", "run"}
 ALLOWED_WITH = {
     "actions/checkout@v7": set(),
@@ -107,6 +120,13 @@
 }
 
 
+# Exactly these jobs, in this order. The docker job runs only after ci passed (needs: ci).
+JOBS = {
+    "ci": {"keys": JOB_KEYS, "timeout": 45, "needs": None, "uses": EXPECTED_USES, "runs": REQUIRED_RUNS},
+    "docker": {"keys": DOCKER_JOB_KEYS, "timeout": 30, "needs": "ci", "uses": DOCKER_USES, "runs": DOCKER_RUNS},
+}
+
+
 def unexpected(obj, allowed):
     """Keys of obj outside allowed; FORBIDDEN_KEYS and defaults are reported separately."""
     return sorted(str(k) for k in set(obj) - allowed - set(FORBIDDEN_KEYS) - {"defaults"})
@@ -174,45 +194,58 @@
     if "defaults" in wf:
         errs.append("workflow: defaults is forbidden")
     jobs = wf.get("jobs") or {}
-    if not isinstance(jobs, dict) or list(jobs) != ["ci"]:
-        errs.append(f"exactly one job 'ci' expected, got {list(jobs) if isinstance(jobs, dict) else jobs!r}")
+    if not isinstance(jobs, dict) or list(jobs) != list(JOBS):
+        errs.append(f"exactly the jobs {list(JOBS)} expected, in that order, got "
+                    f"{list(jobs) if isinstance(jobs, dict) else jobs!r}")
         jobs = jobs if isinstance(jobs, dict) else {}
-    job = jobs.get("ci") or {}
+    for name, spec in JOBS.items():
+        errs += job_errors(name, jobs.get(name), spec)
+    return errs
+
+
+def job_errors(name, job, spec):
+    """The policy for one job: keys, runner, timeout, needs, exact uses and run lists."""
+    errs = []
+    # The ci job keeps the bare `job` / `step N` labels (the self-test matches them).
+    jscope = "job" if name == "ci" else f"job {name}"
+    sscope = "step" if name == "ci" else f"{name} step"
+    job = job or {}
     if not isinstance(job, dict):
-        errs.append(f"job 'ci' must be a mapping, got {job!r}")
-        job = {}
+        return [f"job '{name}' must be a mapping, got {job!r}"]
     if job.get("runs-on") != "ubuntu-26.04":
-        errs.append(f"runs-on must be ubuntu-26.04, got {job.get('runs-on')!r}")
-    if job.get("timeout-minutes") != 45:
-        errs.append("timeout-minutes: 45 required")
+        errs.append(f"{jscope}: runs-on must be ubuntu-26.04, got {job.get('runs-on')!r}")
+    if job.get("timeout-minutes") != spec["timeout"]:
+        errs.append(f"{jscope}: timeout-minutes: {spec['timeout']} required")
+    if job.get("needs") != spec["needs"]:
+        errs.append(f"{jscope}: needs must be {spec['needs']!r}, got {job.get('needs')!r}")
     if "defaults" in job:
-        errs.append("job: defaults is forbidden")
+        errs.append(f"{jscope}: defaults is forbidden")
     steps = job.get("steps") or []
     if not isinstance(steps, list) or not all(isinstance(s, dict) for s in steps):
-        errs.append("job steps must be a list of mappings")
+        errs.append(f"{jscope}: steps must be a list of mappings")
         steps = []
-    for scope, obj in [("job", job)] + [(f"step {i}", s) for i, s in enumerate(steps)]:
+    for scope, obj in [(jscope, job)] + [(f"{sscope} {i}", s) for i, s in enumerate(steps)]:
         for key in FORBIDDEN_KEYS:
             if key in obj:
                 errs.append(f"{scope}: {key} is forbidden")
-    if extra := unexpected(job, JOB_KEYS):
-        errs.append(f"job: unexpected keys {extra}")
+    if extra := unexpected(job, spec["keys"]):
+        errs.append(f"{jscope}: unexpected keys {extra}")
     for i, s in enumerate(steps):
         if extra := unexpected(s, STEP_KEYS):
-            errs.append(f"step {i}: unexpected keys {extra}")
+            errs.append(f"{sscope} {i}: unexpected keys {extra}")
         if ("uses" in s) == ("run" in s):
-            errs.append(f"step {i}: needs exactly one of uses or run")
+            errs.append(f"{sscope} {i}: needs exactly one of uses or run")
         with_ = s.get("with") or {}
         if not isinstance(with_, dict):
-            errs.append(f"step {i}: with must be a mapping, got {with_!r}")
+            errs.append(f"{sscope} {i}: with must be a mapping, got {with_!r}")
         elif "run" in s and with_:
-            errs.append(f"step {i}: with is only allowed on uses steps")
+            errs.append(f"{sscope} {i}: with is only allowed on uses steps")
         elif "uses" in s:
             if extra := sorted(set(map(str, with_)) - ALLOWED_WITH.get(str(s["uses"]), set())):
-                errs.append(f"step {i}: unexpected with: inputs {extra} for {s['uses']}")
+                errs.append(f"{sscope} {i}: unexpected with: inputs {extra} for {s['uses']}")
     uses = [s["uses"] for s in steps if "uses" in s]
-    if uses != EXPECTED_USES:
-        errs.append(f"uses must be exactly {EXPECTED_USES} in order, got {uses}")
+    if uses != spec["uses"]:
+        errs.append(f"{jscope}: uses must be exactly {spec['uses']} in order, got {uses}")
     for s in steps:
         if str(s.get("uses", "")).startswith("dtolnay/rust-toolchain"):
             with_ = s.get("with") if isinstance(s.get("with"), dict) else {}
@@ -220,14 +253,15 @@
             if not {"rustfmt", "clippy"} <= comps:
                 errs.append("dtolnay/rust-toolchain needs components: rustfmt, clippy")
     runs = [str(s["run"]).strip() for s in steps if "run" in s]
-    pos = [runs.index(r) if r in runs else -1 for r in REQUIRED_RUNS]
-    for r, p in zip(REQUIRED_RUNS, pos):
+    required = spec["runs"]
+    pos = [runs.index(r) if r in runs else -1 for r in required]
+    for r, p in zip(required, pos):
         if p < 0:
-            errs.append(f"missing run step: {r!r}")
+            errs.append(f"{jscope}: missing run step: {r!r}")
     if all(p >= 0 for p in pos) and pos != sorted(pos):
-        errs.append(f"run steps out of order: {runs}")
-    elif all(p >= 0 for p in pos) and runs != REQUIRED_RUNS:
-        errs.append(f"run steps must be exactly {REQUIRED_RUNS}, got {runs}")
+        errs.append(f"{jscope}: run steps out of order: {runs}")
+    elif all(p >= 0 for p in pos) and runs != required:
+        errs.append(f"{jscope}: run steps must be exactly {required}, got {runs}")
     if any("actions-rs/" in str(u) for u in uses):
         errs.append("actions-rs/* is forbidden")
     return errs
```

### scripts/ci-guards.sh: guard (i) (insert before the final `if (( failures ))`; update the header line "The tags [a]..[h] map one-to-one to D-05" to mention [i])
```bash
# (i) the Docker image contract (Phase 6 D-08), statically: Docker is not installed on the dev
# host, so the `docker` CI job (scripts/docker-smoke.sh) is the only real build. Checks:
# - external base images are exactly the cargo-chef builder for the rust-toolchain.toml channel
#   (lukemathwalker/cargo-chef:latest-rust-<channel>-trixie, never -slim) and the distroless
#   Debian 13 runtime (gcr.io/distroless/cc-debian13:nonroot), which is the final stage; any
#   other FROM names an earlier stage;
# - every RUN that invokes cargo (except `cargo chef prepare`, which has no such flag) passes
#   --locked, `cargo chef cook` and `cargo build` pass
#   --release, and nothing runs `cargo install`;
# - the final stage ends with USER nonroot:nonroot, sets the image env defaults
#   SERVER.HOST=0.0.0.0 / SERVER.PORT=8080, EXPOSE 8080 and an exec-form (JSON) ENTRYPOINT;
#   no shell-form CMD or ENTRYPOINT anywhere;
# - .dockerignore is an allowlist: first pattern `*`, and the re-included paths are exactly
#   Cargo.toml, Cargo.lock and src (rust-toolchain.toml stays out, see the Dockerfile);
# - scripts/docker-smoke.sh parses (bash -n).
# Comment lines and blank lines are free; `\` continuation lines are joined before checking.
i_errs=""
if ! i_msg=$(python3 - <<'PY' 2>&1
import json, posixpath, re, sys, tomllib
errs = []
try:
    with open("rust-toolchain.toml", "rb") as f:
        channel = tomllib.load(f)["toolchain"]["channel"]
except Exception as e:  # missing file, bad TOML, no channel: the drift check cannot pass
    channel = None
    errs.append(f"cannot read the toolchain channel from rust-toolchain.toml: {type(e).__name__}: {e}")
BUILDER = f"lukemathwalker/cargo-chef:latest-rust-{channel}-trixie"
RUNTIME = "gcr.io/distroless/cc-debian13:nonroot"
try:
    with open("Dockerfile", encoding="utf-8") as f:
        text = f.read()
except OSError as e:
    print(f"Dockerfile: {e}")
    sys.exit(1)
# Logical instructions: drop comment and blank lines, join `\` continuations.
instrs, buf = [], ""
for line in text.split("\n"):
    if not line.strip() or line.lstrip().startswith("#"):
        continue
    if line.rstrip().endswith("\\"):
        buf += line.rstrip()[:-1] + " "
        continue
    instrs.append((buf + line).strip())
    buf = ""
if buf:
    instrs.append(buf.strip())
stages, aliases, final = [], set(), []
for ins in instrs:
    word, _, rest = ins.partition(" ")
    word, rest = word.upper(), rest.strip()
    if word == "FROM":
        m = re.fullmatch(r"(\S+)(?:\s+[Aa][Ss]\s+(\S+))?", rest)
        if not m:
            errs.append(f"unparsable FROM (no flags, no ARG): {ins!r}")
            stages.append(rest)
            final = []
            continue
        ref, alias = m.group(1), m.group(2)
        if ref not in aliases:
            if "slim" in ref:
                errs.append(f"-slim base image {ref!r}: the v8 build script needs curl or python3")
            if ref not in (BUILDER, RUNTIME):
                errs.append(f"base image {ref!r} not allowed (builder {BUILDER!r}, runtime {RUNTIME!r})")
        stages.append(ref)
        if alias:
            aliases.add(alias)
        final = []
        continue
    final.append((word, rest))
    # `cargo chef prepare` only reads manifests and takes no --locked; every other cargo
    # command in a RUN must pass it.
    if word == "RUN" and re.search(r"\bcargo\b", re.sub(r"\bcargo\s+chef\s+prepare\b", "", rest)):
        if not re.search(r"(?:^|\s)--locked(?:\s|$)", rest):
            errs.append(f"cargo without --locked: {ins!r}")
        if re.search(r"\bcargo\s+install\b", rest):
            errs.append(f"cargo install is not allowed (use cargo build --release --locked): {ins!r}")
        if re.search(r"\bcargo\s+(?:chef\s+cook|build)\b", rest) and not re.search(r"(?:^|\s)--release(?:\s|$)", rest):
            errs.append(f"cargo build / cargo chef cook without --release: {ins!r}")
    if word in ("CMD", "ENTRYPOINT"):
        try:
            ok = isinstance(json.loads(rest), list)
        except ValueError:
            ok = False
        if not ok:
            errs.append(f"shell-form {word} (distroless has no shell; use the JSON exec form): {ins!r}")
if BUILDER not in stages:
    errs.append(f"builder stage FROM {BUILDER} missing (Rust version must equal rust-toolchain.toml)")
if not stages or stages[-1] != RUNTIME:
    errs.append(f"the final stage must be FROM {RUNTIME}, got {stages[-1] if stages else None!r}")
users = [rest for word, rest in final if word == "USER"]
if not users or users[-1] != "nonroot:nonroot":
    errs.append(f"the final stage must end with USER nonroot:nonroot, got {users[-1] if users else None!r}")
env_words = " ".join(rest for word, rest in final if word == "ENV").split()
for pair in ("SERVER.HOST=0.0.0.0", "SERVER.PORT=8080"):
    if pair not in env_words:
        errs.append(f"the final stage must set ENV {pair}")
if ("EXPOSE", "8080") not in final:
    errs.append("the final stage must EXPOSE 8080")
if not any(word == "ENTRYPOINT" for word, _ in final):
    errs.append("the final stage needs an exec-form ENTRYPOINT")
try:
    with open(".dockerignore", encoding="utf-8") as f:
        pats = [l.strip() for l in f.read().split("\n") if l.strip() and not l.startswith("#")]
except OSError as e:
    pats = None
    errs.append(f".dockerignore: {e}")
if pats is not None:
    if not pats or pats[0] != "*":
        errs.append(f".dockerignore must start with `*` (allowlist), got {pats[:1]}")
    allowed = {posixpath.normpath(p[1:].strip()).strip("/") for p in pats[1:] if p.startswith("!")}
    others = [p for p in pats[1:] if not p.startswith("!")]
    if allowed != {"Cargo.toml", "Cargo.lock", "src"} or others:
        errs.append(f".dockerignore must re-include exactly Cargo.toml, Cargo.lock and src/ after `*`, got {pats[1:]}")
print("\n".join(errs))
sys.exit(1 if errs else 0)
PY
); then i_errs+=$'\n'"$i_msg"; fi
if ! i_sh=$(bash -n scripts/docker-smoke.sh 2>&1); then
  i_errs+=$'\n'"scripts/docker-smoke.sh does not parse:"$'\n'"$i_sh"
fi
if [[ -z $i_errs ]]; then pass i "Dockerfile (chef builder = toolchain channel, distroless nonroot runtime, --locked, exec ENTRYPOINT), .dockerignore allowlist, docker-smoke.sh parses"
else fail i "${i_errs#$'\n'}"; fi
```
stdlib only (`tomllib`, `json`, `re`), so it runs in CI without PyYAML (Python 3.14 on the runner; 3.12.3 here).

### scripts/test-ci-guards.sh
`fresh()` must copy the two new files:
```diff
-    && cp -r Cargo.toml Cargo.lock rust-toolchain.toml clippy.toml src requests .github scripts \
+    && cp -r Cargo.toml Cargo.lock rust-toolchain.toml clippy.toml Dockerfile .dockerignore src requests .github scripts \
```
Insert these cases before `# --- (h) repo cargo config` (31 cases; all caught or passed in the prototype run):
```bash
# --- (g) the docker job (Phase 6 D-07) --------------------------------------------------------
expect_fail g 'docker job: needs: ci removed' \
  "sed -i '/^    needs: ci\$/d' $WF && ! grep -q '^    needs: ci' $WF"
expect_fail g 'docker job: continue-on-error added' \
  "sed -i 's/^    timeout-minutes: 30\$/    timeout-minutes: 30\n    continue-on-error: true/' $WF && grep -q '^    continue-on-error: true' $WF"
expect_fail g 'docker job: SERVER.HOST passed through a step env' \
  "sed -i 's|^        run: bash scripts/docker-smoke.sh\$|        run: bash scripts/docker-smoke.sh\n        env:\n          SERVER.HOST: 0.0.0.0|' $WF && grep -q 'SERVER.HOST: 0.0.0.0' $WF"
expect_fail g 'docker job: smoke run line suffixed with || true' \
  "sed -i 's|^        run: bash scripts/docker-smoke.sh\$|        run: bash scripts/docker-smoke.sh \|\| true|' $WF && grep -q 'docker-smoke.sh || true' $WF"
expect_fail g 'docker job deleted' \
  "sed -i '/^  docker:\$/,\$d' $WF && ! grep -q '^  docker:' $WF"
expect_fail g 'docker job: if: false' \
  "sed -i 's/^    needs: ci\$/    needs: ci\n    if: false/' $WF && grep -q '^    if: false' $WF"
expect_fail g 'docker job and CANONICAL both get if: false' \
  "sed -i 's/^    needs: ci\$/    needs: ci\n    if: false/' $WF && sed -i 's/^    \"    needs: ci\",\$/&\n    \"    if: false\",/' scripts/check-workflow.py && python3 scripts/check-workflow.py --lines" \
  'job docker: if is forbidden'
expect_fail_ci g 'docker job: timeout-minutes changed (CI, no PyYAML)' \
  "sed -i 's/^    timeout-minutes: 30\$/    timeout-minutes: 300/' $WF && grep -q 'timeout-minutes: 300' $WF" \
  "$LINES"
expect_wf_fail 'docker job: needs: ci removed from ci.yml and CANONICAL' \
  "sed -i '/^    needs: ci\$/d' $WF && sed -i '/^    \"    needs: ci\",\$/d' scripts/check-workflow.py && python3 scripts/check-workflow.py --lines" \
  "job docker: needs must be 'ci', got None"
expect_wf_fail 'docker job: a second run step added to ci.yml and CANONICAL' \
  "printf '      - name: Extra\n        run: echo extra\n' >> $WF && sed -i 's/^    \"        run: bash scripts\/docker-smoke.sh\",\$/&\n    \"      - name: Extra\",\n    \"        run: echo extra\",/' scripts/check-workflow.py && python3 scripts/check-workflow.py --lines" \
  "job docker: run steps must be exactly ['bash scripts/docker-smoke.sh']"

# --- (i) Dockerfile and .dockerignore (Phase 6 D-08) -----------------------------------------
DF=Dockerfile
expect_fail i 'Dockerfile deleted' "rm $DF" 'Dockerfile:'
expect_fail i '-slim builder image (rust:1.99.1-slim-trixie)' \
  "sed -i 's|^FROM lukemathwalker/cargo-chef:latest-rust-1.99.1-trixie AS chef\$|FROM rust:1.99.1-slim-trixie AS chef|' $DF && grep -q '^FROM rust:1.99.1-slim-trixie' $DF" \
  '-slim base image'
expect_fail i 'bookworm runtime (distroless cc-debian12)' \
  "sed -i 's|cc-debian13:nonroot|cc-debian12:nonroot|' $DF && grep -q 'cc-debian12' $DF" \
  'not allowed'
expect_fail i 'runtime debian:latest' \
  "sed -i 's|^FROM gcr.io/distroless/cc-debian13:nonroot AS runtime\$|FROM debian:latest AS runtime|' $DF && grep -q '^FROM debian:latest' $DF" \
  'the final stage must be FROM'
expect_fail i 'legacy ARG-driven runtime FROM ${DISTRO}' \
  "sed -i 's|^FROM gcr.io/distroless/cc-debian13:nonroot AS runtime\$|ARG DISTRO\nFROM \${DISTRO}|' $DF && grep -q 'DISTRO' $DF" \
  'not allowed'
# Drift between the builder tag and rust-toolchain.toml. (Planting the bump in
# rust-toolchain.toml instead would make rustup try to install a toolchain that is not there.)
expect_fail i 'builder tag rust-1.99.0 while rust-toolchain.toml says 1.99.1' \
  "sed -i 's|cargo-chef:latest-rust-1.99.1-trixie|cargo-chef:latest-rust-1.99.0-trixie|' $DF && grep -q 'latest-rust-1.99.0-trixie' $DF" \
  'builder stage FROM lukemathwalker/cargo-chef:latest-rust-1.99.1-trixie missing'
expect_fail i 'cargo chef cook without --locked' \
  "sed -i 's/^RUN cargo chef cook --release --locked --recipe-path recipe.json\$/RUN cargo chef cook --release --recipe-path recipe.json/' $DF && grep -q '^RUN cargo chef cook --release --recipe-path' $DF" \
  'cargo without --locked'
expect_fail i 'cargo build without --locked, split over continuation lines' \
  "sed -i 's/^RUN cargo build --release --locked --bin rs3\$/RUN cargo build \\\\\n    --release --bin rs3/' $DF && grep -q '^    --release --bin rs3' $DF" \
  'cargo without --locked'
expect_fail i 'cargo build without --release' \
  "sed -i 's/^RUN cargo build --release --locked --bin rs3\$/RUN cargo build --locked --bin rs3/' $DF && grep -q '^RUN cargo build --locked --bin' $DF" \
  'without --release'
expect_fail i 'cargo install --path . (legacy build)' \
  "sed -i 's/^RUN cargo build --release --locked --bin rs3\$/RUN cargo install --locked --path ./' $DF && grep -q 'cargo install' $DF" \
  'cargo install is not allowed'
expect_fail i 'USER nonroot:nonroot deleted' \
  "sed -i '/^USER nonroot:nonroot\$/d' $DF && ! grep -q '^USER' $DF" \
  'USER nonroot:nonroot'
expect_fail i 'USER root after USER nonroot' \
  "sed -i 's/^USER nonroot:nonroot\$/USER nonroot:nonroot\nUSER root/' $DF && grep -q '^USER root' $DF" \
  "got 'root'"
expect_fail i 'shell-form ENTRYPOINT' \
  "sed -i 's|^ENTRYPOINT \[\"/usr/local/bin/rs3\"\]\$|ENTRYPOINT /usr/local/bin/rs3|' $DF && grep -q '^ENTRYPOINT /usr' $DF" \
  'shell-form ENTRYPOINT'
expect_fail i 'ENV SERVER.HOST default removed' \
  "sed -i 's/^ENV SERVER.HOST=0.0.0.0 \\\\\$/ENV \\\\/' $DF && ! grep -q 'SERVER.HOST' $DF" \
  'ENV SERVER.HOST=0.0.0.0'
expect_fail i '.dockerignore deleted' "rm .dockerignore" '.dockerignore:'
expect_fail i '.dockerignore as a block-list (target/ only)' \
  "printf 'target/\n' > .dockerignore" 'must start with'
expect_fail i '.dockerignore re-includes rust-toolchain.toml' \
  "printf '!rust-toolchain.toml\n' >> .dockerignore" 'must re-include exactly'
expect_fail i '.dockerignore re-includes target' \
  "printf '!target\n' >> .dockerignore" 'must re-include exactly'
expect_fail i 'docker-smoke.sh syntax error' \
  "printf 'if then\n' >> scripts/docker-smoke.sh" 'docker-smoke.sh does not parse'
expect_pass 'Dockerfile comments mentioning slim, latest, root and cargo build' \
  "sed -i '1i # Never use a -slim builder, a :latest runtime, USER root or a cargo build without flags.' $DF && head -1 $DF | grep -q slim"
```

### README outline (every command and body below was run against the release binary in this session)

Proposed `Readme.md` sections, in order, with verified content:

1. **`# rs3`**: a key/value "information object store" over HTTP (actix-web), with an embedded V8 (deno_core 0.412) that can run a stored value as JavaScript calling back into the store. Short note: originally a 2021 Birkbeck MSc Computer Science project (jfrank11), modernized to Rust 1.99.1 / edition 2024.
2. **Requirements**
   - rustup. In the repo root, `rustup toolchain install` (with no argument it installs "the active toolchain", i.e. the one in `rust-toolchain.toml`) gives Rust **1.99.1** with rustfmt and clippy. Any `cargo` command in the repo also auto-installs the missing toolchain or components (rustup's default auto-install) [VERIFIED: probe + `rustup toolchain install --help`]. Edition 2024, `rust-version = "1.99"`.
   - **First build:** the `v8` crate's build script downloads a prebuilt V8 static library (about 38 to 39 MB gzip for x86_64/aarch64 glibc, 185 MB unpacked) from GitHub releases with python3 or curl [VERIFIED: v8 150.4.0 build.rs:786-850; GitHub API asset sizes]. Network access and curl or python3 are required. Offline: set `RUSTY_V8_ARCHIVE` (path or URL to the `.a.gz`) or `RUSTY_V8_MIRROR` [VERIFIED: build.rs:41-42, 704-709]. Expect a slow first build (about 1.5 min cold on the 4-core aarch64 dev host for a release build of the dependencies) and a large `target/`: about 2.6 GB after a debug build; this host's debug dir is 3.9 GB after `cargo test` [VERIFIED: du].
   - glibc Linux (V8 prebuilts target glibc; musl/Alpine is unsupported). For the dev scripts: python3 ≥ 3.11 (`tomllib`), PyYAML for `ci-guards.sh` locally (`python3-yaml`), and curl.
3. **Quick start** (bash; dotted names need `env`):
   ```bash
   cargo build --release --locked
   env SERVER.HOST=127.0.0.1 SERVER.PORT=5000 cargo run --release --locked
   # Starting Http server at host address: 127.0.0.1, with port: 5000!
   curl -s http://127.0.0.1:5000/            # "Welcome to rs3"
   cargo test
   ```
   Alternatively copy `env.example` (`SERVER.HOST=localhost`, `SERVER.PORT=5000`) to a dotenv file named `.env` in the repo root and run `cargo run`. (The README prose may name the file; agents must not execute commands naming it.)
4. **Configuration**: only `SERVER.HOST` and `SERVER.PORT` are read (case-insensitive; the exact names `server.host`/`server.port` after lowercasing). There are no defaults: a missing one is a startup error. `SERVER_HOST` (underscore) is not accepted. The process environment overrides the dotenv file. The dotenv file is looked up in the current directory, then its ancestors; a missing one is ignored. A malformed or BOM-prefixed one, or an unreadable one, stops startup. Verified messages, all exit 1:
   - `error: invalid configuration: missing configuration field "server.port"`
   - `error: invalid configuration: missing configuration field "server"`
   - `error: invalid configuration: invalid type: string "abc", expected an integer for key \`server.port\` in the environment`
   - dotenv errors start with `error: invalid configuration: .env: `
   - bind failure: `error: cannot bind 127.0.0.1:18551: Address already in use (os error 98)`
   The banner `Starting Http server at host address: <host>, with port: <port>!` prints only after a successful bind [VERIFIED: src/main.rs:29-38]. JS limits are set in code and cannot be set through the environment.
5. **HTTP API**: success bodies are JSON strings (the legacy envelope). Verified live:

   | Route | Success | Example body |
   |-------|---------|--------------|
   | `GET /` | 200 | `"Welcome to rs3"` |
   | `GET /ip` | 200 | `"{'IP' : '127.0.0.1'}"` (configured host) |
   | `POST /insert` body `{"greeting":"hello","m":"6*20"}` | 200 | `"{\"greeting\":\"hello\",\"m\":\"6*20\"}"` (echo in body order; any content type) |
   | `GET /key/greeting` | 200 | `"hello"` |
   | `GET /list` | 200 | `"[{\"greeting\" : \"hello\"},{\"m\" : \"6*20\"}]"` (sorted by key, byte order; `"[]"` when empty) |
   | `PATCH /key/m` | 200 | `"120"` (`String(completion value)`) |
   | `DELETE /key/greeting` | 200 | `"hello"` (old value) |

   Errors are `application/json` `{"error": "<code>", "message": "<detail>"}` [VERIFIED: src/http/error.rs]:

   | Status | `error` | When | Verified body |
   |--------|---------|------|---------------|
   | 400 | `malformed_json` | body not valid JSON / UTF-8 | `{"error":"malformed_json","message":"malformed JSON: EOF while parsing a value at line 1 column 5"}` |
   | 404 | `not_found` | GET/DELETE/PATCH missing key | `{"error":"not_found","message":"key not found: greeting"}` |
   | 422 | `invalid_input` | JSON not an object / non-string value | `{"error":"invalid_input","message":"invalid input: insert body must be a JSON object"}`, `…"invalid input: value for key a must be a string"}` |
   | 422 | `js_error` | script threw / syntax error / uncaught op error | `{"error":"js_error","message":"Uncaught Error: boom"}` |
   | 422 | `js_depth_limit` | PATCH nesting deeper than 4 | `{"error":"js_depth_limit","message":"PATCH nesting depth limit (4) exceeded: me -> me -> me -> me -> me"}` |
   | 504 | `js_timeout` | script ran past 5000 ms | `{"error":"js_timeout","message":"script exceeded the 5000 ms execution limit"}` |
   | 500 | `internal` | JS host fault | `{"error":"internal","message":"internal error"}` (from code; not reproducible on demand) |
   | 413 | — (actix, **text/plain**) | body > 256 KiB (262 144 bytes, actix default) | `payload reached size limit` |
   | 404 | — (empty body) | unknown route or wrong method (e.g. `POST /`, `PUT /key/a`) | empty |

   `not_implemented` is **not** reachable any more (removed in 03-03; not in `ApiError::code`), so omit it. The 256 KiB limit is actix-web 4.17's `DEFAULT_CONFIG_LIMIT: usize = 262_144` [VERIFIED: actix-web-4.17.0/src/types/payload.rs:334].
6. **JavaScript ops**: syntax `Deno.core.ops.op_rs3_get("k")`, no bootstrap call. Mapping (summary of docs/js-ops.md):

   | Old (2021) | New |
   |-----------|-----|
   | `Deno.core.ops();` | delete it (now throws `TypeError: Deno.core.ops is not a function`; verified → 422 `Uncaught TypeError: Deno.core.ops is not a function`) |
   | `Deno.core.opSync("rs3_list", "")` | `Deno.core.ops.op_rs3_list()` |
   | `Deno.core.opSync("rs3_get", key)` | `Deno.core.ops.op_rs3_get(key)` |
   | `Deno.core.opSync("rs3_insert", json)` | `Deno.core.ops.op_rs3_insert(json)` (a JSON object string) |
   | `Deno.core.opSync("rs3_delete", key)` | `Deno.core.ops.op_rs3_delete(key)` |
   | `Deno.core.opSync("rs3_patch", key)` | `Deno.core.ops.op_rs3_patch(key)` |

   Worked examples (verified):
   ```bash
   curl -s -X POST http://127.0.0.1:5000/insert -d '{"num":"5"}'
   curl -s -X POST http://127.0.0.1:5000/insert -d @requests/fac.txt
   curl -s -X PATCH http://127.0.0.1:5000/key/fac                     # "120"
   curl -s -X POST http://127.0.0.1:5000/insert \
     -d '{"sq":"let n = Number(Deno.core.ops.op_rs3_get(\"num\")); Deno.core.ops.op_rs3_insert(JSON.stringify({square: String(n * n)})); Deno.core.ops.op_rs3_get(\"square\")"}'
   curl -s -X PATCH http://127.0.0.1:5000/key/sq                      # "25"
   curl -s -X POST http://127.0.0.1:5000/insert \
     -d '{"safe":"try { Deno.core.ops.op_rs3_get(\"nope\") } catch (e) { e.message }"}'
   curl -s -X PATCH http://127.0.0.1:5000/key/safe                    # "key not found: nope"
   ```
   Stringification: last expression → `String(..)`: `6*20` → `"120"`, `let a=1;` → `"undefined"`, `({a:1})` → `"[object Object]"`, `Promise.resolve(1)` → `"[object Promise]"` (verified; not awaited). Sandbox: no fetch, require, setTimeout or fs. `Deno.core.print` goes to the server's stdout (raw) and stderr (escaped). Link `docs/js-ops.md` for the full contract.
7. **Limits**: depth 4 (counting the PATCHed key) and timeout 5000 ms (from worker pickup; queue wait not counted). Both are hard stops even if caught, and neither rolls writes back. There is **no heap limit**: a runaway allocation can abort the server. The limits are code-only (`JsSettings`).
8. **Behaviour changes** (D-11 list, using the verified bodies above). Wording for the `/list` item: escaped keys and values, legacy `" : "` spacing kept, sorted by key, the trailing `,]` quirk gone, `"[]"` on an empty store. Plus: `Deno.core.ops()`/`opSync` removed, 413 for > 256 KiB, the banner prints after the bind, BOM/malformed dotenv is fatal, only `SERVER.*` is read.
9. **Development**:
   - TOOL-03 gate: `cargo fmt --check`, `cargo clippy --all-targets --locked -- -D warnings`, `cargo test --locked`, `cargo build --release --locked`.
   - Scripts: `bash scripts/ci-smoke.sh` (needs the release build, curl, python3; runs about 6 s plus build), `bash scripts/ci-guards.sh` (guards a to i; PyYAML required locally), `bash scripts/test-ci-guards.sh` (planted-violation self-test, local only, scratch under `target/tmp`), `bash scripts/docker-smoke.sh` (needs Docker).
   - CI: `.github/workflows/ci.yml`, job `ci` (guards, fmt, clippy, test, release build, release smoke on ubuntu-26.04 x86_64) and job `docker` (after ci: image build + smoke). Editing ci.yml means updating `CANONICAL` in `scripts/check-workflow.py` in the same change.
10. **Docker**:
    ```bash
    docker build -t rs3 .
    docker run --rm -p 127.0.0.1:8080:8080 -e SERVER.HOST=0.0.0.0 rs3
    curl -s http://127.0.0.1:8080/        # "Welcome to rs3"
    docker run --rm -p 127.0.0.1:9000:9000 -e SERVER.PORT=9000 rs3   # other port
    ```
    Builder `lukemathwalker/cargo-chef:latest-rust-1.99.1-trixie` (cargo-chef layer caching, `--locked`); runtime `gcr.io/distroless/cc-debian13:nonroot` (no shell, uid/gid 65532). The image env defaults are `SERVER.HOST=0.0.0.0` and `SERVER.PORT=8080`; the binary itself has none. Ctrl-C/`docker stop` shuts down gracefully: actix handles SIGINT/SIGTERM as PID 1 [VERIFIED: actix-rt `signal` feature on via actix-server; SIGTERM exit in about 311 ms locally]. Security note: no authentication, and PATCH runs stored JS, so keep `127.0.0.1` publishing unless the network is trusted.

### docs/js-ops.md carry-overs (D-12)
- **03 IN-03** (lines 199-206): keep the facts but stop the overclaim. Suggested text: "`Deno.core.print(text)` writes `text` unchanged (raw, unescaped) to the server's stdout, not to the HTTP response, so a script can print anything there, including text that looks like rs3's own stdout output such as the startup banner. `Deno.core.print(text, true)` writes to the server's stderr instead; there rs3 escapes the text and writes one line per call, prefixed with `js stderr: ` …, so a script cannot fake rs3's stderr log lines. Treat everything a script prints, on either stream, as untrusted."
- **05 IN-02** [VERIFIED: 05-REVIEW.iter2.md:121-129]:
  - (1) Ops table, lines 25-28: add to the `op_rs3_list`/`op_rs3_get`/`op_rs3_insert`/`op_rs3_delete` "Throws when" cells: "after the depth limit stopped the PATCH: the depth error; after the timeout: returns an empty string and touches nothing".
  - (2) Add the exception: an object or symbol argument still throws its `TypeError` after the timeout. The argument conversion (`#[scoped] StrArg`) runs before the op body checks the timeout [VERIFIED: src/js/ops.rs:209, 177-179].
  - (3) Lines 210-212: "At most 4 scripts run in one PATCH chain" → "the nesting depth is at most 4 scripts … sibling `op_rs3_patch` calls at the same level are bounded only by the timeout".
  - (4) Lines 224-225: "Other routes and other PATCH requests keep being answered meanwhile" → "… other PATCH requests keep being answered while a JS worker is free (otherwise they wait in the queue)".
- **05 IN-05** (`src/js/executor.rs:147-148`, comment only): "Spawned from the worker, so the watchdog descends from the main thread that initialised V8 (`main` calls `init_platform`), through the worker." [VERIFIED: src/main.rs:15-16]. Check `cargo fmt --check` afterwards; the line must stay ≤ 100 columns.

## State of the Art

| Old Approach | Current Approach | When Changed | Impact |
|--------------|------------------|--------------|--------|
| `rust:1.53` ×3 + `cargo install cargo-chef` per stage | `lukemathwalker/cargo-chef:latest-rust-1.99.1-trixie` (chef preinstalled) | — | No per-stage cargo-chef compile |
| Separate `cacher` stage + `COPY --from=cacher target` and `$CARGO_HOME` | cook and build in one `builder` stage (separate layers) | upstream README pattern | Simpler, same caching |
| `ARG DISTRO` runtime | fixed `gcr.io/distroless/cc-debian13:nonroot` | — | Testable, glibc-matched, non-root |
| hadolint | BuildKit build checks (built into `docker build`, Dockerfile frontend ≥ 1.8, Buildx ≥ 0.15) | 2024 | Zero-dependency lint on the runner [CITED: docs.docker.com/build/checks] |
| rustup auto-install everywhere | rustup 1.29: auto-install still on by default for proxies; `rustup` subcommands warn that it is deprecated (`RUSTUP_AUTO_INSTALL=0` opts out) | rustup 1.28/1.29 | Docs should say "`rustup toolchain install` in the repo root" as the explicit step [VERIFIED: rustup 1.29.1 src/config.rs:145-170] |

**Deprecated/outdated:** the legacy `Dockerfile` (rust:1.53, `WORKDIR rs3` relative path, `cargo install --path .`, `ARG DISTRO`), and the legacy `Readme.md` (`curl https://localhost:5000/`: wrong scheme; "Create a .env file" with no format note).

## Assumptions Log

| # | Claim | Section | Risk if Wrong |
|---|-------|---------|---------------|
| A1 | The cold `docker build` on a 4-vCPU x86_64 `ubuntu-26.04` runner finishes well under the 30-minute job timeout (locally: cook 74 s + build 8 s on 4 aarch64 cores, plus image pulls) | ci.yml docker job | The job times out; raise `timeout-minutes` (and CANONICAL/JOBS) |
| A2 | Image size is about 90 to 100 MB | Pitfall 10 | Only informational; recorded in UAT |
| A3 | BuildKit default build checks flag nothing in the proposed Dockerfile (uppercase `AS`, lowercase stage names, absolute WORKDIR, `KEY=value` ENV, JSON ENTRYPOINT) | Open Question 3 | Without `check=error=true` only warnings; with it the docker job fails and names the rule |
| A4 | Kubernetes `runAsNonRoot` rejects non-numeric image users | Pitfall 8 | Documentation nuance only |
| A5 | GitHub-hosted runners can reach Docker Hub, gcr.io and GitHub release downloads without rate-limit failures for one build per push | docker job | A flaky pull; re-run the job |

## Open Questions

1. **`USER nonroot:nonroot` (D-02, locked) or numeric `65532:65532`?**
   - Known: both run as uid 65532 in Docker. The name resolves from distroless `/etc/passwd`.
   - Unclear: future Kubernetes use (`runAsNonRoot` wants numeric users).
   - Recommendation: **keep D-02** as locked. Guard (i) requires `nonroot:nonroot`, and the README mentions uid/gid 65532. Revisit with OPS-03.
2. **Builder tag `latest-rust-1.99.1-trixie` (D-01) or the pinned `0.1.78-rust-1.99.1-trixie`?**
   - Recommendation: **keep D-01**. Guard (i) accepts only the D-01 tag. Note the pinned tag in the README for reproducible builds; SHA pinning is OPS-03.
3. **Add `# check=error=true` as line 1 of the Dockerfile?**
   - Known: BuildKit runs build checks by default and prints violations as warnings. The directive makes them fatal. Supported by Docker 29.4.2 / Buildx 0.38 on the runner.
   - Recommendation: **include it.** It is the only Dockerfile lint available without hadolint, and the proposed Dockerfile follows every documented rule (A3). If the remote run fails on a rule, the log names it. Guard (i) ignores comment lines, so it needs no change.
4. **Manual alternative in 06-UAT (D-09 text uses `-p 8080:8080`).**
   - Recommendation: use `-p 127.0.0.1:8080:8080` in both the README and the UAT text, with the same `-e SERVER.HOST=0.0.0.0` and `curl http://127.0.0.1:8080/`. Functionally identical for the check and safer by default.
5. **Plan split.**
   - Recommendation: **06-01** (Wave 1): Dockerfile + `.dockerignore` + `scripts/docker-smoke.sh` + guard (i) + `fresh()` + the (i) planted cases. **06-02** (Wave 2, depends on 06-01): ci.yml docker job + check-workflow.py + the (g) planted cases + `06-UAT.md`. **06-03** (Wave 1, parallel-safe: no shared files): `Readme.md` + `docs/js-ops.md` + the executor comment. Guard (i)'s `bash -n scripts/docker-smoke.sh` means the smoke script must land with or before guard (i). The per-plan gate (TOOL-03 + guards + self-test) holds after each plan.
6. **Pre-existing: guards (d)/(e) exit silently under `set -e` when `cargo tree` fails** (Pitfall 5). Out of scope (they still fail closed). Recommendation: record it as an info item for a later quick fix; do not touch it in Phase 6.

## Environment Availability

| Dependency | Required By | Available | Version | Fallback |
|------------|------------|-----------|---------|----------|
| docker / podman / buildah / hadolint | Real image build and lint | ✗ (not on host; do not install) | — | CI `docker` job (remote = UAT) + guard (i) static check |
| Rust toolchain | build/test/gate | ✓ | 1.99.1 (rustup 1.29.1) with rustfmt + clippy | — |
| python3 + PyYAML | guards (tomllib), check-workflow --structure | ✓ | Python 3.12.3, PyYAML 6.0.1 | — |
| curl | smokes, README verification | ✓ | 8.5.0 | — |
| Network (crates.io, GitHub releases, registries) | V8 download in cold builds, D-08(c) simulation | ✓ | — | `RUSTY_V8_ARCHIVE` |
| Disk | builds | `/home` 45 GB free; `/` 96% full | — | Keep everything under `target/tmp` |
| Docker on CI runner | docker job | ✓ (per runner-images readme) | Docker 29.4.2, Buildx 0.38.0 | — |

**Missing dependencies with no fallback:** none blocking. The real `docker build`/`run` is deferred to the remote run (06-UAT).
**Missing dependencies with fallback:** Docker locally → guard (i) + context simulation + CI job.

## Validation Architecture

### Test Framework
| Property | Value |
|----------|-------|
| Framework | bash guard scripts + Python stdlib/PyYAML checker + `cargo test` (libtest, 171 tests at last count) |
| Config file | none (scripts/ are self-contained) |
| Quick run command | `bash scripts/ci-guards.sh && python3 scripts/check-workflow.py && bash -n scripts/docker-smoke.sh` |
| Full suite command | `cargo fmt --check && cargo clippy --all-targets --locked -- -D warnings && cargo test --locked && cargo build --release --locked && bash scripts/ci-smoke.sh && bash scripts/ci-guards.sh && bash scripts/test-ci-guards.sh` |

### Phase Requirements → Test Map
| Req / Decision | Behavior | Test Type | Automated Command | File Exists? |
|--------|----------|-----------|-------------------|-------------|
| DOCK-01, D-01 | builder tag = channel, non-slim, `--locked` on cook + build, `--release`, no `cargo install` | static | `bash scripts/ci-guards.sh` → `ok   [i]` | ❌ Wave 0: guard (i) |
| DOCK-01, D-08(c) | the allowlisted files alone build | integration (local, ~1.5 min) | `rm -rf target/tmp/ctx && mkdir -p target/tmp/ctx && cp -r Cargo.toml Cargo.lock src target/tmp/ctx/ && (cd target/tmp/ctx && CARGO_TARGET_DIR="$PWD/target" cargo build --release --locked --bin rs3) && rm -rf target/tmp/ctx` | ✅ (ad hoc; no file) |
| DOCK-02, D-02/D-03 | distroless debian13 nonroot final stage, `USER nonroot:nonroot`, ENV defaults, EXPOSE 8080, exec ENTRYPOINT | static | `bash scripts/ci-guards.sh` → `ok   [i]` | ❌ Wave 0 |
| DOCK-02, D-06 | container serves `GET /`, non-root, V8 works | remote e2e | CI job `docker` (`bash scripts/docker-smoke.sh`); manual: README Docker commands | ❌ Wave 0: smoke script; remote = **06-UAT item 1** |
| D-06 smoke logic | assertions and cleanup behave | local dry run (optional) | the fake-docker shim from this research (not committed; recreate under `target/tmp` if wanted) | — |
| D-08(b) | smoke parses | static | `bash -n scripts/docker-smoke.sh` (also inside guard (i)) | ❌ Wave 0 |
| DOCK-03, D-05 | `.dockerignore` starts with `*`, re-includes exactly Cargo.toml/Cargo.lock/src | static | `bash scripts/ci-guards.sh` → `ok   [i]` | ❌ Wave 0 |
| D-07 | two jobs pinned; guards and self-test pass | static + self-test | `python3 scripts/check-workflow.py && bash scripts/ci-guards.sh && bash scripts/test-ci-guards.sh` (expect `… cases, 0 missed`, about 130) | ✅ extend |
| DOCS-01 | toolchain, V8 note, build/run/test | doc check + live run | `grep -q '1.99.1' Readme.md && grep -qi 'edition 2024' Readme.md && grep -q 'RUSTY_V8_ARCHIVE' Readme.md && grep -q 'env SERVER.HOST=' Readme.md`; then run the quick start with `env SERVER.HOST=127.0.0.1 SERVER.PORT=<18550-18569>` and curl `/` | ❌ Wave 0 (README) |
| DOCS-02 | op syntax, mapping, example | doc check + live run | `grep -q 'Deno.core.ops.op_rs3_get' Readme.md && grep -q 'opSync' Readme.md && grep -q 'docs/js-ops.md' Readme.md`; run the fac/sq examples → `"120"`, `"25"` | ❌ Wave 0 |
| DOCS-03 | status codes + Behaviour changes | doc check + live run | `grep -q '^## Behaviour changes' Readme.md && for c in not_found malformed_json invalid_input js_error js_depth_limit js_timeout internal 413 504; do grep -q "$c" Readme.md || echo "missing $c"; done`; `! grep -q not_implemented Readme.md` | ❌ Wave 0 |
| D-12 IN-03 | no stdout overclaim | doc check | `! grep -q 'so a script cannot fake server log lines' docs/js-ops.md && grep -qi 'raw' docs/js-ops.md` | ✅ edit |
| D-12 IN-02 | ops table / limits wording | doc check | `grep -q 'while a JS worker is free' docs/js-ops.md && grep -qi 'nesting depth' docs/js-ops.md` | ✅ edit |
| D-12 IN-05 | comment names the main thread | doc check | `grep -q 'main thread that initialised V8' src/js/executor.rs && cargo fmt --check` | ✅ edit |
| D-09 | UAT recorded | file check | `test -f .planning/phases/06-docker-image-documentation/06-UAT.md && grep -q 'docker' …/06-UAT.md` | ❌ Wave 0 |
| TOOL-03 | still green | full | full suite command above | ✅ |

### Sampling Rate
- **Per task commit:** quick run command.
- **Per plan:** full suite command (TOOL-03 + ci-smoke + guards + self-test, about 4 to 6 min with the timeout smoke and the self-test).
- **Phase gate:** full suite green plus the D-08(c) context build, before `/gsd-verify-work`.

### Remote-run UAT item (06-UAT.md, item 1)
Push `modernize-toolchain` (operator only; the autonomous run never pushes). In Actions → `CI`, confirm:
- job `fmt, clippy, test, release smoke (x86_64)` is green; guards print `ok   [a]` to `ok   [i]`;
- job `docker image build and smoke (x86_64)` is green, its log shows `docker-smoke: OK (rs3:ci, port …, user nonroot:nonroot, uid 65532, image size … bytes)`, and there is no `info: downloading 2 components` in the build output;
- record the image size.

The same push satisfies 04-UAT item 1 and 05-UAT item 1. Manual alternative on any Docker host: `docker build -t rs3 . && docker run --rm -p 127.0.0.1:8080:8080 -e SERVER.HOST=0.0.0.0 rs3`, then `curl http://127.0.0.1:8080/` → `"Welcome to rs3"`.

### Wave 0 Gaps
- [ ] `Dockerfile` (rewrite), `.dockerignore`, `scripts/docker-smoke.sh`: DOCK-01..03
- [ ] guard (i) in `scripts/ci-guards.sh` + `fresh()` + (i) cases in `scripts/test-ci-guards.sh`
- [ ] ci.yml docker job + `scripts/check-workflow.py` JOBS/CANONICAL + (g) cases
- [ ] `Readme.md` rewrite, `docs/js-ops.md` fixes, executor comment
- [ ] `06-UAT.md`

## Security Domain

`security_enforcement` is enabled (ASVS level 1, block on high).

### Applicable ASVS Categories

| ASVS Category | Applies | Standard Control |
|---------------|---------|-----------------|
| V1 Architecture / V14 Configuration | yes | Minimal distroless runtime, non-root uid 65532, no shell; allowlist build context; `--locked` builds; read-only CI token (`contents: read`), no secrets in the docker job |
| V2 Authentication | no (rs3 has none; documented as a deployment risk) | README warns; loopback publish by default |
| V3 Session Management | no | — |
| V4 Access Control | no (no users) | Network-level: `-p 127.0.0.1:` |
| V5 Input Validation | unchanged (Phase 2/3/5 controls: 400/422, 256 KiB, depth/timeout) | documented in README |
| V6 Cryptography | no | — |
| V10 Malicious Code / supply chain | yes | Registry-verified base images; tags rather than digests (OPS-03 deferred); V8 prebuilt fetched over HTTPS from GitHub releases with no hash check by the build script (residual, same as host builds) |

### Known Threat Patterns for this phase

| Pattern | STRIDE | Standard Mitigation |
|---------|--------|---------------------|
| Secrets (the dotenv file, `docker_rs3/.secrets/`, `.git`) copied into image layers via `COPY . .` | Information disclosure | `.dockerignore` allowlist (`*` first), pinned by guard (i) |
| Container runs as root | Elevation of privilege | `USER nonroot:nonroot` (guard (i)), smoke checks Config.User and process UID |
| Glibc mismatch → crash or undefined behaviour at runtime | Denial of service | trixie builder ↔ debian13 runtime (guard (i)) |
| Unauthenticated JS execution reachable from the network | Elevation / DoS | README: loopback publishing by default; no heap limit documented; out of scope to add auth |
| Workflow weakening (`if:`, `continue-on-error`, `|| true`, env injection of `SERVER.`) on the new job | Tampering | CANONICAL lines + per-job structure check + planted cases |
| Floating base tags (`latest-…`, `nonroot`) change under you | Tampering (supply chain) | Accepted for v1; SHA pinning = OPS-03 |
| Build-time network fetch (V8 archive) tampered | Tampering | HTTPS from github.com; residual, documented |

## Sources

### Primary (HIGH confidence)
- Docker Hub API: `lukemathwalker/cargo-chef` tags `latest-rust-1.99.1-trixie`, `0.1.78-rust-1.99.1-trixie`; `library/rust:1.99.1-trixie`
- gcr.io registry API: `distroless/cc-debian13` tags list, `nonroot` index (amd64, arm64/v8, …), amd64 manifest, config blob (`User 65532`), layers (`etc/passwd`, `libc6 2.41-12+deb13u4`, os-release trixie)
- cargo-chef 0.1.78 crate source (crates.io): `src/main.rs` (Prepare/Cook flags), `src/recipe.rs` (forwards `--locked`/`--release`/`--bin`), `src/skeleton/{mod,read}.rs` (toolchain file copied into the skeleton, version masking); local install + 3-stage simulation
- rustup 1.29.1 source (github tag): `src/config.rs`, `src/cli/proxy_mode.rs`; local probe with a scratch RUSTUP_HOME
- rust-lang/docker-rust `stable/trixie/Dockerfile` and `slim/Dockerfile`; LukeMathWalker/cargo-chef `docker/Dockerfile` and README
- moby/patternmatcher `patternmatcher.go`; moby/buildkit `instructions/parse.go`; docker/cli `opts/env.go`, `cli/command/container/top.go`
- v8 150.4.0 `build.rs` (local registry); rusty_v8 v150.4.0 release assets (GitHub API)
- actix-web 4.17.0 `src/types/payload.rs`; actix-rt 2.15.0 Cargo.toml features; `cargo tree -e features -i actix-rt`
- Repo files read this session: `src/main.rs`, `src/config.rs`, `src/http/{mod,kv,error}.rs`, `src/js/{mod,executor,ops}.rs` (excerpts), `src/store/legacy.rs`, `docs/js-ops.md`, `scripts/{ci-guards.sh,check-workflow.py,test-ci-guards.sh,ci-smoke.sh}`, `.github/workflows/ci.yml`, `Dockerfile`, `Readme.md`, `env.example`, `rust-toolchain.toml`, `Cargo.toml`, review dispositions 03/05
- Live runs: release binary on ports 18550-18557 (all README bodies), startup error messages, SIGTERM shutdown; prototype guards (`all guards passed`) and self-test (`130 cases, 0 missed`)

### Secondary (MEDIUM confidence)
- docs.docker.com/build/concepts/context (.dockerignore rules); docs.docker.com/build/checks (build checks, `check=error=true`)
- actions/runner-images `Ubuntu2604-Readme.md` (Docker 29.4.2, Buildx 0.38.0, Python 3.14.4)

### Tertiary (LOW confidence)
- Web search on dotted env names in Docker (context only; superseded by the buildkit/docker-cli source reads): [HashiCorp discuss](https://discuss.hashicorp.com/t/using-environment-variable-with-dot-in-it/41666), [Atlassian BCLOUD-14892](https://jira.atlassian.com/browse/BCLOUD-14892)

## Metadata

**Confidence breakdown:**
- Standard stack: HIGH. Tags, users, glibc and flags were checked against registries and source; the three stages were simulated.
- Architecture: HIGH. The contract extension was prototyped, and the guards and self-test passed in a scratch copy.
- Pitfalls: HIGH for 1-7 and 9 (each verified by probe or source); MEDIUM for 8 and 10 (assumed).
- README facts: HIGH. Captured live from the release binary.

**Research date:** 2026-10-21
**Valid until:** 2026-11-20 (base-image tags move; re-check the tag existence if planning slips past a Rust point release)
