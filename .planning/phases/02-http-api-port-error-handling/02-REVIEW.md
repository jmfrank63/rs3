---
phase: 02-http-api-port-error-handling
reviewed: 2026-10-20T18:58:01Z
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
  warning: 0
  info: 5
  total: 5
status: issues_found
---

# Phase 2: Code Review Report

**Reviewed:** 2026-10-20T18:58:01Z
**Depth:** standard
**Files Reviewed:** 11
**Status:** issues_found (info only)

## Narrative Findings (AI reviewer)

## Summary

This is the `--auto` re-review after fix iteration 2:
- fb014fb: unpaired surrogate escapes answer 400.
- 6bf25e1: the duplicate-key doc comment is corrected.

Since the iteration-3 review, only `src/store/legacy.rs` and `src/http/kv.rs` have changed
(`git diff 3514062 HEAD`). I re-read all 11 files anyway.

**Gate on the current tree:**
- `cargo test --locked`: 95 passed, 0 failed, 2 ignored.
- `cargo clippy --all-targets --locked -- -D warnings`: clean.
- `cargo fmt --check`: clean.
- No `allow(` attributes in `src/` or `Cargo.toml`.
- The new tests contain real ASCII `\uXXXX` escape sequences. The only non-ASCII bytes are in
  older, intentional tests (`kv.rs:485-491`, `legacy.rs:221`).

**Prior findings:**
- **WR-01 (lone-surrogate 422 with false messages) is fixed.** I checked the classifier against the
  serde_json 1.0.154 source:
  - Pass 1 (`IgnoredAny` → `Deserializer::ignore_value`, `de.rs:1102`) checks the full grammar.
    That covers idents, numbers (`ignore_integer`/`ignore_decimal`/`ignore_exponent`), string
    control characters, escape letters, hex digits, trailing commas and trailing characters. It
    uses a heap `scratch` stack, so a 256 KiB `[[[[…` body cannot overflow the stack in pass 1.
  - It does not check surrogate pairing (`ignore_escape`, `read.rs:1076`), check numeric range, or
    limit depth.
  - Pass 2 can therefore fail for only four reasons:
    - `LoneLeadingSurrogateInHexEscape` and `UnexpectedEndOfHexEscape` (`parse_unicode_escape`,
      `read.rs:951-1012`).
    - `NumberOutOfRange`.
    - `RecursionLimitExceeded`.
  - `InvalidUnicodeCodePoint` (`as_str`, `read.rs:903`) cannot be reached from `from_str` with
    `validate = true`.
  - Of the error `Display` strings (`error.rs:364-383`), only the two surrogate codes contain
    "escape". The `" at line N column M"` suffix never echoes input. So
    `is_surrogate_escape_error` is correct for the locked version.
  - Scratch run of the exact code:
    - `{"a":1,"b":"\ud800"}` → 400.
    - `{"b":"\ud800","a":1e400}` → 400.
    - `{"a":1e400,"b":"\ud800"}` → 422 naming `a`.
    - `{"a":{"b":1e400}}` → 422 naming `a`.
- **IN-04 (duplicate-key doc) is fixed.** The sentence now matches the behaviour, and two tests pin
  it.

**New in this pass:**
- The new "first problem in body order decides" sentence is not accurate for the mixed
  non-string plus unrepresentable case (IN-04 below).
- Two test names say that deep nesting is a 400, which contradicts the 422 decision for valid deep
  nesting (IN-05).

Both are documentation or naming issues. Neither changes a status code.

**Locked decisions:** nothing in 02-CONTEXT.md D-01..D-18 is reported. The orchestrator decisions
are also respected:
- 422 for valid over-deep nesting and for out-of-range numbers.
- 400 for invalid surrogate escapes and for syntax errors.

## Info

### IN-01: `ApiError::Internal` is never constructed and the `internal` code is untested (carried over)

**File:** `src/http/error.rs:16-17, 35, 47`

**Issue:** Still open. No code path builds `ApiError::Internal`, and no test checks the 500 /
`"error":"internal"` mapping. The enum is `pub`, so rustc does not report the dead variant.

**Fix:** Add a unit test that asserts
`ApiError::Internal.error_response().status() == 500` and checks the body code `"internal"`. Or
remove the variant until Phase 3 constructs it.

### IN-02: Startup banner prints the configured address, not the bound one (carried over)

**File:** `src/main.rs:30`

**Issue:** Still open. With `SERVER.PORT=0` the banner says `port: 0`, while the OS chose an
ephemeral port.

**Fix:** After a successful `bind`, print `server.addrs()`. For example, append
`(bound: {:?})` with `server.addrs()`.

### IN-03: `/insert` accepts keys that can never be read or deleted over HTTP (carried over)

**File:** `src/store/legacy.rs:58-66`, `src/http/kv.rs:37-52`

**Issue:** Still open, and confirmed in the scratch run: `parse_insert_body(r#"{"":"v"}"#)` returns
`Ok([("", "v")])`. `/key/{key}` needs a non-empty segment, so the entry only shows up in `/list` and
cannot be removed.

**Fix:** In the `object.iter().map(...)` step, reject an empty key with
`StoreError::InvalidInput("key must not be empty")` (422). Or document that empty keys are
write-only over HTTP.

### IN-04: Doc claim "the first one in body order decides" is false when a non-string value precedes an unrepresentable one

**File:** `src/store/legacy.rs:29-30` (behaviour at `src/store/legacy.rs:46-66`)

**Issue:** The doc comment added in fb014fb says: "When valid JSON grammar has more than one such
problem, the first one in body order decides." Pass 2 builds the whole `Value` before the
non-string check at lines 58-66 runs. So an unrepresentable value anywhere in the body beats an
earlier non-string value. Verified with the exact code against serde_json 1.0.154:

| Body | Result |
|------|--------|
| `{"a":1,"b":1e400}` | `InvalidInput("value for key b must be a string: number out of range …")`. The first problem in body order is `a`. |

The status is still 422 and nothing is written, so the contract holds. Only the documented
tie-break and the named key are wrong. A client that fixes only `b` gets a second 422 for `a`.
The fix report repeats the claim.

**Fix:** Narrow the sentence to match the two-pass order:
```rust
/// When a body has several problems, a string that cannot be decoded or a value
/// that cannot be represented is reported first (the first such one in body
/// order); only if there is none is the first non-string value reported.
```
Optionally pin it with `assert!(matches!(parse_insert_body(r#"{"a":1,"b":1e400}"#),
Err(StoreError::InvalidInput(m)) if m.contains("key b")))`.

### IN-05: Tests named "deep nesting is 400" exercise a truncated body, which contradicts the 422 decision for valid deep nesting

**File:** `src/store/legacy.rs:272-275` (`parse_rejects_deep_nesting`), `src/http/kv.rs:289-292`
(`insert_deeply_nested_is_400_json`)

**Issue:** Both tests send `"[".repeat(200)` with no closing brackets. They pass only because pass 1
reports `EOF while parsing a list`. Depth has nothing to do with it. The orchestrator decision and
`parse_valid_deep_nesting_is_invalid_input` (lines 293-301) say that valid deep nesting answers
**422**. So these names state the opposite of the contract, and a reader could "fix" the 422 path
to match them. They are not a regression guard for depth either: a 200-deep truncated body and a
2-deep truncated body take the same code path.

**Fix:** Rename both tests to describe the real case, for example
`parse_rejects_truncated_deep_nesting` and `insert_truncated_deep_nesting_is_400_json`. Optionally,
add an HTTP-level `insert_valid_deep_nesting_is_422_json`
(`format!("{}{}", "[".repeat(200), "]".repeat(200))` → 422 `invalid_input`, store empty) next to
the existing unit test.

---

_Reviewed: 2026-10-20T18:58:01Z_
_Reviewer: Claude (gsd-code-reviewer)_
_Depth: standard_
