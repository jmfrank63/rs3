---
phase: 02-http-api-port-error-handling
fixed_at: 2026-10-20T19:30:00Z
review_path: .planning/phases/02-http-api-port-error-handling/02-REVIEW.md
iteration: 1
findings_in_scope: 3
fixed: 3
skipped: 0
status: all_fixed
---

# Phase 2: Code Review Fix Report

**Fixed at:** 2026-10-20T19:30:00Z
**Source review:** .planning/phases/02-http-api-port-error-handling/02-REVIEW.md
**Iteration:** 1

**Summary:**
- Findings in scope: 3 (fix scope `critical_warning`: CR-01, WR-01, WR-02)
- Fixed: 3
- Skipped: 0
- Out of scope (Info, not attempted): IN-01, IN-02, IN-03, IN-04. The IN-04 last-wins rule is now
  documented in the `parse_insert_body` doc comment as part of WR-02, but the behaviour did not change.

## Fixed Issues

### CR-01: Env-over-dotenv precedence is non-deterministic when key case differs

**Files modified:** `src/config.rs`
**Commit:** 49504bc
**Status:** fixed: requires human verification (precedence logic)
**Applied fix:** `merge_dotenv_env` now lowercases every key (`to_ascii_lowercase`) before it decides
precedence. The merged map therefore never holds two spellings of one config key, and `config` can
no longer resolve such a collision in random `HashMap` order. The process environment now always
beats the dotenv file, and the first dotenv occurrence always wins, whatever the case of the key.
If the environment holds two spellings of the same key, the later entry in `vars_os()` order wins.
That order is fixed for a given environment, and the doc comment now says so. Existing tests were
updated for the lowercase keys. Two regression tests were added:
- `env_overrides_dotenv_across_case` checks both case directions. It asserts the merged map has
  exactly 2 entries with `server.host = env`, and it repeats the full `Settings::from_sources`
  load 64 times.
- `dotenv_first_occurrence_wins_across_case` covers a dotenv file holding `SERVER.HOST=first`
  followed by `server.host=second`.

I confirmed that both new tests fail when the merge body is reverted to the old logic. They fail on
the merged-map assertion, so the failure is deterministic and does not depend on `HashMap` order.

### WR-01: `server.` prefix filter still admits keys that collide with the server table

**Files modified:** `src/config.rs`
**Commit:** bd2137e
**Applied fix:** The `starts_with("server.")` filter was replaced with an exact allowlist,
`const SERVER_KEYS: [&str; 2] = ["server.host", "server.port"]`. It is applied after the CR-01
lowercasing, so matching is case-insensitive, which is consistent with D-16. The module and
function docs were updated. Test changes:
- New test `nested_server_keys_do_not_break_settings`: `SERVER.HOST.X` and
  `SERVER.PORT.ANYTHING` set next to valid keys still configure `host = h`, `port = 3`, checked
  over 32 repetitions.
- `merge_keeps_only_server_keys` now also passes `SERVER.HOST.X` and `SERVER.OTHER` and expects
  them to be dropped.
- `non_utf8_env_entries_are_skipped` now places a non-UTF-8 `SERVER.HOST` value after the valid
  one and asserts that the valid value survives. The old `SERVER.BAD` check would have passed
  without testing anything under the allowlist.

Both new assertions fail when the old prefix filter is put back.

### WR-02: Valid JSON with an out-of-range number answers 400 `malformed_json` instead of 422

**Files modified:** `src/store/legacy.rs`, `src/http/kv.rs`
**Commit:** 3514062
**Status:** fixed: requires human verification (400/422 classification logic)
**Applied fix:** `parse_insert_body` now works in passes:
1. **Syntax pass.** `serde_json::from_str::<serde::de::IgnoredAny>` scans numbers without
   converting them. Only real syntax, EOF or I/O errors return 400 `MalformedJson`.
2. **Value pass.** The body is built into a `Value`. If that fails, the text is valid JSON that
   serde_json cannot represent.
3. **Key search.** Only when the value pass fails, a small `DeserializeSeed`/`Visitor`
   (`FindBadKey`) walks the top-level object. It records each key before building that key's
   value, so the error is `InvalidInput("value for key a must be a string: number out of range
   ...")` and names the key, as the orchestrator required. When the whole body is unrepresentable
   (`1e400`, `[1e400]`), the error is `InvalidInput("insert body must be a JSON object: ...")`.

`arbitrary_precision` was not enabled. Tests added:
- `parse_out_of_range_number_is_invalid_input_naming_key` (positive and negative exponent)
- `parse_out_of_range_number_body_is_invalid_input`
- `parse_valid_deep_nesting_is_invalid_input`
- `parse_syntax_error_next_to_big_number_is_malformed`
- HTTP test `insert_out_of_range_number_is_422_naming_key`, which checks 422 `invalid_input`, that
  the message contains `key a`, and that nothing was written.

**Behaviour change to check:** `IgnoredAny` has no recursion limit. As a result, a body that is
valid JSON but nested past serde_json's 128-level limit now gets **422** `invalid_input` instead of
400. The key is named if the deep value sits inside the object; otherwise the error says "must be a
JSON object". This matches the D-03 rule that valid JSON of the wrong shape gives 422. However, the
old doc comment listed "nesting past the recursion limit" as malformed (400), so the doc comment
was updated to match. Unclosed deep nesting (`"[" * 200`, as used by `parse_rejects_deep_nesting`
and `insert_deeply_nested_is_400_json`) is still 400, because pass 1 reports EOF. The syntax pass is
iterative and uses heap memory, and actix's payload limit bounds that, so there is no stack-overflow
risk.

## Verification

- Every fix was checked after it was applied: the changed lines were re-read, then
  `cargo test --locked` was run on the affected module, followed by
  `cargo clippy --all-targets --locked -- -D warnings` and `cargo fmt --check`. Each fix was
  committed only after those passed.
- Full gate (`cargo build --locked && cargo test && cargo clippy --all-targets -- -D warnings &&
  cargo fmt --check`):
  - **Isolated worktree:** passed with all 3 commits applied. The worktree builds used the main
    checkout's `target/` (`CARGO_TARGET_DIR=/home/t3code/rs3/Projects/rs3/target`).
  - **Main checkout:** passed again after the fast-forward, so the result can be reproduced from the
    tree as it is now. Results: 87 passed, 0 failed, 2 ignored (79 passed before these fixes).
    clippy is clean and fmt is clean.
- No `#[allow(...)]` or `cfg_attr(..., allow(...))` attributes were added (grep is clean).
- The worktree was removed and the temporary branch `gsd-reviewfix/02-3481811` was deleted after
  `modernize-toolchain` was fast-forwarded to 3514062. The recovery sentinel was removed. The
  existing uncommitted files (`.planning/config.json`, `.planning/state.json`,
  `.planning/milestone.lock`) were left untouched.

---

_Fixed: 2026-10-20T19:30:00Z_
_Fixer: Claude (gsd-code-fixer)_
_Iteration: 1_
