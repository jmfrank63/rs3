---
phase: 02-http-api-port-error-handling
reviewed: 2026-10-20T18:49:52Z
depth: standard
files_reviewed: 11
files_reviewed_list:
  - Cargo.toml
  - clippy.toml
  - src/config.rs
  - src/http/error.rs
  - src/http/kv.rs
  - src/http/mod.rs
  - src/lib.rs
  - src/main.rs
  - src/state.rs
  - src/store/legacy.rs
  - src/store/mod.rs
findings:
  critical: 0
  warning: 1
  info: 4
  total: 5
status: issues_found
---

# Phase 2: Code Review Report

**Reviewed:** 2026-10-20T18:49:52Z
**Depth:** standard
**Files Reviewed:** 11
**Status:** issues_found

## Narrative Findings (AI reviewer)

## Summary

This is the `--auto` re-review after fix iteration 1 (49504bc CR-01, bd2137e WR-01, 3514062 WR-02).
The gate is green on the current tree. `cargo test --locked` gives 87 passed and 2 ignored.
`cargo clippy --all-targets -- -D warnings` and `cargo fmt --check` are both clean.

**Prior findings:**
- **CR-01 is fixed.** `merge_dotenv_env` lowercases each key before it decides precedence, so the
  map can no longer hold two spellings of one key. I also checked that ASCII lowercasing cannot
  miss a key that `config`'s Unicode `to_lowercase` would fold onto `server.host` or
  `server.port`. The only non-ASCII character that lowercases to an ASCII letter is U+212A KELVIN
  SIGN, which becomes `k`, and neither key contains `k`. Because the allowlist runs before
  `config` sees the map, no such collision can reach it.
- **WR-01 is fixed.** The filter is now an exact allowlist.
- **WR-02 is fixed** for the cases it targeted: out-of-range numbers and valid nesting past the
  recursion limit. Per the orchestrator decision, the 422 for nesting past the recursion limit is
  not a defect.

**New finding:** the WR-02 two-pass classifier lets a third class of input through pass 1:
`\uXXXX` escapes that encode lone UTF-16 surrogates. serde_json's `ignore_escape`
(serde_json-1.0.154 `src/read.rs:1085`) deliberately skips the surrogate check. Those bodies used
to answer 400. They now answer 422, and the message is false: it says "must be a string" about a
value that is a string, and "must be a JSON object" about a body that is an object. No test covers
this.

**Locked decisions:** nothing in 02-CONTEXT.md D-01..D-18 is reported as a defect.

## Warnings

### WR-01: Lone-surrogate escapes now answer 422 with false error messages (regression from the WR-02 fix)

**File:** `src/store/legacy.rs:24-30, 49-59, 81-88` (doc comment at `src/store/legacy.rs:15-22`)

**Issue:** Pass 1, `serde_json::from_str::<IgnoredAny>`, accepts escapes such as `"\ud800"` and
`"x\udc00y"`. serde_json's `ignore_escape` reads the four hex digits and states in its source:
"At this point we don't care if the codepoint is valid". Pass 2 then fails to build the `String`
(`LoneLeadingSurrogateInHexEscape` / "unexpected end of hex escape"), and `unrepresentable`
produces the messages below. I ran the exact `parse_insert_body` code against serde_json 1.0.154
with `preserve_order` in a scratch crate and got:

| Body | Result |
|------|--------|
| `{"a":"\ud800"}` | `InvalidInput("value for key a must be a string: unexpected end of hex escape at line 1 column 13")`. The value **is** a string. |
| `{"ok":"1","a":"x\udc00y"}` | `InvalidInput("value for key a must be a string: lone leading surrogate in hex escape ...")` |
| `{"\ud800":"v"}` | `InvalidInput("insert body must be a JSON object: ...")`. The body **is** an object. The key fails inside `map.next_key::<String>()` while the slot is still `None`, so the code falls through to the "not an object" message. |
| `{"ok":"1","\ud800":"v"}` | The same false "must be a JSON object" message. The slot was reset to `None` after key `ok`. |

Before 3514062, all of these were 400 `malformed_json`, because the single `from_str::<Value>` pass
classified them as `Category::Syntax`, which matches the literal D-03 rule. The fix report and the
doc comment list only out-of-range numbers and recursion depth as "unrepresentable", so this
reclassification was never intended. No test pins it.

You can argue that 422 is the right status, because RFC 8259 §8.2 grammar permits lone surrogates.
Even so, the client is told something untrue. It cannot fix a "must be a string" error on a value
that is already a string, and it is told its object is not an object. That breaks the D-03
contract that the 422 message names the actual problem.

**Fix:** Have `FindBadKey` record whether the failure happened while reading a key or a value.
Word the messages around "cannot be represented" rather than a shape claim. Only say "not a JSON
object" when the body does not start with `{`. Then document the surrogate case and pin it with
tests.
```rust
enum Bad { None, Key(Option<String>), Value(String) }

// in visit_map
let mut last = None;
loop {
    *self.0 = Bad::Key(last.take());          // failure here = an undecodable key (after `last`)
    let Some(key) = map.next_key::<String>()? else { break };
    *self.0 = Bad::Value(key.clone());        // failure here = this key's value
    map.next_value::<Value>()?;
    last = Some(key);
}
*self.0 = Bad::None;

// in unrepresentable
match bad {
    Bad::Value(k) => format!("value for key {k} cannot be represented: {error}"),
    Bad::Key(_)   => format!("an object key cannot be represented: {error}"),
    Bad::None if !body.trim_start().starts_with('{') =>
        format!("insert body must be a JSON object: {error}"),
    Bad::None     => format!("insert body cannot be represented: {error}"),
}
```
Add unit and HTTP tests for `{"a":"\ud800"}` (422, message names `key a`, no "must be a string")
and `{"\ud800":"v"}` (422, message does not say "not a JSON object"). Alternatively, keep the
pre-fix 400 for surrogate errors: classify a pass-2 failure whose message contains "surrogate" or
"hex escape" as `MalformedJson`. Choose one and record it in the doc comment.

## Info

### IN-01: `ApiError::Internal` is never constructed and the `internal` code is untested (carried over)

**File:** `src/http/error.rs:16-17, 35, 47`

**Issue:** Still open. No code path builds `Internal`, and no test checks the 500 /
`"error":"internal"` mapping. Because the enum is `pub`, rustc does not warn about it.

**Fix:** Add a unit test on `ApiError::Internal.error_response()`, or remove the variant until
Phase 3 needs it.

### IN-02: Startup banner prints the configured address, not the bound one (carried over)

**File:** `src/main.rs:30`

**Issue:** Still open. With `SERVER.PORT=0` the banner says `port: 0`, while the OS picked an
ephemeral port.

**Fix:** After a successful `bind`, print or append `server.addrs()`.

### IN-03: `/insert` accepts keys that can never be read or deleted over HTTP (carried over)

**File:** `src/store/legacy.rs:37-45`, `src/http/kv.rs:37-52`

**Issue:** Still open. `{"":"v"}` is stored with 200, but `/key/{key}` needs a non-empty segment.
The entry can only be seen through `/list` and cannot be deleted.

**Fix:** Reject empty keys in `parse_insert_body` with `InvalidInput` (422), or document that empty
keys are write-only over HTTP.

### IN-04: Doc claim "only that value is checked" is false for unrepresentable earlier duplicates

**File:** `src/store/legacy.rs:21-22`

**Issue:** The previous IN-04 is resolved: the last-wins rule is now documented. However, the new
sentence "With duplicate keys the last occurrence wins ..., so only that value is checked" is not
accurate after WR-02. `{"a":1e400,"a":"1"}` answers 422 `value for key a must be a string: number
out of range`. Pass 2 fails on the first occurrence before last-wins applies. I verified this in
the scratch run. Rejecting the body is reasonable, but the doc promises otherwise.

**Fix:** Narrow the sentence. For example: "only the last value's type is checked; any occurrence
that cannot be represented at all is still rejected".

---

_Reviewed: 2026-10-20T18:49:52Z_
_Reviewer: Claude (gsd-code-reviewer)_
_Depth: standard_
