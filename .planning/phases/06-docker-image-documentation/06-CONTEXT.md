# Phase 6: Docker Image & Documentation - Context

**Gathered:** 2026-10-21
**Status:** Ready for planning
**Mode:** Smart discuss (autonomous, non-interactive — every grey area auto-accepted with the Recommended answer)

<domain>
## Phase Boundary

Ship rs3 as a slim, non-root Docker image built with cargo-chef caching and `--locked`, with a
`.dockerignore` that keeps `target/` and other non-build files out of the context. Rewrite
`Readme.md` so a new developer can set up Rust 1.99.1 / edition 2024, expect the slow first V8
build, build, run and test the server, use the `Deno.core.ops.op_rs3_*` syntax (old→new
mapping plus a working example), know the HTTP status codes, and see a "Behaviour changes"
section.

Docker is not installed on the dev host (and there is no hadolint, podman or buildah). The
image build and the `docker run` + `curl /` smoke are therefore automated as a CI job, which
runs on the next push. **The autonomous run never pushes**, so the remote result is a pending
human UAT item, consolidated with the 04/05 UAT push items. Local validation is static.

Requirements: DOCK-01, DOCK-02, DOCK-03, DOCS-01, DOCS-02, DOCS-03.

Not in this phase: musl/Alpine images (out of scope), multi-arch publishing or registries,
release profile tuning (v2 OPS-04). `docker_rs3/`, `aws/` and the `Vagrantfile` are left
untouched; a scan found no trivial Rust-version strings in them to update.

</domain>

<decisions>
## Implementation Decisions

### Dockerfile (replaces the legacy root `Dockerfile`)
- **D-01:** Three cargo-chef stages (planner → cook → build) on `lukemathwalker/cargo-chef:latest-rust-1.99.1-trixie`. Do not use a `-slim` builder, because the `v8` build.rs needs curl to download the prebuilt static lib. `cargo chef cook --release --locked --recipe-path recipe.json`, then `cargo build --release --locked --bin rs3`. The V8 download is baked into the cooked dependency layer.
- **D-02:** Runtime stage `gcr.io/distroless/cc-debian13:nonroot`. Debian 13 matches the trixie builder's glibc; never pair a trixie builder with a bookworm runtime. Copy only the `rs3` binary (e.g. to `/usr/local/bin/rs3`). Set `USER nonroot:nonroot` explicitly, even though the tag already defaults to it. Use exec-form `ENTRYPOINT ["/usr/local/bin/rs3"]` and `EXPOSE 8080`.
- **D-03:** Image env defaults `ENV SERVER.HOST=0.0.0.0` and `ENV SERVER.PORT=8080`, overridable with `docker run -e SERVER.PORT=…`. Dotted names are valid Docker env keys, and rs3 reads them from the process env (Phase 2 D-16 allowlist). The binary itself still has no built-in defaults; the image supplies them. Document this.
- **D-04:** The build must not depend on files outside the context allowlist (D-05). If `rust-toolchain.toml` is excluded, the builder's preinstalled 1.99.1 is used and rustup does not try to fetch the `rustfmt`/`clippy` components during the image build. Research must confirm the behaviour either way and choose (Claude's discretion after research).

### .dockerignore
- **D-05:** **Allowlist** style: `*` first, then re-include only what the build needs (`!Cargo.toml`, `!Cargo.lock`, `!src/`, plus anything research finds is required). This keeps `target/` (about 2.6 GB+), `.git`, `.planning`, `.claude`, `experiments/`, `aws/`, `docker_rs3/`, `requests/`, `scripts/`, docs and the `Vagrantfile` out of the context (DOCK-03).

### Image verification without local Docker
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

### README (`Readme.md`)
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

</decisions>

<canonical_refs>
## Canonical References

**Downstream agents MUST read these before planning or implementing.**

### Project scope
- `.planning/REQUIREMENTS.md`: DOCK-01..03, DOCS-01..03, plus Out of Scope (musl/Alpine; untouched dirs)
- `.planning/ROADMAP.md` §Phase 6: success criteria and notes (Docker not on host; non-slim builder; leave docker_rs3/aws/Vagrantfile alone)
- `.claude/CLAUDE.md` / `.planning/research/STACK.md`: builder image `lukemathwalker/cargo-chef:latest-rust-1.99.1-trixie`, runtime `gcr.io/distroless/cc-debian13:nonroot`, the glibc-match rule, the "do not use -slim builder" rule
- `.planning/research/PITFALLS.md`: Docker pitfalls (slim builder lacks curl, trixie/bookworm mismatch, missing `--locked`, missing `.dockerignore` → multi-GB context; smoke with `docker run` + curl)

### Prior phases (behaviour the README must describe accurately)
- `.planning/phases/02-http-api-port-error-handling/02-CONTEXT.md` + SUMMARYs: error contract, `/list`, config hardening
- `.planning/phases/03-javascript-runtime-port/03-CONTEXT.md` + SUMMARYs, `docs/js-ops.md`: op mapping, PATCH semantics
- `.planning/phases/05-js-execution-limits/05-CONTEXT.md` + SUMMARYs: limits, error codes
- `.planning/phases/04-ci-gate-on-x86-64/04-CONTEXT.md` (D-01 amended to ubuntu-26.04) + `scripts/check-workflow.py`, `scripts/ci-guards.sh`, `scripts/test-ci-guards.sh`: the workflow contract the new docker job must respect (CANONICAL lines must be updated in the same change)
- Review dispositions with doc carry-overs: `03-REVIEW-DISPOSITION.md` (IN-03), `05-REVIEW-DISPOSITION.md` (IN-02, IN-05)

</canonical_refs>

<code_context>
## Existing Code Insights

### Reusable Assets
- `scripts/ci-smoke.sh`: smoke pattern (free loopback port, readiness poll, exact body asserts, trap cleanup), reusable for `scripts/docker-smoke.sh`.
- `scripts/check-workflow.py` (`CANONICAL` list + PyYAML structure check), `scripts/ci-guards.sh`, `scripts/test-ci-guards.sh` (planted-violation self-test): extend for the docker job and the Dockerfile check.
- `docs/js-ops.md`: the full JS op contract the README summarises and links.
- `env.example` (`SERVER.HOST=localhost`, `SERVER.PORT=5000`): referenced by the README.

### Established Patterns
- Exact-body assertions, loopback-only smokes, trap cleanup, guard-backed contracts, no `continue-on-error`, no `SERVER.` in workflow text.

### Integration Points
- `Dockerfile` (rewrite), `.dockerignore` (new), `.github/workflows/ci.yml` (+ docker job), `scripts/docker-smoke.sh` (new), `scripts/check-workflow.py`, `scripts/ci-guards.sh`, `scripts/test-ci-guards.sh`, `Readme.md` (rewrite), `docs/js-ops.md` (carry-over fixes).

</code_context>

<specifics>
## Specific Ideas

- **Git identity (user instruction):** commit with env vars `GIT_AUTHOR_NAME="Johannes Maria Frank"`, `GIT_AUTHOR_EMAIL="jmfrank63@gmail.com"`, `GIT_COMMITTER_NAME="Johannes Maria Frank"`, `GIT_COMMITTER_EMAIL="jmfrank63@gmail.com"`. **Never modify git config.** Commit messages end with `Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>`.
- **Never `git push`** (an operator session may push separately). Stay on `modernize-toolchain`. Another session may commit concurrently: rebase or fast-forward cleanly, and never rewrite others' commits.
- The TOOL-03 gate, `bash scripts/ci-guards.sh` and `bash scripts/test-ci-guards.sh` must pass at the end of every plan.
- Keep `target/` inside the project dir (scratch under `target/tmp`; `/tmp` is about 96% full). A project hook blocks naming the dotenv file in commands, so README examples that mention it are fine as prose, but use env vars for any executed smoke.

</specifics>

<deferred>
## Deferred Ideas

- Release profile tuning (LTO, strip): v2 OPS-04.
- Image publishing, multi-arch builds, SBOM/signing: future.
- SHA-pinned base images and actions: v2 OPS-03.

</deferred>

---

*Phase: 06-docker-image-documentation*
*Context gathered: 2026-10-21 (smart discuss, auto-accepted recommendations)*
