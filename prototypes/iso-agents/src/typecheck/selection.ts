// TSCHECKER env selection (WASM-03): pure parsing, zero imports, so vitest
// specs exercise it without pulling in either checker graph (typescript
// 8.8 MB or the 5.1 MB wasm module — the Phase 2 transport lesson).
export type TSCheckerSelection = "ts-js" | "tsc-rs-wasm";

export function parseTSChecker(env?: { TSCHECKER?: string }): TSCheckerSelection {
  const raw = env?.TSCHECKER;
  if (raw === undefined || raw === "" || raw === "ts-js") return "ts-js";
  if (raw === "tsc-rs-wasm") return "tsc-rs-wasm";
  throw new Error(`TSCHECKER must be "ts-js" or "tsc-rs-wasm"; got "${raw}"`);
}

// FORK-02: TSCHECKER_WASM_RECYCLE_AFTER overrides the held wasm instance's
// run-count recycle threshold (default 50 = RECYCLE_AFTER_RUNS in
// src/typecheck-wasm/shim.ts). undefined when unset/empty so the shim's
// default applies; throws honestly on garbage, same style as parseTSChecker.
export function parseTSCheckerWasmRecycleAfter(
  env?: { TSCHECKER_WASM_RECYCLE_AFTER?: string },
): number | undefined {
  const raw = env?.TSCHECKER_WASM_RECYCLE_AFTER;
  if (raw === undefined || raw === "") return undefined;
  const n = Number(raw);
  if (!Number.isInteger(n) || n < 1) {
    throw new Error(`TSCHECKER_WASM_RECYCLE_AFTER must be a positive integer; got "${raw}"`);
  }
  return n;
}
