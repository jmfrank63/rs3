---
phase: 02-http-api-port-error-handling
plan: 03
subsystem: api
tags: [actix-web, web-bytes, serde_json, classify, thiserror, clippy, unwrap_used, list-escaping, tdd]

requires:
  - phase: 02-http-api-port-error-handling
    provides: "02-01 AppState + http::configure(state), ApiError with exhaustive status/code match, assert_json_error(resp, expected, code), test_state(); 02-02 env-free Settings::load"
provides:
  - "StoreError::MalformedJson (400) next to InvalidInput (422); parse_insert_body splits serde_json errors on classify()"
  - "POST /insert on raw web::Bytes: strict UTF-8, any content type or charset, 400 malformed_json / 422 invalid_input, nothing written on either"
  - "Infallible Store::insert_all (replaces the Result-item insert_many) with the partial-batch doc caveat"
  - "Escaped legacy /list codec (store::legacy::format_list) shared with Phase 3 op_rs3_list"
  - "[lints.clippy] unwrap_used/expect_used = deny plus clippy.toml test allowances"
  - "Full Phase 2 gate green on the merged tree"
affects: [03-js-runtime-port, 04-ci, 06-docs]

actuals:
  tokens: 6783
  tasks: 3
  commits: 4
plan_head_before: caac0b273ae2215b4a55108c37c9ac0c5a57f787
plan_head_after: 9e763d148b41f69e6275f0c65fd85c19bc81c6b4

tech-stack:
  added: []
  patterns:
    - "Error classification in the codec: serde_json::Error::classify() decides 400 vs 422; shape checks are the only 422 source"
    - "Raw-bytes extraction: web::Bytes + std::str::from_utf8 with map_err into StoreError, never the String or Json extractor"
    - "Validate-then-write: parse the whole body, then call the infallible insert_all"
    - "Lint gate without suppression: deny in Cargo.toml, test exemptions only through clippy.toml"

key-files:
  created:
    - clippy.toml
    - .planning/phases/02-http-api-port-error-handling/tdd-evidence/02-03-task3-red.json
    - .planning/phases/02-http-api-port-error-handling/tdd-evidence/02-03-task3-red.junit.xml
  modified:
    - src/store/mod.rs
    - src/store/legacy.rs
    - src/http/error.rs
    - src/http/kv.rs
    - Cargo.toml

key-decisions:
  - "classify() is matched with explicit Syntax | Eof | Io arms (no wildcard), matching the no-catch-all style of ApiError"
  - "insert_all returns the count and takes one pin() guard; validation lives only in parse_insert_body, so Phase 3's op_rs3_insert reuses parse-then-insert_all"
  - "Duplicate keys in one body are defined behaviour (first position, last value), pinned by insert_duplicate_key_last_value_wins"
  - "actix's built-in text/plain 413 is kept (D-05); tests assert only the status"

patterns-established:
  - "HTTP test helpers per concern: assert_insert_400(payload), raw_insert(payload) without content type, list_body(&state)"
  - "/list round-trip check: decode the outer JSON string, parse the inner text as Vec<Map<String, Value>>, compare to the stored pairs"

requirements-completed: [HTTP-03, HTTP-04, ROBU-02, HTTP-08, TEST-01, TEST-02]

coverage:
  - id: D1
    description: "Malformed, empty, whitespace, truncated, trailing-character, deeply nested or non-UTF-8 insert bodies answer 400 application/json malformed_json and write nothing; any content type and charset is accepted"
    requirement: ROBU-02
    verification:
      - kind: unit
        ref: "src/store/legacy.rs#parse_rejects_malformed_json, parse_rejects_empty_and_whitespace, parse_rejects_trailing_characters, parse_rejects_deep_nesting"
        status: pass
      - kind: unit
        ref: "src/http/kv.rs#insert_malformed_is_400_json, insert_empty_body_is_400_json, insert_invalid_utf8_is_400_json, insert_deeply_nested_is_400_json, insert_without_content_type_is_ok, insert_with_bogus_charset_is_ok"
        status: pass
      - kind: e2e
        ref: "live smoke 127.0.0.1:18465: {bad -> 400 application/json malformed_json; FF FE -> 400; curl -d echo; text/plain; charset=bogus echo"
        status: pass
    human_judgment: false
  - id: D2
    description: "Valid JSON of the wrong shape (non-object, null, non-string value) answers 422 invalid_input naming the key, and nothing from the body is inserted; repeated rejections are stable"
    requirement: ROBU-02
    verification:
      - kind: unit
        ref: "src/store/legacy.rs#parse_null_is_invalid_input, parse_rejects_non_object, parse_rejects_non_string_value"
        status: pass
      - kind: unit
        ref: "src/http/kv.rs#insert_non_object_is_422_json, insert_non_string_value_is_422_and_inserts_nothing, insert_rejected_twice_leaves_store_unchanged"
        status: pass
      - kind: e2e
        ref: "live smoke 18466: {\"a\":\"1\",\"b\":2} -> 422 application/json invalid_input 'key b'; GET /key/a -> 404; [1,2] -> 422"
        status: pass
    human_judgment: false
  - id: D3
    description: "Valid inserts go through the infallible insert_all: {} is a 200 no-op echoing {}, duplicates keep the last value, overwrites work, non-ASCII round-trips byte-exact, over-256-KiB bodies get 413"
    requirement: HTTP-03
    verification:
      - kind: unit
        ref: "src/http/kv.rs#insert_empty_object_is_noop, insert_duplicate_key_last_value_wins, insert_overwrites_existing_key, insert_non_ascii_round_trips, insert_over_256_kib_is_413"
        status: pass
      - kind: unit
        ref: "src/store/mod.rs#insert_all_writes_all_and_counts, concurrent_inserts_are_all_visible, concurrent_remove_of_one_key_succeeds_once"
        status: pass
      - kind: e2e
        ref: "live smoke 18466: {} -> \"{}\"; 300000-byte body -> 413"
        status: pass
    human_judgment: false
  - id: D4
    description: "GET /list keeps the legacy envelope with serde_json-escaped keys and values; plain data is byte-identical, empty store lists [], entries sorted by key byte order, values cannot forge entries"
    requirement: HTTP-04
    verification:
      - kind: unit
        ref: "src/store/legacy.rs#format_list_shape, format_list_empty_is_brackets, format_list_single_entry, format_list_escapes_quotes_and_backslashes, format_list_round_trips, format_list_cannot_forge_entries"
        status: pass
      - kind: unit
        ref: "src/http/kv.rs#list_escapes_quotes_and_backslashes, list_golden_sorted, list_empty_is_brackets_json, test_list_is_ok"
        status: pass
      - kind: e2e
        ref: "live smoke 18467: forging value and quote/backslash value -> inner /list text parses to exactly 2 entries, byte-exact"
        status: pass
    human_judgment: false
  - id: D5
    description: "unwrap/expect denied outside tests with no suppression attribute anywhere and no lowered lint"
    requirement: HTTP-08
    verification:
      - kind: other
        ref: "cargo clippy --all-targets -- -D warnings with [lints.clippy] unwrap_used/expect_used = deny; grep gates for allow/expect/cfg_attr in src/ and lowered lints in Cargo.toml"
        status: pass
    human_judgment: false
  - id: D6
    description: "Full Phase 2 gate on the merged tree: TOOL-03 four commands, env-free, serial and 5x runs, static/ARCH-01 gate, all-routes golden smoke"
    requirement: TEST-01
    verification:
      - kind: other
        ref: "cargo build --locked && cargo test && cargo clippy --all-targets -- -D warnings && cargo fmt --check"
        status: pass
      - kind: other
        ref: "env -i PATH HOME cargo test; cargo test -- --test-threads=1; 5x cargo test -q (79 passed, 2 ignored each)"
        status: pass
      - kind: e2e
        ref: "live smoke 18468: /, /ip, insert echo, GET, /list, PATCH 501, DELETE, then 404 x3 and \"[]\""
        status: pass
    human_judgment: false

duration: 5min
completed: 2026-10-20
status: complete
---

# Phase 2 Plan 03: Input Validation, Escaped /list and Lint Gate Summary

**POST /insert now reads raw `web::Bytes` and uses serde_json's `classify()` to split errors: a malformed body answers 400 `malformed_json` and a wrong-shape body answers 422 `invalid_input`, and neither writes anything. Valid bodies are written through the infallible `Store::insert_all`. `/list` escapes every key and value with serde_json inside the legacy `[{"k" : "v"},...]` envelope, so a stored value can no longer forge an entry. `unwrap_used`/`expect_used` are denied outside tests with no suppression anywhere, and the full Phase 2 gate passes.**

## Performance

- **Duration:** about 5 min
- **Started:** 2026-10-20T18:28:24Z
- **Completed:** 2026-10-20T18:33:42Z
- **Tasks:** 3 (1 tracer, 1 auto, 1 TDD)
- **Files modified:** 6 source/config files (1 created, 5 modified) plus 2 RED-evidence files

## Accomplishments

- Added `StoreError::MalformedJson` with the message `malformed JSON: {0}`. `parse_insert_body` maps `Category::Syntax | Eof | Io` to MalformedJson and `Data` to InvalidInput (a defensive arm, since parsing into `Value` never yields Data). `ApiError` maps MalformedJson to `malformed_json` / 400, and its match still has no wildcard arm.
- `insert` now takes `body: web::Bytes`. It decodes with `std::str::from_utf8`, maps errors with `map_err` into `MalformedJson("body is not valid UTF-8: ...")`, and accepts any content type or charset. `curl -d` with its form content type still echoes.
- `Store::insert_all` replaces `insert_many` and its two tests. It takes one `pin()` guard, returns the count, and its doc comment states that concurrent readers may observe a partially written batch (D-13, review IN-03).
- `format_list` renders `{<json key> : <json value>}` through `Value::from(&str)` Display. The `,]` replace quirk is gone (D-09). Output is byte-identical to legacy for plain data, and `/list` is still the one shared codec.
- `Cargo.toml` now has `[lints.clippy] unwrap_used = "deny"` and `expect_used = "deny"`. The new root `clippy.toml` sets `allow-unwrap-in-tests = true` and `allow-expect-in-tests = true`. Clippy is clean, and no non-test code needed a fix.
- Added 31 tests in total (5 codec parse, 4 codec list, 2 store, 20 HTTP), and the quirk test and the two `insert_many_*` tests were removed. The final lib suite has **79 passed, 2 ignored** (only the two parked PATCH tests are ignored).

## Observed bodies (live smokes)

- **400 (18465):** `POST /insert` with `{bad` gives `400 application/json` and `{"error":"malformed_json","message":"malformed JSON: key must be a string at line 1 column 2"}`. Bytes `FF FE` give 400.
- **422 (18466):** `{"a":"1","b":2}` gives `422 application/json` and `{"error":"invalid_input","message":"invalid input: value for key b must be a string"}`. Afterwards `GET /key/a` gives 404, `[1,2]` gives 422 and `{}` gives `"{}"`.
- **413 (18466):** a 300000-byte body gets status 413 (actix's built-in text/plain response).
- **Escaped /list (18467):** `"[{\"f\" : \"x\\\"},{\\\"admin\\\" : \\\"true\"},{\"q\" : \"a\\\"b\\\\c\"}]"`. The inner text parses to exactly 2 entries, and both values round-trip byte-exact.
- **Golden (18468):** `"Welcome to rs3"`, `"{'IP' : '127.0.0.1'}"`, the insert echo `"{\"insert_test\":\"John Doe\"}"`, `"John Doe"`, and `"[{\"insert_test\" : \"John Doe\"}]"`. PATCH on the existing key gives 501 and DELETE gives `"John Doe"`. After that, GET, PATCH and DELETE give 404, and `/list` gives `"[]"`.

## Task Commits

1. **Task 1 (tracer): malformed insert body -> JSON 400 end to end** - `c390647` (feat)
2. **Task 2: 422 wrong shape, insert_all, lint gate** - `84b2b1e` (feat)
3. **Task 3 (TDD): escaped /list + phase gate**
   - RED: `2dc15f8` (test)
   - GREEN: `9e763d1` (feat)
   - REFACTOR: none needed

## TDD Gate Compliance (Task 3)

- **RED:** `format_list_round_trips`, `format_list_escapes_quotes_and_backslashes`, `format_list_cannot_forge_entries` and `http::kv::tests::list_escapes_quotes_and_backslashes` were written against the legacy unescaped codec. All four failed on their planned assertions: the output was invalid JSON (`expected ',' or '}'`), a raw quote and backslash were emitted, and the forging value parsed as 2 entries instead of 1. JUnit evidence for `store::legacy::tests::format_list_round_trips` was captured with `RUSTC_BOOTSTRAP=1` on the test binary only (exit 101, tests=1, failures=1). `gsd-tools check tdd-red-evidence` returned **RED_EVIDENCE_OK** (`target_test_failed`). **Semantic assessment:** the target compiled and ran, and it failed inside its round-trip parse for the intended reason (unescaped values break the JSON). It was not a build, load, fixture or unrelated failure. Evidence files: `tdd-evidence/02-03-task3-red.json` and `tdd-evidence/02-03-task3-red.junit.xml`.
- **GREEN:** rewrote `format_list` with `Value::from` Display and deleted the replace step. All 79 lib tests pass.
- **Gate log:** `test(02-03)` at `2dc15f8` precedes `feat(02-03)` at `9e763d1`. Compliant.

## Files Created/Modified

- `src/store/mod.rs`: adds `MalformedJson`, replaces `insert_many` with `insert_all` (partial-batch doc), and adds the `insert_all_writes_all_and_counts` and `concurrent_remove_of_one_key_succeeds_once` tests
- `src/store/legacy.rs`: adds the `classify()` split in `parse_insert_body`, makes `format_list` escape through serde_json, and adds the new parse and list tests
- `src/http/error.rs`: maps `malformed_json` / 400
- `src/http/kv.rs`: makes `insert` take `web::Bytes` with a strict UTF-8 check and call `insert_all`, and adds 20 insert and list tests
- `Cargo.toml`: adds the `[lints.clippy]` table (the only change; Cargo.lock is untouched)
- `clippy.toml` (new): contains only the two test allowances

## Decisions Made

- I matched `classify()` with explicit `Syntax | Eof | Io` arms and no wildcard, in line with the exhaustive `ApiError` matches.
- `insert_all` counts while it writes and does not collect first, so a batch is never cloned.
- In `parse_rejects_malformed_json` I also assert `{bad` (the smoke payload) so that the codec and the live smoke check the same input.
- Sorted `/list` shipped in Phase 1 (review IN-04). `list_golden_sorted` pins it, and this plan did not introduce it.

## Deviations from Plan

None. The plan executed exactly as written. The lint gate flagged nothing in files this plan does not own, and the ownership guard passed on every task.

## Issues Encountered

- One compile error came from my own Task 1 test: `actix_http::Request` has no `headers()` method in scope. I fixed it with `req.head().headers` before committing.
- The first Task 2 edit script failed in Python on a `\u` escape in an inline string. I rewrote the edits through quoted heredoc temp files under `/home/t3code`, which I deleted afterwards. The code was not affected.

## Threat Mitigations

T-02-12 (forgery), T-02-13 (partial insert), T-02-14 (413), T-02-15 (deep nesting), T-02-16 (UTF-8 and content type) and T-02-17 (lint gate integrity) each have a test and a live-smoke check, listed above. T-02-SC holds: no packages were added, Cargo.lock is unchanged, and every build used `--locked`. This plan adds no new attack surface.

## User Setup Required

None. The 02-02 dotenv-file `<human-check>` items remain pending operator UAT. Agents did not run them.

## Next Phase Readiness

- Phase 2 is complete, and every gate is green on the merged tree.
- For Phase 3, `op_rs3_insert` should call `parse_insert_body` and then `insert_all`, and `op_rs3_list` should call `legacy::format_list`. The two parked PATCH tests are still ignored.
- Phase 6 README should document the built-in text/plain 413 at 256 KiB, the 400/422 split, and the `/list` escaping change.

## Self-Check: PASSED

- All 8 key files exist, and commits `c390647`, `84b2b1e`, `2dc15f8` and `9e763d1` are ancestors of HEAD.
- `check evaluation-scope --plan 02-03 --commits-only` resolved 4 commits on this branch.
- All acceptance criteria and plan `<verification>` commands were re-run and exited 0: TOOL-03, env-free, serial, 5x, static/ARCH-01, TOOL-01 regression, and the 18465/18466/18467/18468 smokes.

---
*Phase: 02-http-api-port-error-handling*
*Completed: 2026-10-20*
