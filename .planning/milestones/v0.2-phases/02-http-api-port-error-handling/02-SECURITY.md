---
phase: "02"
slug: "http-api-port-error-handling"
status: verified
# threats_open = count of OPEN threats at or above workflow.security_block_on severity (the blocking gate)
threats_open: 0
asvs_level: 1
created: "2026-10-20"
---

# Phase 02 — Security

> Per-phase security contract: threat register, accepted risks, and audit trail.
> Register authored at plan time (02-01/02/03 `<threat_model>`). Verified at ASVS L1 (grep plus automated test-gate depth) against tree `1838bf1`. Short-circuit rule: threats_open 0, register authored at plan time, ASVS 1, so no auditor was spawned.

---

## Trust Boundaries

| Boundary | Description | Data Crossing |
|----------|-------------|---------------|
| HTTP client → actix handlers | Any client reaches GET/DELETE/PATCH `/key/{key}`, POST `/insert`, GET `/list`, GET `/ip`; path segments and raw bodies (any content type, ≤ 256 KiB) are untrusted | Untrusted user input |
| stored values → GET `/list` consumers | Values written by one client are rendered into text other clients parse | Untrusted user content |
| main → worker threads | One `AppState` (Store + `Arc<Settings>`) created on the main thread, shared via `web::Data` | In-process state |
| dotenv file / process environment → config loader | File parsed once at startup; only `server.*` keys are intended config | Operator config (low sensitivity) |
| test harness → process state | Parallel libtest threads share one process; isolation via per-test `AppState` | Test-only |
| developer → lint gate | TOOL-03 / D-13 gates must not be greened by suppression | Build integrity |

---

## Threat Register

| Threat ID | Category | Component | Severity | Disposition | Mitigation | Status |
|-----------|----------|-----------|----------|-------------|------------|--------|
| T-02-01 | Denial of Service | `src/http/kv.rs` get/delete/insert, `src/http/mod.rs` patch_key | high | mitigate | Handlers return `Result<HttpResponse, ApiError>` (kv.rs:27,41,50; mod.rs:48). Zero `.unwrap()`/`.expect()` before `#[cfg(test)]` in any src file. `clippy::unwrap_used`/`expect_used = "deny"` | closed |
| T-02-02 | Tampering | test isolation (shared global store) | medium | mitigate | No `static … Store` / `LazyLock` in `src/`; per-test `test_state()`; suite green under `env -i`, `--test-threads=1` and 5× repeat | closed |
| T-02-03 | Information Disclosure | `ApiError` not_found message | low | accept | Echoes only the caller's own key, serde-escaped; no internals in error bodies | closed |
| T-02-04 | Information Disclosure | GET `/ip` | low | accept | Required legacy behaviour (HTTP-02); exposes only the operator-chosen host | closed |
| T-02-05 | Tampering | runtime config re-reads | medium | mitigate | `Settings` loaded once in `main`, shared as `Arc<Settings>` (state.rs:15); no `Settings::load`/`env::var`/`vars_os`/`dotenvy` in `src/http/` or `src/state.rs` | closed |
| T-02-06 | Denial of Service | `web::Data<AppState>` extraction | low | accept | `configure(state)` always registers it; the text/plain 500 path is reachable only in `test_ip_without_settings_is_500` | closed |
| T-02-07 | Tampering | `Settings::load` env-mutation race (UB, edition 2024) | high | mitigate | `load()` = `from_sources(dotenvy::dotenv_iter(), std::env::vars_os())` (config.rs:75); no `set_var`/`remove_var`/`dotenvy::dotenv()` anywhere in `src/` | closed |
| T-02-08 | Tampering | half-loaded malformed dotenv file | medium | mitigate | Merge returns `Err` before any pair is applied; `malformed_dotenv_is_error_with_env_prefix`, `unreadable_dotenv_is_error` | closed |
| T-02-09 | Tampering | unrelated env var colliding with `server` table | medium | mitigate | `server.` allow-list (config.rs:63); `merge_keeps_only_server_keys`, `bare_server_var_does_not_break_settings` | closed |
| T-02-10 | Information Disclosure | dotenv parse error text on stderr | low | accept | Raw failing line aids diagnosis (orchestrator-resolved); file holds only host/port | closed |
| T-02-11 | Denial of Service | non-UTF-8 environment entry | medium | mitigate | `vars_os()` + `into_string()` skip (config.rs:59); `non_utf8_env_entries_are_skipped` | closed |
| T-02-12 | Tampering | `/list` entry forgery via unescaped value | high | mitigate | serde_json escaping in `format_list`; `format_list_cannot_forge_entries`, `list_escapes_quotes_and_backslashes`, `format_list_round_trips` | closed |
| T-02-13 | Tampering | partial insert from invalid batch | medium | mitigate | Full validation before infallible `insert_all`; `insert_non_string_value_is_422_and_inserts_nothing` | closed |
| T-02-14 | Denial of Service | POST `/insert` payload size | medium | mitigate | actix default 262 144-byte `PayloadConfig` on the `Bytes` extractor; `insert_over_256_kib_is_413` | closed |
| T-02-15 | Denial of Service | deeply nested JSON | medium | mitigate | serde_json recursion limit → 400 `malformed_json`; `parse_rejects_deep_nesting`, `insert_deeply_nested_is_400_json` | closed |
| T-02-16 | Denial of Service | invalid UTF-8 / unexpected content type | medium | mitigate | `web::Bytes` + `std::str::from_utf8` → JSON 400 (kv.rs:26-28); `insert_invalid_utf8_is_400_json`, `insert_with_bogus_charset_is_ok` | closed |
| T-02-17 | Repudiation | lint gate integrity (D-13) | medium | mitigate | `[lints.clippy]` denies unwrap/expect; `clippy.toml` exempts tests only; no allow/cfg_attr-allow attributes in `src/`; no `.cargo/` config; exactly 2 `#[ignore]` (parked PATCH tests) | closed |
| T-02-SC | Tampering | cargo dependency installs (phase-wide) | high | mitigate | No new packages; 0 Phase 2 commits touch `Cargo.lock`; `Cargo.toml` diff is +4 lines (`[lints.clippy]` only); builds use `--locked` | closed |

*Status: open · closed · open — below high threshold (non-blocking)*
*Severity: critical > high > medium > low — only open threats at or above workflow.security_block_on count toward threats_open*
*Disposition: mitigate (implementation required) · accept (documented risk) · transfer (third-party)*

---

## Accepted Risks Log

| Risk ID | Threat Ref | Rationale | Accepted By | Date |
|---------|------------|-----------|-------------|------|
| AR-02-01 | T-02-03 | not_found message echoes the caller's own key, JSON-escaped; no structure injection, no internals | plan-time threat model (02-01), autonomous run | 2026-10-20 |
| AR-02-02 | T-02-04 | `/ip` returning the configured host is the required legacy behaviour (HTTP-02) | plan-time threat model (02-01), autonomous run | 2026-10-20 |
| AR-02-03 | T-02-06 | missing `web::Data` 500 path only reachable in a deliberate test; `configure(state)` always registers state | plan-time threat model (02-01), autonomous run | 2026-10-20 |
| AR-02-04 | T-02-10 | raw dotenv line in the startup error helps diagnosis; dotenv file holds only host/port. Revisit if secrets move into it | orchestrator-resolved research Open Question 2 (Recommended), autonomous run | 2026-10-20 |

*Accepted risks do not resurface in future audit runs.*

---

## Security Audit Trail

| Audit Date | Threats Total | Closed | Open | Run By |
|------------|---------------|--------|------|--------|
| 2026-10-20 | 18 | 18 | 0 | gsd-secure-phase (orchestrator, L1 grep + test-gate short-circuit) |

---

## Sign-Off

- [x] All threats have a disposition (mitigate / accept / transfer)
- [x] Accepted risks documented in Accepted Risks Log
- [x] `threats_open: 0` confirmed
- [x] `status: verified` set in frontmatter

**Approval:** verified 2026-10-20
