---
phase: 02-http-api-port-error-handling
reviewed: 2026-10-20T18:40:33Z
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
  critical: 1
  warning: 2
  info: 4
  total: 7
status: issues_found
---

# Phase 2: Code Review Report

**Reviewed:** 2026-10-20T18:40:33Z
**Depth:** standard
**Files Reviewed:** 11
**Status:** issues_found

## Narrative Findings (AI reviewer)

## Summary

I reviewed the Phase 2 HTTP port (injected `AppState`, `configure(state)`, `ApiError`), the
`/insert` 400/422 codec split, the escaped `/list` codec, the store `insert_all` path and the
WR-01/WR-02 config-loader hardening. The gate is green: `cargo test` gives 79 passed and 2 ignored,
and `cargo clippy --all-targets -- -D warnings` is clean. The HTTP layer is in good shape. The
handlers have no `unwrap`, nothing is written before validation finishes, there is no global
store, and error bodies are real JSON objects.

The main defect is in the new config merge. `merge_dotenv_env` resolves precedence on
case-sensitive keys, but the `config` crate folds keys to lowercase later and resolves the
resulting collisions in random `HashMap` order. As a result, the documented rule that the
process environment overrides the dotenv file (D-14) does not hold whenever the two sources spell
a key in different case. I confirmed this empirically: over 200 runs, the dotenv value won 89
times and the env value won 111 times. The other findings are edge cases in the `server.*` key
filter and the 400/422 classification, plus minor quality notes.

Deliberate decisions in 02-CONTEXT.md are not reported as defects. These include the legacy
`" : "` spacing, actix's plain-text 413, no defaults and no `SERVER__HOST` alias, the raw dotenv
line in startup errors, the failure on a BOM-prefixed dotenv file, and PATCH answering 501.

## Critical Issues

### CR-01: Env-over-dotenv precedence is non-deterministic when key case differs

**File:** `src/config.rs:51-63` (consumed at `src/config.rs:98-104`)
**Issue:** `merge_dotenv_env` builds its map with the key spelling exactly as given:
`vars.entry(key).or_insert(value)` for dotenv entries and `vars.insert(key, value)` for env
entries. The module and function docs say keys are accepted "in any case". However,
`config::Environment::collect` (config-0.15.27 `src/env.rs`) then runs `key.to_lowercase()` on
every entry and calls `m.insert(key, …)`, iterating over the source map. With
`default-features = false`, that map is a `std::collections::HashMap`, so its iteration order is
random for each process. If `.env` contains `SERVER.HOST=file` and the operator exports
`server.host=env` (or the reverse spelling), both keys survive the merge, both fold to
`server.host`, and whichever one `config` visits last wins. A scratch crate pinned to
config 0.15.27 that fed it the exact map `merge_dotenv_env` produces for this input chose the
dotenv value 89 times and the env value 111 times out of 200. The same flaw breaks the documented
rule that the first dotenv occurrence wins when duplicates in the file differ in case. It also
makes the result random when the environment holds both `SERVER.HOST` and `server.host`.

The server can therefore bind to a different host on each restart, and an operator override
through the environment is silently ignored about half the time. The existing tests
`env_overrides_dotenv` (line 260) and `dotenv_first_occurrence_wins` (line 270) use identical
casing, so they cannot catch this. `bare_server_var_does_not_break_settings` repeats its check 32
times precisely because the authors knew `config` applies keys in `HashMap` order.

**Fix:** Normalise the key before precedence is decided, so the map can never hold two spellings
of the same config key. Then add a regression test that repeats the check.
```rust
let mut vars = ::config::Map::new();
if let Some(items) = dotenv {
    for item in items {
        let (key, value) = item.map_err(dotenv_error)?;
        vars.entry(key.to_ascii_lowercase()).or_insert(value);
    }
}
for (key, value) in env {
    if let (Ok(key), Ok(value)) = (key.into_string(), value.into_string()) {
        vars.insert(key.to_ascii_lowercase(), value);
    }
}
vars.retain(|key, _| key.starts_with("server."));

// test
#[test]
fn env_overrides_dotenv_across_case() {
    for _ in 0..64 {
        let s = Settings::from_sources(
            Ok(dotenvy::from_read_iter("SERVER.HOST=file\nSERVER.PORT=1\n".as_bytes())),
            env(&[("server.host", "env")]),
        )
        .expect("valid");
        assert_eq!(s.server.host, "env");
    }
}
```
If two env entries differ only in case, `vars_os()` order then decides the winner. That order is
deterministic for a given environment. Document it, or reject the conflict with a
`ConfigError::Message`. Update `merge_keeps_only_server_keys` (line 231) for the lowercased keys.

## Warnings

### WR-01: `server.` prefix filter still admits keys that collide with the server table

**File:** `src/config.rs:63`
**Issue:** D-16 / IN-07 added the filter so that stray variables "cannot collide with the server
table". The filter, `starts_with("server.")`, still admits any deeper or unrelated `server.*` key.
For example, `SERVER.HOST.X=y` next to `SERVER.HOST=h` turns `server.host` into a table, and
startup fails with `invalid type: map, expected a string for key server.host`. I confirmed this
with config 0.15.27. `SERVER.PORT.ANYTHING` behaves the same way. This is the same class of
order-dependent collision that IN-07 meant to remove, and the module doc already says the only
accepted names are `SERVER.HOST` / `SERVER.PORT`.
**Fix:** Allowlist the two exact keys (after the lowercasing from CR-01) instead of a prefix:
```rust
const KEYS: [&str; 2] = ["server.host", "server.port"];
vars.retain(|key, _| KEYS.contains(&key.as_str()));
```
Then add a test with `("SERVER.HOST.X", "y")` in the env that still configures `host = "h"`.

### WR-02: Valid JSON with an out-of-range number answers 400 `malformed_json` instead of 422

**File:** `src/store/legacy.rs:17-21`
**Issue:** The doc comment (lines 12-15) and D-03 say that valid JSON of the wrong shape,
including "an object with a non-string value", gives `InvalidInput` (422) naming the offending
key. Without `arbitrary_precision`, serde_json reports a number outside the `f64` range while
building the `Value`, and it classifies that error as `Category::Syntax`. Verified:
`{"a": 1e400}` gives `Syntax: number out of range at line 1 column 11`. The body is valid
RFC 8259 JSON, yet it gets 400 `malformed_json` and the message never names key `a`. The client
cannot tell a typo from a wrong value type.
**Fix:** Do the syntax check and the shape check separately. A first pass with
`serde::de::IgnoredAny` scans numbers without converting them, so only real syntax, EOF, or
recursion errors return 400. A failure in the second pass, which builds the `Value`, then means
an unrepresentable value and returns 422:
```rust
serde_json::from_str::<serde::de::IgnoredAny>(body)
    .map_err(|e| StoreError::MalformedJson(e.to_string()))?;
let value: Value = serde_json::from_str(body)
    .map_err(|e| StoreError::InvalidInput(e.to_string()))?;
```
If you keep the current behaviour, narrow the doc comment and D-03 wording, and add a test that
pins `1e400` to 400. Do not enable `arbitrary_precision`: feature unification would change
`serde_json::Value` for deno_core/serde_v8 as well.

## Info

### IN-01: `ApiError::Internal` is never constructed and the `internal` code is untested

**File:** `src/http/error.rs:16-17, 35, 47`
**Issue:** Nothing builds the `Internal` variant, so the `"internal"` / 500 mapping is dead code
with no test. Because the enum is `pub`, rustc does not warn about it.
**Fix:** Add a unit test that calls `ApiError::Internal.error_response()` and checks 500,
`application/json` and `"error":"internal"`. Alternatively, leave the variant out until Phase 3
needs it.

### IN-02: Startup banner prints the configured address, not the bound one

**File:** `src/main.rs:30`
**Issue:** The banner prints the configured `host` and `port`. With `SERVER.PORT=0` it reports
`port: 0` while the OS picked an ephemeral port. With a hostname that resolves to several
addresses, it does not show what was actually bound.
**Fix:** Print `server.addrs()` after `bind` succeeds, for example
`println!("Starting Http server at {:?}", server.addrs());`, or append the bound addresses to the
existing legacy text.

### IN-03: `/insert` accepts keys that can never be read or deleted over HTTP

**File:** `src/store/legacy.rs:25-33`, `src/http/kv.rs:37-52`
**Issue:** `{"":"v"}` is accepted with 200 and stored, but the route `/key/{key}` needs a
non-empty segment. Such a key is only ever visible in `/list`: GET and DELETE on it return
actix's default non-JSON 404, and it cannot be removed. The legacy code behaved the same way, but
the new validation layer is now the natural place to reject it.
**Fix:** Reject empty keys in `parse_insert_body` with `StoreError::InvalidInput("key must not be
empty")` (422), or document that empty keys are write-only through HTTP.

### IN-04: Non-string values are discarded silently when a later duplicate key is a string

**File:** `src/store/legacy.rs:17-33`
**Issue:** Validation runs on the `Value` after serde_json has already collapsed duplicate keys
(last one wins). `{"a":2,"a":"1"}` therefore returns 200 and stores `"1"`, while
`{"a":"1","a":2}` returns 422. This follows the last-wins rule pinned by
`insert_duplicate_key_last_value_wins`, but it is not obvious, and D-03 says "an object with any
non-string value → 422".
**Fix:** Document the last-wins rule in the `parse_insert_body` doc comment. To be strict
instead, deserialize into `Vec<(String, Value)>` through a custom map visitor so that every
occurrence is checked.

---

_Reviewed: 2026-10-20T18:40:33Z_
_Reviewer: Claude (gsd-code-reviewer)_
_Depth: standard_
