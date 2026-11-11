# iso-agents

A prototype of an AI coding agent that lives entirely inside a Cloudflare workerd isolate:
given a small coding task and a spec, it generates TypeScript, type-checks it in-isolate,
executes it in a WASM sandbox, reads the diagnostics, and iterates until the code is green
— all within a single request. See AGENTS.md for project context and stack decisions.

## Why this exists

The tsc-rs video thesis: an agent's write → check → run → fix loop only becomes viable inside
a request-scoped runtime when type-checking is fast enough to run many times per request.
This repo maps that stack onto a workerd isolate — and is honest about where each piece runs
today:

| Piece            | The video's stack                          | This repo                                                                                                                                                                                                                                     |
| ---------------- | ------------------------------------------ | --------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| Type-checker     | tsc-rs (native TS7-port lineage)           | **Both sides now.** Native host-side for the bench's first leg — and INSIDE the isolate: the multi-run fork (branch `multi-run` @ `c1b8218339d0` of pin 569df52), vendored as `src/typecheck-wasm/ts_rust.wasm` (unpublished; built from source — nothing is on npm). Selected with `TSCHECKER=tsc-rs-wasm`, default `ts-js`. |
| Drop-in seam     | swap the compiler in                       | The `Typechecker` interface (src/typecheck/index.ts) — the agent loop, `/agent/shell` and `/typecheck` depend on the interface, never on a concrete checker; the WASM build plugged in exactly this way (`TscRsWasmTypechecker` via `TSCHECKER=tsc-rs-wasm`), as did `TranspileOnlyTypechecker`. |
| In-isolate check | —                                          | `TSCHECKER` selects: `ts-js` (default) = pure-JS `typescript@6.0.3` with a fully in-memory CompilerHost (bundled es2022 core lib closure, libs memoized per isolate, warm checks in single-digit-to-low ms); `tsc-rs-wasm` = the vendored Rust checker (embedded lib closure, ONE held wasm instance per isolate — recycled after 50 runs or on any error; `TSCHECKER_WASM_RECYCLE_AFTER` overrides the threshold). |
| Execution        | run the code                               | QuickJS-WASM **inside the same isolate** — counter+wall-clock deadline interrupt, memory/stack limits, fresh single-use context, captured console, and zero host eval (`npm run gate:no-host-eval`).                                             |
| Agent surface    | a shell on a box                           | `POST /agent/run` (the request-scoped loop) and `POST /agent/shell` (a stateless tool-protocol session, below) — the operational surface an external agent harness drives over HTTP.                                                             |

## Benchmark

Measured, not asserted. Reproduce with `npm run bench:tsc-rs` — it checks the same five guest
snippets with the native tsc-rs binary (host-side JS API, cold + 10 warm runs) and BOTH
in-isolate checkers (two booted `wrangler dev` workers: default ts-js, then
`--var TSCHECKER:tsc-rs-wasm`; POST /typecheck, runs: 6 → 1 cold + 5 warm each), writes
[docs/benchmarks.md](docs/benchmarks.md), and prints exactly this table:

<!-- BEGIN BENCH TABLE — paste the exact table + spawn line + machine line + footnotes
     emitted by scripts/bench-tsc-rs.mjs between these markers. Real numbers from a real
     run only; never hand-edit. -->

| snippet          | tsc-rs cold ms | tsc-rs warm med(10) | tsc-rs warm min | ts-js cold ms | ts-js warm med(5) | wasm fresh cold ms | wasm fresh warm med(5) | wasm held cold ms | wasm held warm med(5) | errors (exp/got) |
|------------------|----------------|---------------------|-----------------|---------------|-------------------|--------------------|------------------------|-------------------|-----------------------|------------------|
| slugify-iter1    | 155.0 | 31.3 | 29.9 | 266.0 | 7.0 | 567.0 | 179.0 | 598.0 | 96.0 | 2/2 |
| slugify-final    | 33.3 | 31.4 | 29.8 | 6.0 | 6.0 | 164.0 | 160.0 | 91.0 | 82.0 | 0/0 |
| fibonacci-final  | 34.1 | 30.1 | 29.2 | 11.0 | 5.0 | 167.0 | 151.0 | 90.0 | 80.0 | 0/0 |
| lib-heavy        | 33.2 | 29.8 | 28.6 | 33.0 | 13.0 | 181.0 | 165.0 | 87.0 | 77.0 | 0/0 |
| synthetic-100    | 34.1 | 30.8 | 29.4 | 22.0 | 15.0 | 160.0 | 154.0 | 83.0 | 78.0 | 0/0 |

tsc-rs native spawn+init: 2.5 ms median across the 5 API instances (not part of the cold column).
Machine: linux arm64, v24.19.0, tsc-rs 0.2.0 (native), workerd via wrangler dev, typescript 6.0.3 in-isolate (ts-js), ts-rust wasm multi-run/c1b8218339d0 (fork of pin 569df52) in-isolate (TSCHECKER=tsc-rs-wasm; fresh = TSCHECKER_WASM_RECYCLE_AFTER:1 boot, held = default boot).

Footnotes:
- tsc-rs warm med(10) = median of 10 subsequent createProgram+diagnostics checks on the SAME API instance (persistent native subprocess, sync-RPC); cold = the first such check after spawn.
- In-isolate cold = POST /typecheck data.coldMs (run 1 of 6); warm med(5) = median of runs 2-6 (runs: 6 so there are 5 warm samples; the endpoint default of 5 yields only 4).
- Lib-closure difference, footnoted rather than forced to parity: tsc-rs checks against its default lib.es2022.full chain INCLUDING DOM; the in-isolate checker uses the bundled 115-file es2022 core closure (no DOM). All five snippets type-check identically on both.
- Both legs share the guest surface: /guest-globals.d.ts (repo GUEST_GLOBALS_TEXT) is an explicit rootName on the tsc-rs leg and prepended implicitly by the in-isolate checker; options mirror GUEST_COMPILER_OPTIONS (strict, ES2022, noEmit, ESNext modules, Bundler resolution, skipLibCheck, isolatedModules, forceConsistentCasingInFileNames).
- tsc-rs is host-side only on the native leg (binary subprocess). Its WASM build is the third and fourth legs: the vendored ts_rust.wasm (unpublished; built from the multi-run FORK — branch multi-run @ c1b8218339d0 of pin 569df52 — docs/ts-rust-wasm.md) runs INSIDE the isolate behind the same Typechecker interface.
- wasm fresh cold/warm = POST /typecheck against a wrangler dev boot with TSCHECKER_WASM_RECYCLE_AFTER:1 — every run drops and re-instantiates the WebAssembly.Instance (2-8 ms), i.e. Phase 6's fresh-instance semantics (asserted per snippet: runsOnInstance 1 with a cumulative gen 6/12/18/24/30 — one recycle per run). wasm held cold/warm = the default boot: ONE held instance per isolate serves ALL snippets (asserted: instanceGen 1, recycles 0 across all 30 runs); run 1 is cold, runs 2-6 ride the fork's mtime-keyed parse cache of unchanged files. Both boots use the same 1 cold + 5 warm protocol; the lib.d.ts closure is embedded in the module (strict + es2022 tsconfig under /app, mirroring the ts-js guest surface).
- The wasm module ships unconditionally in the worker bundle (+5,150,781 bytes raw); local dev does not enforce CPU limits — a fresh wasm check costs ~0.5-1.2 s of synchronous CPU, a warm held check ~70-125 ms (07-RESEARCH).

<!-- END BENCH TABLE -->

Honest reading (measured on this machine): warm-for-warm on these tiny (~350-byte) guests the
ts-js checker stays competitive with native tsc-rs; the wasm checker pays a much larger
per-check CPU cost for the real Rust compiler INSIDE the isolate — the video's stack story,
now executed where the loop runs. The Phase 7 multi-run fork closes the gap from the other
side: the SAME held wasm instance re-serves unchanged files' parses, so warm checks drop to
~70-125 ms (the "wasm held" columns) while "wasm fresh" keeps the Phase 6
per-run-instance numbers honest side by side. Build provenance and rebuild steps:
[docs/ts-rust-wasm.md](docs/ts-rust-wasm.md).

## Running agents on this isolate

Every endpoint speaks one JSON envelope: `{"ok": true, "data": …}` or
`{"ok": false, "error": {"code", "message", "detail?"}}`. No auth, no API keys; all state is
in-memory and dies with the isolate.

| Endpoint          | Method | Body                                            | Returns                                                                                                                              |
| ----------------- | ------ | ----------------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------ |
| `/healthz`        | GET    | —                                               | runtime identity: workerd, compatibility date, typechecker/sandbox load state (reflects the `TSCHECKER` selection), agent provider, isolate `instanceId` |
| `/agent/run`      | POST   | `{task, spec, maxIterations?}`                  | the full run: iterations (LLM output, files, diagnostics, sandbox results, feedback), per-step timings, terminal status                 |
| `/agent/run/:id`  | GET    | —                                               | the stored run (honest 404 for unknown ids — in-memory store)                                                                          |
| `/agent/shell`    | POST   | `{commands: [{op, args}...], files?, stopOnError?}` | per-op results `[{op, ok, result, ms}]` (+ `haltedAt` when `stopOnError` halted); ops: `ls`/`cat`/`write`/`rm`/`typecheck`/`run` over a per-request VFS (lands with plan 05-02) |
| `/typecheck`      | POST   | `{files: {name: content}, runs?}`               | N-run latency stats `{runs, results, minMs, medianMs, meanMs, coldMs, held?}` — `held` (wasm checker only) exposes the held instance's `{instanceGen, runsOnInstance, runsTotal, recycles, memoryBytes}` |
| `/sandbox/execute`| POST   | `{code}`                                        | structured `ExecuteResult` `{status, logs, tests, result?, error?, ms}` (200 even when the guest timed out — the execution succeeded)   |

## Commands

- `npm run dev` — start `wrangler dev` (real workerd, local, no Cloudflare account) on http://localhost:8787
- `npm run check` — strict type-check (`tsc --noEmit`)
- `npm test` — vitest run (small-module specs only; worker-graph behavior lives in the live harnesses)
- `npm run test:live` — the live harnesses under real workerd (typecheck ts-js, sandbox, agent loop, agent shell, typecheck tsc-rs-wasm)
- `npm run bench:tsc-rs` — the benchmark table above (boots THREE `wrangler dev` workers: ts-js, tsc-rs-wasm fresh via `TSCHECKER_WASM_RECYCLE_AFTER:1`, tsc-rs-wasm held)
- `npm run bundle:check` — dry-run production bundle gate (no upload)
- `npm run gate:no-host-eval` — grep gate: no `eval`/`new Function` in src/ (QuickJS-WASM is the only execution path)
- `TSCHECKER=ts-js|tsc-rs-wasm` — checker selection (default `ts-js`); under `wrangler dev`, pass it as `--var TSCHECKER:tsc-rs-wasm`. `/healthz` identity flips to `ts-rust@multi-run/c1b8218 (wasm)` after the first check in wasm mode. `TSCHECKER_WASM_RECYCLE_AFTER=<n>` overrides the held wasm instance's recycle threshold (default 50).

## Smoke test

```sh
npm run dev &
sleep 8
curl -s --max-time 5 http://localhost:8787/healthz
```

Returns the JSON envelope with the full runtime identity — `typechecker`/`sandbox` report
`"unloaded"` until the first real use (lazy singletons), and `workerd`/`userAgent` report
`navigator.userAgent` (best-effort: workerd has no in-band version API).
