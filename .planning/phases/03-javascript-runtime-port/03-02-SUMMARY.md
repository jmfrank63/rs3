---
phase: 03-javascript-runtime-port
plan: 02
subsystem: js-runtime
tags: [deno_core, javascript, migration, docs, samples]

requires:
  - phase: 03-javascript-runtime-port
    provides: "03-01 as-built op semantics (five #[op2] ops, StrArg coercion, Error/TypeError mapping, op_panic disabled, nested Uncaught stripping)"
provides:
  - "requests/{fac,sum,bin,store}.js migrated to Deno.core.ops.op_rs3_get / op_rs3_insert (D-14 fixtures for 03-03)"
  - "requests/{fac,sum,bin,store}.txt JSON-escaped /insert payloads migrated, still valid JSON with one key each"
  - "Both REST-client walkthroughs post the code payload Deno.core.ops.op_rs3_list().toString();"
  - "src/example.js lists through Deno.core.ops.op_rs3_list() and prints with Deno.core.print"
  - "docs/js-ops.md: JSRT-06 old-to-new op mapping, error rules, PATCH results, runtime model, arguments and encoding, sandbox and Phase 5 limits"
affects: [03-03 PATCH fixture tests (include_str! of the migrated samples, doc greps), phase 6 README (links docs/js-ops.md)]

actuals:
  tokens: 3987
  tasks: 3
  commits: 3
plan_head_before: 53b23d52ab23caf985ffa82d0b7318a7232fb23b
plan_head_after: f125e06a4399a91dc4aebb1aa51e18c0504d3c7f

tech-stack:
  added: []
  patterns:
    - "JS artefacts are checked with node vm.runInNewContext against a stub Deno.core.ops (syntax, op names, completion values) before the real runtime runs them in 03-03"
    - "Each .txt payload is the JSON-escaped one-line form of its sibling .js file and is checked to complete with the same value"

key-files:
  created: [docs/js-ops.md]
  modified: [requests/fac.js, requests/fac.txt, requests/sum.js, requests/sum.txt, requests/bin.js, requests/bin.txt, requests/store.js, requests/store.txt, requests/rs3_http_localhost_5000.http, requests/rs3_https_rs3_cwe.http, src/example.js]

key-decisions:
  - "docs/js-ops.md documents the as-built 03-01 StrArg rule: primitives are coerced like String(..), objects and symbols throw TypeError: op argument must be a string or another primitive value, and the object's toString is never run; inserting an object needs JSON.stringify"
  - "The doc names the plain built-in classes (Error for missing keys, TypeError for bad insert input) and explains why no NotFound class exists"

patterns-established:
  - "Migration is a two-edit change per artefact: delete the bootstrap statement and rename the call; everything else stays byte-identical"

requirements-completed: [JSRT-05, JSRT-06]

coverage:
  - id: D1
    description: "fac, sum and bin samples (standalone .js and JSON payload .txt) use Deno.core.ops.op_rs3_get and still compute 120, 10 and the 5/3/8 tree JSON"
    requirement: JSRT-05
    verification:
      - kind: other
        ref: "Task 1 and Task 2 node vm stub checks (03-02-PLAN.md verify blocks)"
        status: pass
    human_judgment: false
  - id: D2
    description: "store sample (both forms) writes res=1111 via op_rs3_insert; both REST-client code payloads evaluate to the list text; example.js prints [] plus newline via Deno.core.print"
    requirement: JSRT-05
    verification:
      - kind: other
        ref: "Task 3 node vm stub check (03-02-PLAN.md verify block 1)"
        status: pass
    human_judgment: false
  - id: D3
    description: "No legacy op syntax (opSync, bootstrap call, unprefixed rs3_ op names) remains in requests/ or src/; every .txt payload is valid JSON"
    requirement: JSRT-05
    verification:
      - kind: other
        ref: "Task 3 grep gate (03-02-PLAN.md verify block 2)"
        status: pass
    human_judgment: false
  - id: D4
    description: "docs/js-ops.md documents the old-to-new mapping, error rules, PATCH results, runtime model, encoding and the known Phase 5 limits"
    requirement: JSRT-06
    verification:
      - kind: other
        ref: "Task 1 docs gate (03-02-PLAN.md verify block 2)"
        status: pass
    human_judgment: true
    rationale: "The gate proves the required statements are present; whether the guide reads clearly for a script author is a human judgement"
  - id: D5
    description: "No Rust, Cargo or clippy.toml change; TOOL-03 still green"
    verification:
      - kind: other
        ref: "cargo build --locked && cargo test && cargo clippy --all-targets -- -D warnings && cargo fmt --check; ownership guard"
        status: pass
    human_judgment: false

duration: 3min
completed: 2026-10-20
status: complete
---

# Phase 3 Plan 02: Samples and Migration Guide Summary

**Every rs3 JS artefact (four samples in .js and JSON-payload form, both REST-client walkthroughs, src/example.js) now calls Deno.core.ops.op_rs3_* with the legacy bootstrap call removed and unchanged results. docs/js-ops.md gives the full old-to-new op table plus the error, PATCH-result, runtime-model and Phase 5 limit rules as built in 03-01.**

## Performance

- **Duration:** 3 min
- **Started:** 2026-10-20T20:13:57Z
- **Completed:** 2026-10-20T20:16:30Z
- **Tasks:** 3
- **Files modified:** 12 (1 created, 11 modified)

## Accomplishments

- Migrated all twelve artefacts. Node stub checks pass:
  - fac gives `120` in both forms, sum gives `10`, and bin gives `{"value":5,"left":{"value":3},"right":{"value":8}}`.
  - store inserts exactly `{"res":"1111"}` from both forms.
  - Both `.http` code payloads evaluate to the list text.
  - example.js prints `[]\n` through `Deno.core.print`.
- Wrote `docs/js-ops.md` (JSRT-06). Its mapping table has six `Deno.core` rows, and every message in it matches the 03-01 code and tests.
- No legacy syntax is left. `opSync`, `Deno.core.ops()` and unprefixed `rs3_` op names return zero grep matches in `requests/` and `src/`.
- TOOL-03 is green: 132 passed, 2 ignored. The 2 ignored tests are the parked PATCH tests that belong to 03-03. No Rust or Cargo file changed.

## Task Commits

1. **Task 1 (tracer): fac sample and docs/js-ops.md**: `194d661` (docs)
2. **Task 2: sum and bin samples**: `3754d34` (docs)
3. **Task 3: store, REST-client walkthroughs, example.js and TOOL-03**: `f125e06` (docs)

Tracer gate: auto mode is active (`workflow.auto_advance: true`). Task 1's verify re-ran green before expanding, so no checkpoint was synthesised.

## Op Lines Before and After

| Artefact | Before | After |
|----------|--------|-------|
| requests/fac.js | `Deno.core.ops();` / `let num = Number(Deno.core.opSync("rs3_get", "num").toString());` | `let num = Number(Deno.core.ops.op_rs3_get("num").toString());` |
| requests/fac.txt | `...}Deno.core.ops();let num = Number(Deno.core.opSync(\"rs3_get\",\"num\").toString());...` | `...}let num = Number(Deno.core.ops.op_rs3_get(\"num\").toString());...` |
| requests/sum.js | `Deno.core.ops();` / `let numbers = Deno.core.opSync("rs3_get", "numbers").toString();` | `let numbers = Deno.core.ops.op_rs3_get("numbers").toString();` |
| requests/sum.txt | `...}Deno.core.ops();let numbers = Deno.core.opSync(\"rs3_get\",\"numbers\").toString();...` | `...}let numbers = Deno.core.ops.op_rs3_get(\"numbers\").toString();...` |
| requests/bin.js | `Deno.core.ops();` / `let numbers = Deno.core.opSync("rs3_get", "tree").toString();` | `let numbers = Deno.core.ops.op_rs3_get("tree").toString();` |
| requests/bin.txt | `...}Deno.core.ops();let numbers = Deno.core.opSync(\"rs3_get\",\"tree\").toString();...` | `...}let numbers = Deno.core.ops.op_rs3_get(\"tree\").toString();...` |
| requests/store.js | `Deno.core.ops();` / `Deno.core.opSync("rs3_insert", "{\"res\":\"" + p + "\"}");` | `Deno.core.ops.op_rs3_insert("{\"res\":\"" + p + "\"}");` |
| requests/store.txt | `"Deno.core.ops();let p = \"1111\";Deno.core.opSync(\"rs3_insert\",\"{\\\"res\\\":...` | `"let p = \"1111\";Deno.core.ops.op_rs3_insert(\"{\\\"res\\\":...` (argument escapes byte-identical) |
| requests/rs3_http_localhost_5000.http | `"code": "Deno.core.ops();Deno.core.opSync(\"rs3_list\",\"\").toString();"` | `"code": "Deno.core.ops.op_rs3_list().toString();"` |
| requests/rs3_https_rs3_cwe.http | same as above | same as above |
| src/example.js | `Deno.core.ops();` / `let res = Deno.core.opSync('op_list', '');` | `let res = Deno.core.ops.op_rs3_list();` |

## Node Stub-Check Outputs

- Task 1 verify: exit 0. The docs gate also exits 0, with 6 mapping rows.
- Task 2 verify: exit 0.
- Task 3 verify: node check exit 0, grep and JSON gate exit 0, TOOL-03 exit 0.
- Final re-run on the tree at HEAD: `fac 120 120`, `sum 10 10`, `bin "{\"value\":5,\"left\":{\"value\":3},\"right\":{\"value\":8}}"` for both forms.

## docs/js-ops.md Sections

1. Title and introduction
2. Migrating from the legacy API: the mapping table, the `opSync` removal, the unregistered `op_list` note and "no compatibility shim"
3. Errors: the missing-key `Error`, the insert `TypeError` prefixes, object/symbol `TypeError`, a try/catch example, why there is no NotFound class, the uncaught 422 body and nested rethrow
4. PATCH results: a 200 stringification table plus the Promise caveat, the 422 `js_error` examples, 500 `internal`, 404 `not_found`
5. Runtime model: a fresh runtime per PATCH on an `rs3-js` thread, no top-level collisions, a shared store, and listing sorted by key in byte order
6. Arguments and encoding: coercion, the empty list `[]`, insert `{}` writing nothing, and exact UTF-8 strings with no normalisation
7. Sandbox and known limits: no fetch/require/setTimeout, `Deno.core.print` writing to stdout, `op_panic` disabled, unbounded nesting (235/778 levels, about 800 MB, 2.7 s), `while(true){}` pinning a worker, and the Phase 5 422/504
8. Worked example: fac, using `requests/fac.txt`
9. Stored scripts after upgrading

## Interpreted Behaviour

- **The StrArg rule (from 03-01 deviation 2) goes beyond the plan's interface table.** Following the orchestrator note, the doc states the as-built rule:
  - Primitives are coerced (`null` reads `null`, `true` reads `true`).
  - Objects and symbols throw `TypeError: op argument must be a string or another primitive value`, without calling `toString`.
  - The doc tells authors to pass `JSON.stringify(obj)` to insert an object.
- **The nested-error and host-fault wording is taken from `src/js/ops.rs` / `runtime.rs` / `src/http/error.rs`:**
  - A nested missing key arrives as `key not found: <key>`.
  - The 500 body is the fixed `{"error":"internal","message":"internal error"}`.
  - The 404 body is `{"error":"not_found","message":"key not found: <key>"}`.
- **The doc describes PATCH as it will behave once 03-03 wires it.** Today PATCH still answers 501. 03-03 lands in this same phase, and its verification greps this doc for the exact messages.
- **No artefact needed its logic changed.** Every edit was "delete the bootstrap statement and rename the call".

## Decisions Made

See key-decisions in the frontmatter.

## Deviations from Plan

None. The plan was executed exactly as written. Documenting the 03-01 StrArg object/symbol rule was a direct instruction from the orchestrator, not a deviation.

## Issues Encountered

None.

## Known Stubs

None.

## User Setup Required

None. No external service configuration is required.

## Next Phase Readiness

- Plan 03-03 can `include_str!` the migrated `requests/{fac,sum,bin,store}.js` as the TEST-05 fixtures and grep `docs/js-ops.md` for `op is disabled`, `key not found: nope` and the `js_error` body.
- JSRT-05 is shared with 03-03. The shared-ID gate keeps it open until 03-03's SUMMARY exists, so only JSRT-06 was marked complete in REQUIREMENTS.md.

---
*Phase: 03-javascript-runtime-port*
*Completed: 2026-10-20*

## Self-Check: PASSED

All 12 owned files exist. The commits 194d661, 3754d34 and f125e06 are ancestors of HEAD. evaluation-scope resolves the three 03-02 commits. Every task's acceptance criteria and the plan-level verification (node stub checks, legacy-syntax grep gate, docs gate, TOOL-03, ownership guard) re-ran green on the final tree.
