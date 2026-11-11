import ts from "typescript";
import { GUEST_COMPILER_OPTIONS, makeHost } from "./host";
import { GUEST_GLOBALS } from "./vfs";
import { parseTSChecker, parseTSCheckerWasmRecycleAfter } from "./selection";
import { TscRsWasmTypechecker } from "../typecheck-wasm/checker";

export interface TypecheckDiagnostic {
  code: number;
  message: string;
  /** 1-based; -1 when the diagnostic has no file/start (global/config diags). */
  line: number;
  /** 1-based; -1 when the diagnostic has no file/start. */
  column: number;
  file: string;
  category: string;
}

export interface TypecheckResult {
  ok: boolean;
  diagnostics: TypecheckDiagnostic[];
  ms: number;
}

// tsc-rs drop-in seam. THREE implementations prove it is genuine:
// TsTypechecker (pure-JS typescript@6.0.3, the default), TranspileOnly
// (the Phase 2 seam proof), and TscRsWasmTypechecker (the real Rust
// checker, vendored as src/typecheck-wasm/ts_rust.wasm from the multi-run
// fork — branch multi-run @ 5ada636ae of upstream pin 569df52 —
// unpublished on npm; see docs/ts-rust-wasm.md). The agent loop
// (Phase 4), /agent/shell and the /typecheck benchmark depend on
// `Typechecker`, never on a concrete checker — swapping implementations
// changes nothing above this line.
// Optional per-isolate held-instance stats (FORK-02/03): implemented only
// by TscRsWasmTypechecker; the /typecheck endpoint surfaces it so the live
// harness can prove instance reuse + recycling over HTTP. Other checkers
// omit it (the field is absent from their responses).
export interface TypecheckerHeldStats {
  instanceGen: number;
  runsOnInstance: number;
  runsTotal: number;
  recycles: number;
  memoryBytes: number | null;
}

export interface Typechecker {
  readonly id: string;
  check(files: Map<string, string>): Promise<TypecheckResult>;
  heldStats?(): TypecheckerHeldStats;
}

// Boot-time identity assert (TYPE-01): an accidental upgrade to npm "latest"
// (native tsgo 7.x) must fail loudly here, not silently break in-isolate
// checking. Runs once, on the first ts-js checker build.
function assertCompilerIdentity(): void {
  if (ts.version !== "6.0.3") {
    throw new Error(
      `FATAL: in-isolate typechecker requires exactly typescript@6.0.3 (pure-JS line); ` +
        `loaded "${ts.version}". npm "latest" (7.x) is the NATIVE tsgo build and cannot run in workerd.`,
    );
  }
  if (typeof ts.createProgram !== "function" || typeof ts.getPreEmitDiagnostics !== "function") {
    throw new Error("FATAL: typescript createProgram/getPreEmitDiagnostics missing — API shape changed");
  }
  if (typeof (globalThis as Record<string, unknown>).process !== "undefined") {
    throw new Error(
      "FATAL: process global present — no_nodejs_compat flags lost; isNodeLikeSystem() would take the Node branch",
    );
  }
}

// Exported for src/typecheck/transpile.ts (Phase 4): transpile diagnostics
// share the exact {code,message,line,column,file,category} shape.
export function mapDiagnostic(d: ts.Diagnostic): TypecheckDiagnostic {
  const loc =
    d.file !== undefined && d.start !== undefined
      ? ts.getLineAndCharacterOfPosition(d.file, d.start) // 0-based
      : { line: -1, character: -1 }; // global/config diags carry no file/start
  return {
    code: d.code,
    message: ts.flattenDiagnosticMessageText(d.messageText, "\n"),
    line: loc.line + 1,
    column: loc.character + 1,
    file: d.file?.fileName ?? "(program)",
    category: ts.DiagnosticCategory[d.category],
  };
}

function byLocation(a: TypecheckDiagnostic, b: TypecheckDiagnostic): number {
  return a.file.localeCompare(b.file) || a.line - b.line || a.column - b.column;
}

export class TsTypechecker implements Typechecker {
  readonly id = "typescript@6.0.3 (in-isolate)";

  async check(files: Map<string, string>): Promise<TypecheckResult> {
    const host = makeHost(files);
    const t0 = performance.now();
    const program = ts.createProgram({
      rootNames: [GUEST_GLOBALS, ...files.keys()],
      options: GUEST_COMPILER_OPTIONS,
      host,
    });
    const diagnostics = ts.getPreEmitDiagnostics(program);
    const ms = performance.now() - t0;
    const mapped = diagnostics.map(mapDiagnostic).sort(byLocation);
    return { ok: mapped.every((d) => d.category !== "Error"), diagnostics: mapped, ms };
  }
}

// Second implementation (TYPE-04 seam proof): same interface, no
// createProgram — transpiles each file, always reports ok. A future tsc-rs
// WASM build plugs in exactly like this class does.
export class TranspileOnlyTypechecker implements Typechecker {
  readonly id = "transpile-only (seam proof)";

  async check(files: Map<string, string>): Promise<TypecheckResult> {
    const t0 = performance.now();
    for (const text of files.values()) {
      ts.transpileModule(text, {
        compilerOptions: { target: ts.ScriptTarget.ES2022, isolatedModules: true },
      });
    }
    return { ok: true, diagnostics: [], ms: performance.now() - t0 };
  }
}

export type TypecheckerKind = "ts" | "transpile-only";

export function createTypechecker(kind: TypecheckerKind = "ts"): Typechecker {
  return kind === "ts" ? new TsTypechecker() : new TranspileOnlyTypechecker();
}

// Lazy module-level singletons (CONTEXT decision), one per implementation:
// construction on first call — for ts-js that includes the boot assert, and
// lib parse happens inside the first real check. getTypecheckerIdentity(env)
// never triggers init, so /healthz stays fast and flips only after the
// SELECTED checker is genuinely used.
let tsSingleton: Typechecker | undefined;
let wasmSingleton: Typechecker | undefined;

export function getTypechecker(env?: {
  TSCHECKER?: string;
  TSCHECKER_WASM_RECYCLE_AFTER?: string;
}): Typechecker {
  if (parseTSChecker(env) === "tsc-rs-wasm") {
    return (wasmSingleton ??= new TscRsWasmTypechecker(parseTSCheckerWasmRecycleAfter(env)));
  }
  if (tsSingleton === undefined) {
    assertCompilerIdentity();
    tsSingleton = new TsTypechecker();
  }
  return tsSingleton;
}

export function getTypecheckerIdentity(env?: { TSCHECKER?: string }): string {
  const loaded = parseTSChecker(env) === "tsc-rs-wasm" ? wasmSingleton : tsSingleton;
  return loaded !== undefined ? loaded.id : "unloaded";
}
