---
phase: 02-http-api-port-error-handling
fixed_at: 2026-10-20T18:54:57Z
review_path: .planning/phases/02-http-api-port-error-handling/02-REVIEW.md
iteration: 2
findings_in_scope: 1
fixed: 1
skipped: 0
status: all_fixed
---

# Phase 2: Code Review Fix Report

**Fixed at:** 2026-10-20T18:54:57Z
**Source review:** .planning/phases/02-http-api-port-error-handling/02-REVIEW.md
**Iteration:** 2

**Summary:**
- Findings in scope: 1 (fix scope `critical_warning`: WR-01)
- Fixed: 1
- Skipped: 0
- Also fixed, at the orchestrator's request because it sits next to WR-01: IN-04 (doc comment only,
  plus tests).
- Out of scope, not attempted: IN-01, IN-02, IN-03.

## Fixed Issues

### WR-01: Lone-surrogate escapes now answer 422 with false error messages (regression from the WR-02 fix)

**Files modified:** `src/store/legacy.rs`, `src/http/kv.rs`
**Commit:** fb014fb
**Status:** fixed: requires human verification (400/422 classification logic)
**Applied fix:** This follows the orchestrator decision (the recommended option). A `\uXXXX` escape
that encodes an unpaired UTF-16 surrogate now answers **400 `malformed_json`** again, the same as
before 3514062 and as D-03 states ("fails to parse → 400"). Genuinely unrepresentable but valid
values keep their **422**: out-of-range numbers and valid nesting past the recursion limit.

- Pass 2 (`serde_json::from_str::<Value>`) now checks its error first with the new
  `is_surrogate_escape_error` helper. If the error is a surrogate escape, it returns
  `MalformedJson("string contains an unpaired UTF-16 surrogate \u escape: <serde error>")`.
  Otherwise it goes to `unrepresentable` as before.
- **How the helper classifies.** serde_json exposes no error code, so the helper checks the message
  text (`contains("escape")`). Pass 1 has already accepted the grammar, so serde_json 1.0.154 can fail
  in pass 2 for only these reasons (checked in `src/read.rs` `parse_unicode_escape`):
  - `lone leading surrogate in hex escape` (surrogate)
  - `unexpected end of hex escape` (surrogate)
  - `number out of range`
  - `recursion limit exceeded`

  Only the two surrogate errors mention an escape. The helper's doc comment records this reasoning,
  and the tests below lock it in. If a serde_json upgrade changed the wording, those tests would
  fail.
- **The messages are now accurate.** The 400 message says what is actually wrong. With surrogate
  failures removed, `unrepresentable` only sees number or depth failures, and those always happen
  inside a value. So for an object it always names the key, and "must be a JSON object" only appears
  when the body really is not an object. Its doc comment now says this.
- **Documentation.** The `parse_insert_body` doc comment now has an explicit 400-vs-422
  classification list that includes the surrogate case for both keys and values. It also states that
  when there are several problems, the first one in body order decides.
- **Tests added** (`src/store/legacy.rs`):
  - `parse_unpaired_surrogate_in_value_is_malformed` covers:
    - `{"a":"\ud800"}` and `{"a":"\udc00"}`
    - `{"ok":"1","a":"x\udc00y"}`
    - `\ud800x`, `\ud800\u0041`, `\ud800\ud800`, `\ud800\n`, `\ud800\"`
  - `parse_unpaired_surrogate_in_key_is_malformed` covers `{"\ud800":"v"}`, `{"\udc00":"v"}` and
    `{"ok":"1","\ud800":"v"}`.
  - `parse_unpaired_surrogate_nested_or_whole_body_is_malformed`
  - `parse_paired_surrogate_escape_is_accepted`: `\ud83d\ude00` decodes to U+1F600 in both the key
    and the value.
  - `parse_unrepresentable_messages_never_mention_an_escape`: a guard so that 422 cases cannot be
    misclassified.
  - Each surrogate assertion checks for `MalformedJson`, checks that the message contains
    "unpaired UTF-16 surrogate", and checks that it contains neither "must be a string" nor
    "JSON object".
- **HTTP test added** (`src/http/kv.rs`): `insert_unpaired_surrogate_escape_is_400_json` sends
  `{"a":"\ud800"}`, `{"\ud800":"v"}` and `{"ok":"1","a":"x\udc00y"}`. For each it checks for 400
  `malformed_json`, `application/json`, and the same message wording. It also checks that the store
  is still empty afterwards.
- **Mutation check.** With the helper forced to return `false`, which is the old behaviour, all
  4 surrogate tests fail. With the fix in place they pass.

### IN-04: Doc claim "only that value is checked" is false for unrepresentable earlier duplicates

**Files modified:** `src/store/legacy.rs`
**Commit:** 6bf25e1
**Applied fix:** This is a documentation-only change. The sentence now says that only the last
value must be a string, and that an earlier occurrence which cannot be parsed or represented at all
(`{"a":1e400,"a":"1"}`) still rejects the whole body. Two tests lock this in:
- `parse_duplicate_key_checks_only_last_value_type`: `{"a":1,"a":"2"}` gives `[("a","2")]`, and
  `{"a":"1","a":2}` gives 422.
- `parse_duplicate_key_unrepresentable_earlier_value_is_rejected`: `{"a":1e400,"a":"1"}` gives 422
  naming `key a`, and `{"a":"\ud800","a":"1"}` gives 400.

## Verification

- After each fix I re-read the changed lines, then ran `cargo test --locked`,
  `cargo clippy --all-targets --locked -- -D warnings` and `cargo fmt --check`. Each fix was
  committed only after all three passed.
- **Escape bytes checked.** While editing, the Edit tool silently turned the `\u0041` escape into a
  literal `A`, and the `\ud83d\ude00` escape pair into a literal emoji. Both were restored
  byte-exactly before committing. A grep for non-ASCII bytes over the added lines comes back clean,
  so the committed tests contain the real JSON escape sequences.
- Full gate (`cargo build --locked && cargo test && cargo clippy --all-targets -- -D warnings &&
  cargo fmt --check`):
  - **Isolated worktree:** passed with both commits applied. Builds used the main checkout's
    `target/` (`CARGO_TARGET_DIR=/home/t3code/rs3/Projects/rs3/target`).
  - **Main checkout:** passed again after the fast-forward, so you can reproduce it from the current
    tree. Results: 95 passed, 0 failed, 2 ignored (87 passed before these fixes). clippy is clean
    and fmt is clean.
- No `#[allow(...)]` or `cfg_attr(..., allow(...))` attributes were added (grep is clean).
- Cleanup:
  - `modernize-toolchain` was fast-forwarded to 6bf25e1.
  - The worktree was removed and the temporary branch `gsd-reviewfix/02-3490180` was deleted.
  - The recovery sentinel was removed.
  - The existing uncommitted files (`.planning/config.json`, `.planning/state.json`,
    `.planning/milestone.lock`, `*.iter2.md` backups) were left untouched.

## Previous Iterations

### Iteration 1 (2026-10-20, backed up at `02-REVIEW-FIX.iter2.md`)

Fix scope `critical_warning`: 3 in scope, 3 fixed, 0 skipped (`all_fixed`). The 02-REVIEW.md
re-review for iteration 2 confirmed all three as fixed.

| Finding | Title | Commit | Summary |
|---------|-------|--------|---------|
| CR-01 | Env-over-dotenv precedence is non-deterministic when key case differs | 49504bc | `merge_dotenv_env` lowercases keys before deciding precedence, so env always beats dotenv whatever the key case (requires human verification: precedence logic) |
| WR-01 | `server.` prefix filter still admits keys that collide with the server table | bd2137e | Exact allowlist `["server.host", "server.port"]`, applied after lowercasing |
| WR-02 | Valid JSON with an out-of-range number answers 400 `malformed_json` instead of 422 | 3514062 | Two-pass `parse_insert_body` (`IgnoredAny` syntax pass, then `Value` pass, then a `FindBadKey` walk) answers 422 naming the key for out-of-range numbers and valid deep nesting (requires human verification: 400/422 classification). Iteration 2's WR-01 fixes the lone-surrogate regression it introduced. |

At the end of iteration 1 the main checkout gave 87 passed and 2 ignored, with clippy and fmt
clean. See the backup for the full per-finding detail.

---

_Fixed: 2026-10-20T18:54:57Z_
_Fixer: Claude (gsd-code-fixer)_
_Iteration: 2_
