# Benchmark: native tsc-rs vs the in-isolate checkers (ts-js + tsc-rs-wasm fresh/held)

Reproduce with `npm run bench:tsc-rs` — it boots THREE `wrangler dev` workers (default ts-js +
TSCHECKER=tsc-rs-wasm fresh via TSCHECKER_WASM_RECYCLE_AFTER:1 + TSCHECKER=tsc-rs-wasm held),
prints this table, and rewrites this file. README.md commits the same numbers.

Generated: 2026-10-15T10:22:38.524Z

| snippet          | tsc-rs cold ms | tsc-rs warm med(10) | tsc-rs warm min | ts-js cold ms | ts-js warm med(5) | wasm fresh cold ms | wasm fresh warm med(5) | wasm held cold ms | wasm held warm med(5) | errors (exp/got) |
|------------------|----------------|---------------------|-----------------|---------------|-------------------|--------------------|------------------------|-------------------|-----------------------|------------------|
| slugify-iter1    | 41.4 | 34.4 | 31.1 | 225.0 | 7.0 | 548.0 | 173.0 | 539.0 | 1.0 | 2/2 |
| slugify-final    | 34.1 | 31.8 | 29.5 | 6.0 | 5.0 | 176.0 | 158.0 | 121.0 | 1.0 | 0/0 |
| fibonacci-final  | 49.4 | 30.0 | 29.0 | 11.0 | 5.0 | 154.0 | 151.0 | 104.0 | 1.0 | 0/0 |
| lib-heavy        | 34.3 | 30.6 | 29.7 | 31.0 | 12.0 | 150.0 | 144.0 | 20.0 | 0.0 | 0/0 |
| synthetic-100    | 35.5 | 31.2 | 30.2 | 22.0 | 14.0 | 148.0 | 144.0 | 104.0 | 1.0 | 0/0 |

tsc-rs native spawn+init: 2.6 ms median across the 5 API instances (not part of the cold column).
Machine: linux arm64, v24.19.0, tsc-rs 0.2.0 (native), workerd via wrangler dev, typescript 6.0.3 in-isolate (ts-js), ts-rust wasm multi-run-incr/79ffdb0339d0 (fork of pin 569df52) in-isolate (TSCHECKER=tsc-rs-wasm; fresh = TSCHECKER_WASM_RECYCLE_AFTER:1 boot, held = default boot).

Footnotes:
- tsc-rs warm med(10) = median of 10 subsequent createProgram+diagnostics checks on the SAME API instance (persistent native subprocess, sync-RPC); cold = the first such check after spawn.
- In-isolate cold = POST /typecheck data.coldMs (run 1 of 6); warm med(5) = median of runs 2-6 (runs: 6 so there are 5 warm samples; the endpoint default of 5 yields only 4).
- Lib-closure difference, footnoted rather than forced to parity: tsc-rs checks against its default lib.es2022.full chain INCLUDING DOM; the in-isolate checker uses the bundled 115-file es2022 core closure (no DOM). All five snippets type-check identically on both.
- Both legs share the guest surface: /guest-globals.d.ts (repo GUEST_GLOBALS_TEXT) is an explicit rootName on the tsc-rs leg and prepended implicitly by the in-isolate checker; options mirror GUEST_COMPILER_OPTIONS (strict, ES2022, noEmit, ESNext modules, Bundler resolution, skipLibCheck, isolatedModules, forceConsistentCasingInFileNames).
- tsc-rs is host-side only on the native leg (binary subprocess). Its WASM build is the third and fourth legs: the vendored ts_rust.wasm (unpublished; built from the multi-run FORK — branch multi-run @ 79ffdb0339d0 of pin 569df52 — docs/ts-rust-wasm.md) runs INSIDE the isolate behind the same Typechecker interface.
- wasm fresh cold/warm = POST /typecheck against a wrangler dev boot with TSCHECKER_WASM_RECYCLE_AFTER:1 — every run drops and re-instantiates the WebAssembly.Instance (2-8 ms), i.e. Phase 6's fresh-instance semantics (asserted per snippet: runsOnInstance 1 with a cumulative gen 6/12/18/24/30 — one recycle per run). wasm held cold/warm = the default boot: ONE held instance per isolate serves ALL snippets (asserted: instanceGen 1, recycles 0 across all 30 runs); run 1 is cold, runs 2-6 ride the fork's mtime-keyed parse cache of unchanged files. Both boots use the same 1 cold + 5 warm protocol; the lib.d.ts closure is embedded in the module (strict + es2022 tsconfig under /app, mirroring the ts-js guest surface).
- The wasm module ships unconditionally in the worker bundle (+5,150,781 bytes raw); local dev does not enforce CPU limits — a fresh wasm check costs ~0.5-1.2 s of synchronous CPU, a warm held check ~70-125 ms (07-RESEARCH).
