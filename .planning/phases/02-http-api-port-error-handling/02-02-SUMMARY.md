---
phase: 02-http-api-port-error-handling
plan: 02
subsystem: config
tags: [dotenvy, config, env, startup-errors, tdd]

requires:
  - phase: 01-toolchain-dependencies-foundation
    provides: "config 0.15 builder path (Settings::from_map), dotenvy 0.15.7, main's 'error: invalid configuration' exit-1 arm"
provides:
  - "Settings::load() as a one-line shell over Settings::from_sources(dotenvy::dotenv_iter(), std::env::vars_os()), with no environment mutation"
  - "pub fn merge_dotenv_env: pure merge where the first occurrence wins in the file, env wins over the file, non-UTF-8 env entries are skipped, and only server.* keys pass"
  - "Settings::from_sources: a not-found dotenv is ignored; any other dotenv error becomes a '.env: ' ConfigError (never half-loaded)"
  - "16 config unit tests driven by in-memory dotenv content and explicit env lists"
affects: [02-03, phase-03-js-engine, phase-06-readme]

actuals:
  tokens: 3462
  tasks: 2
  commits: 3
plan_head_before: 276af7cbe44ea1c9c25060e1cb41e59281c613c3
plan_head_after: addda7fd0d7f84adb0b00bb947fd61d65fcbd12a

tech-stack:
  added: []
  patterns:
    - "Thin I/O shell (load) over pure, injectable functions (from_sources, merge_dotenv_env) so config tests never touch the process env or cwd"
    - "Dotenv errors become ConfigError::Message('.env: <dotenvy Display>') and surface through main's existing exit-1 arm"

key-files:
  created:
    - .planning/phases/02-http-api-port-error-handling/tdd-evidence/02-02-task2-red.json
    - .planning/phases/02-http-api-port-error-handling/tdd-evidence/02-02-task2-red.junit.xml
  modified:
    - src/config.rs

key-decisions:
  - "[02-02] Settings::load() reads the dotenv file via dotenvy::dotenv_iter() and the env via std::env::vars_os() and never writes the environment. The process env wins, and the first duplicate wins inside the file (old dotenvy never-override parity)."
  - "[02-02] A missing dotenv file is ignored. Any other dotenv error is a startup error 'error: invalid configuration: .env: <dotenvy Display>' with exit 1. The raw failing line stays in the message (T-02-10 accepted)."
  - "[02-02] Only keys whose ASCII-lowercased name starts with 'server.' enter the config map. Names are unchanged (dotted SERVER.HOST / SERVER.PORT). No defaults, no __ alias (OPS-02 stays deferred)."
  - "[02-02] A dotenv file that starts with a UTF-8 BOM is now a startup error, pinned by bom_prefixed_dotenv_is_error. The Phase 6 README must document it."

patterns-established:
  - "Config tests feed dotenvy::from_read_iter(bytes) or a constructed dotenvy::Error::Io(..) plus a Vec<(OsString, OsString)> env list into from_sources / merge_dotenv_env"

requirements-completed: [ARCH-03, TEST-02]

coverage:
  - id: D1
    description: "Settings::load() reads the dotenv file and the environment without mutating either. Nothing in src/ calls set_var, remove_var or dotenvy::dotenv()."
    requirement: ARCH-03
    verification:
      - kind: other
        ref: "! grep -rnE 'dotenvy::dotenv\\(\\)|set_var|remove_var' src/ && grep -q 'dotenvy::dotenv_iter()' src/config.rs && grep -q 'vars_os()' src/config.rs"
        status: pass
      - kind: unit
        ref: "src/config.rs#config::tests::dotenv_source_configures_settings"
        status: pass
    human_judgment: false
  - id: D2
    description: "A malformed or unreadable dotenv source is a '.env: ' configuration error with no half-load. A missing file is ignored."
    requirement: TEST-02
    verification:
      - kind: unit
        ref: "src/config.rs#config::tests::malformed_dotenv_is_error_with_env_prefix"
        status: pass
      - kind: unit
        ref: "src/config.rs#config::tests::unreadable_dotenv_is_error"
        status: pass
      - kind: unit
        ref: "src/config.rs#config::tests::missing_dotenv_file_uses_env_only"
        status: pass
    human_judgment: false
  - id: D3
    description: "Only server.* keys reach the config. Env overrides the file, the first occurrence wins in the file, non-UTF-8 entries are skipped, lowercase keys are accepted, and a BOM is rejected."
    requirement: TEST-02
    verification:
      - kind: unit
        ref: "src/config.rs#config::tests::merge_keeps_only_server_keys"
        status: pass
      - kind: unit
        ref: "src/config.rs#config::tests::bare_server_var_does_not_break_settings"
        status: pass
      - kind: unit
        ref: "src/config.rs#config::tests::env_overrides_dotenv"
        status: pass
      - kind: unit
        ref: "src/config.rs#config::tests::dotenv_first_occurrence_wins"
        status: pass
      - kind: unit
        ref: "src/config.rs#config::tests::non_utf8_env_entries_are_skipped"
        status: pass
      - kind: unit
        ref: "src/config.rs#config::tests::lowercase_server_keys_are_accepted"
        status: pass
      - kind: unit
        ref: "src/config.rs#config::tests::bom_prefixed_dotenv_is_error"
        status: pass
      - kind: other
        ref: "env -i PATH=\"$PATH\" HOME=\"$HOME\" cargo test --lib config:: (16 passed)"
        status: pass
    human_judgment: false
  - id: D4
    description: "Real binary started from an empty directory: env-only config serves /ip (port 18463), a stray SERVER=x / OTHER=1 does not break startup (port 18464), and with no config it exits 1 with 'error: invalid configuration' and no panic."
    requirement: ARCH-03
    verification:
      - kind: integration
        ref: "env -i -C <empty dir> PATH SERVER.HOST=127.0.0.1 SERVER.PORT=18463 target/debug/rs3; curl /ip"
        status: pass
      - kind: integration
        ref: "env -i -C <empty dir> PATH SERVER=x OTHER=1 SERVER.HOST=127.0.0.1 SERVER.PORT=18464 target/debug/rs3; curl /ip"
        status: pass
      - kind: integration
        ref: "timeout 30 env -i -C <empty dir> PATH target/debug/rs3 (exit 1, no panic)"
        status: pass
    human_judgment: false
  - id: D5
    description: "Live dotenv-file paths: a malformed .env exits 1 with '.env: Error parsing line:', a valid .env with SERVER/OTHER noise configures /ip, and env overrides the file."
    requirement: ARCH-03
    verification: []
    human_judgment: true
    rationale: "A project hook forbids the agent from creating or naming the dotenv file, so these three live checks are operator UAT items. Everything except dotenvy's own file lookup is covered in-process by the from_sources unit tests."

duration: 5min
completed: 2026-10-20
status: complete
---

# Phase 2 Plan 02: Config Without Env Mutation Summary

**Settings::load() now merges `dotenvy::dotenv_iter()` pairs under `std::env::vars_os()` through a pure `merge_dotenv_env`, so it no longer writes the environment. A broken dotenv file now stops startup with `error: invalid configuration: .env: ...`, and only `server.*` keys reach the config.**

## Performance

- **Duration:** about 5 min
- **Started:** 2026-10-20T18:20:05Z
- **Completed:** 2026-10-20T18:25:21Z
- **Tasks:** 2
- **Files modified:** 3 (src/config.rs plus 2 RED evidence files)

## Accomplishments
- WR-02 fixed: `load()` is `Self::from_sources(dotenvy::dotenv_iter(), std::env::vars_os())`. Nothing in `src/` mutates the process environment, so `load()` is sound whatever the thread count.
- WR-01 fixed: a dotenv parse or I/O error (anything except not-found) is a `ConfigError::Message(".env: ...")`, returned before any pair is used, so a broken file is never half-loaded. Main's existing arm prints it and exits 1.
- IN-07 / D-16: only `server.*` keys (case-insensitive) pass the merge. A stray `SERVER=x` no longer collides with the server table. The live smoke on port 18464 proves this.
- TEST-02: 11 new config unit tests (16 in total) run on in-memory dotenv content and explicit env lists. They pass under `env -i`.

## Task Commits

1. **Task 1 (tracer): load() reads the dotenv file and env without mutating either; dotenv errors stop startup.** `2990013` (fix)
2. **Task 2 (TDD): server.* allow-list plus precedence, UTF-8 and BOM pins**
   - RED: `cfde775` (test)
   - GREEN: `addda7f` (feat)
   - REFACTOR: none needed

**Plan metadata:** see the docs(02-02) commit that follows.

## Files Created/Modified
- `src/config.rs`: new module doc, `merge_dotenv_env` (pure merge, precedence, UTF-8 skip, `server.` filter), private `dotenv_error`, `Settings::from_sources`, `load()` as a one-line shell, and 11 new tests plus `env` / `not_found` helpers. `from_map` and `from_env_source` are unchanged.
- `.planning/phases/02-http-api-port-error-handling/tdd-evidence/02-02-task2-red.{json,junit.xml}`: RED evidence for the Task 2 target.

## Error text from the malformed in-memory dotenv test

`malformed_dotenv_is_error_with_env_prefix` (content `SERVER.HOST=a\nSERVER.PORT 5000\n`) produces:

```
.env: Error parsing line: 'SERVER.PORT 5000', error at line index: 12
```

At startup main prints this as `error: invalid configuration: .env: Error parsing line: 'SERVER.PORT 5000', error at line index: 12`. There is no line number (dotenvy reports a character index), so tests assert only the `.env: ` prefix.

## TDD Gate Compliance

- **RED** `test(02-02)` `cfde775`: I added seven behaviour tests. The target `config::tests::merge_keeps_only_server_keys` failed on its key-set `assert_eq!` (`left: ["OTHER", "PATH", "SERVER", "SERVER.HOST", "SERVER.PORT"]`, `right: ["SERVER.HOST", "SERVER.PORT"]`). `bare_server_var_does_not_break_settings` also failed on the order-dependent `SERVER` collision, as expected. I re-ran the target alone through libtest's JUnit reporter: `RUSTC_BOOTSTRAP=1 target/debug/deps/rs3-d5bd492b7c3dd84a config::tests::merge_keeps_only_server_keys --exact -Z unstable-options --format junit`, exit 101. `RUSTC_BOOTSTRAP=1` was set only on that run of the already-built test binary, never on cargo or rustc. `gsd-tools check tdd-red-evidence` returned **`RED_EVIDENCE_OK`** (`target_test_failed`). **Semantic assessment:** the target compiled, ran and failed on its planned key-set assertion, because `merge_dotenv_env` had no `server.` allow-list yet. There was no build, load, fixture or unrelated failure. The other five new tests already passed at RED, as expected: Task 1 had already delivered precedence, the UTF-8 skip and dotenv error propagation. Evidence is in `tdd-evidence/02-02-task2-red.{json,junit.xml}`.
- **GREEN** `feat(02-02)` `addda7f`: one `retain(|key, _| key.to_ascii_lowercase().starts_with("server."))` at the end of `merge_dotenv_env`. 16/16 config tests pass, 55 in total.
- **REFACTOR:** not needed.

## Decisions Made
- `bare_server_var_does_not_break_settings` runs `from_sources` 32 times. config applies keys in HashMap order, and the repeat makes the order-dependent collision fail reliably rather than intermittently. This is how the test meets the plan's "deterministically" requirement.
- The `.env: ` error mapping lives in one private `dotenv_error` helper, used by both `merge_dotenv_env` and `from_sources`.
- `non_utf8_env_entries_are_skipped` also checks a valid key with a non-UTF-8 value (`SERVER.BAD`), so the test proves the filter that runs after the skip did not hide it.

## Deviations from Plan

None. The plan executed exactly as written.

## Issues Encountered
- A stray `source` of a markdown reference file in one shell call hung until the 2-minute timeout. It had no side effects: the RED evidence JSON was written before the hang, and I checked it on disk afterwards.

## Pending Operator UAT (dotenv-file human-checks)

A project hook stops the agent from creating the dotenv file, so these three live checks are queued for the operator:
1. **Malformed file (Task 1):** in an empty dir under /home/t3code, create `.env` with `SERVER.HOST=127.0.0.1` and `SERVER.PORT 5000`. Run `env -i PATH="$PATH" /home/t3code/rs3/Projects/rs3/target/debug/rs3`. Expect exit 1, one stderr line starting `error: invalid configuration: .env: Error parsing line:`, no banner and no panic.
2. **Valid file with noise (Task 2a):** `.env` with `SERVER.HOST=127.0.0.1`, `SERVER.PORT=18463`, `SERVER=x`, `OTHER=1`. Run `env -i PATH="$PATH" .../rs3`. Then `curl -s http://127.0.0.1:18463/ip` should print `"{'IP' : '127.0.0.1'}"`. Stop the server.
3. **Env overrides the file (Task 2b):** change the host line to `SERVER.HOST=10.255.255.1` and run `env -i PATH="$PATH" SERVER.HOST=127.0.0.1 .../rs3`. /ip on port 18463 should still report 127.0.0.1. Stop the server.

## User Setup Required

None. No external service configuration is required.

## Next Phase Readiness
- Plan 02-03 (wave 2) can turn on `unwrap_used` / `expect_used = "deny"`. Non-test code in `src/config.rs` has no unwrap or expect.
- The Phase 6 README must document two behaviour changes: a BOM-prefixed `.env` is a startup error, and a malformed `.env` stops startup instead of being half-applied.

---
*Phase: 02-http-api-port-error-handling*
*Completed: 2026-10-20*

## Self-Check: PASSED
