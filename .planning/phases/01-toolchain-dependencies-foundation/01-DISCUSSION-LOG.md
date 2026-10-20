# Phase 1: Toolchain, Dependencies & Foundation - Discussion Log

> **Audit trail only.** Do not use as input to planning, research, or execution agents.
> Decisions are captured in CONTEXT.md — this log preserves the alternatives considered.

**Date:** 2026-10-20
**Phase:** 1-Toolchain, Dependencies & Foundation
**Areas discussed:** Store API & list ordering, Phase-1 store access bridge, JS module in Phase 1, Config shape & failure behaviour, Legacy tests during the transition, Lint policy, Commit sequencing

Mode: `--auto` — every choice is the recommended default, auto-selected without user prompts.

---

## Store API & list ordering

| Option | Description | Selected |
|--------|-------------|----------|
| Concrete `Store` newtype over papaya, owned values, sorted `list()` | Narrow API, extract trait later (research decision h) | ✓ |
| `Arc<dyn StoreBackend>` trait now | Ready for git backend, but premature | |
| dashmap with guards | Deadlock risk with re-entrant JS | |

**User's choice:** [auto] recommended default
**Notes:** Sorted list gives deterministic `/list` output.

## Phase-1 store access bridge

| Option | Description | Selected |
|--------|-------------|----------|
| Temporary `LazyLock<Store>` in `http`, removed in Phase 2 | Smallest change; ARCH-02 stays in Phase 2 | ✓ |
| Inject `web::Data` already in Phase 1 | Pulls Phase 2 work forward | |

**User's choice:** [auto] recommended default

## JS module in Phase 1

| Option | Description | Selected |
|--------|-------------|----------|
| Add `deno_core` now with a stub `js` module; PATCH → 501 | V8 link + single-V8 check happen early | ✓ |
| Leave deno_core out until Phase 3 | Faster Phase 1 builds, but DEPS-02 unverifiable | |

**User's choice:** [auto] recommended default

## Config shape & failure behaviour

| Option | Description | Selected |
|--------|-------------|----------|
| `Settings` via config 0.15 builder + dotenvy, no defaults, in-code test constructor, `main` exits non-zero on error | Keeps dotted env names; no `set_var` in tests | ✓ |
| Add defaults + `SERVER__HOST` alias now | Scope creep (v2 OPS-02) | |

**User's choice:** [auto] recommended default

## Legacy tests during the transition

| Option | Description | Selected |
|--------|-------------|----------|
| Port to actix 4.17 in `http` tests; PATCH tests `#[ignore]` until Phase 3 | Keeps coverage, `cargo test` green | ✓ |
| Delete PATCH tests and re-add later | Loses tracking | |

**User's choice:** [auto] recommended default

## Lint policy

| Option | Description | Selected |
|--------|-------------|----------|
| `unsafe_code = "forbid"`; defer `clippy::unwrap_used` to Phase 2/3 | Gate passes now; tighten once unwraps removed | ✓ |
| `unwrap_used = "deny"` now | Forces Phase 2 work into Phase 1 | |

**User's choice:** [auto] recommended default

## Commit sequencing

| Option | Description | Selected |
|--------|-------------|----------|
| Toolchain/manifest → restructure → store/config+tests → standalone fmt commit | Reviewable history | ✓ |

**User's choice:** [auto] recommended default

## Claude's Discretion

- Module file layout, helper naming, error wording, whether `init_platform` is called in Phase 1.

## Deferred Ideas

- Config defaults / `SERVER__HOST` alias (v2 OPS-02)
- `clippy::unwrap_used` (Phase 2/3)
- `StoreBackend` trait (FUT-01)
