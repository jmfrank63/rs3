import {
  newQuickJSWASMModuleFromVariant,
  newVariant,
  type QuickJSContext,
  type QuickJSHandle,
  type QuickJSSyncVariant,
  type QuickJSWASMModule,
} from "quickjs-emscripten-core";
import wasmfileVariant from "@jitl/quickjs-wasmfile-release-sync";
// Project-local copy of the variant artifact (wrangler's CompiledWasm rule
// does not reach node_modules/): a PRECOMPILED WebAssembly.Module, so
// instantiation needs zero runtime codegen — the workerd-legal path. The
// singlefile variant is DEAD on workerd ("Wasm code generation disallowed
// by embedder"); this wasmfile+newVariant({wasmModule}) shape is the
// live-verified replacement (03-RESEARCH).
import wasmModule from "./quickjs.wasm";
import { findAsyncUsage } from "./gate";
import { fitLogArgs, jsonSafe, MAX_LOG_BYTES } from "./console";
import { HARNESS_SOURCE } from "./harness";
import { markSandboxLoaded } from "./identity";
import {
  MAX_GUEST_SOURCE_BYTES,
  parseLimits,
  type SandboxEnv,
  type SandboxLimits,
} from "./limits";

// Seam symmetry with Typechecker (src/typecheck/index.ts): Phase 4's agent
// loop depends on this interface, never on quickjs directly.
export interface SandboxRuntime {
  execute(code: string): Promise<ExecuteResult>;
}

export interface SandboxLog {
  level: string;
  args: unknown[];
}

export interface SandboxTestResult {
  name: string;
  passed: boolean;
  error?: string;
}

// Status contract (03-CONTEXT + 03-RESEARCH failure-mode matrix):
// - "error": source evaluated, guest JS exception (incl. the catchable
//   InternalError: out of memory / stack overflow) or a pre-eval gate rejection
// - "timeout": interrupt deadline hit inside the guest (uncatchable in practice)
// - "crash": QuickJS/WASM-level host abort (host RangeError or dispose abort)
export interface ExecuteResult {
  status: "ok" | "error" | "timeout" | "crash";
  logs: SandboxLog[];
  tests: SandboxTestResult[];
  result?: unknown;
  error?: string;
  ms: number;
}

// newContext() creates a fresh runtime OWNED by the context — one dispose()
// tears both down, so single-use contexts cannot poison the module singleton
// (SAND-04). A dispose-abort (stack >128KiB class) leaks that runtime's heap
// permanently; the abort counter rebuilds the singleton after 3 (a fresh
// module builds in ~3ms — 03-RESEARCH rows 8/9).
const variant = newVariant<QuickJSSyncVariant>(wasmfileVariant, { wasmModule });
let modulePromise: Promise<QuickJSWASMModule> | undefined;
let abortCount = 0;
const ABORT_REBUILD_THRESHOLD = 3;

function getModule(): Promise<QuickJSWASMModule> {
  return (modulePromise ??= newQuickJSWASMModuleFromVariant(variant));
}

const LOG_LEVELS = ["log", "info", "warn", "error"] as const;
const OPS_BACKSTOP_MULTIPLIER = 4; // handler ≈2.1 calls/ms measured; secondary guard only

function describeError(err: unknown): string {
  return err instanceof Error ? `${err.name}: ${err.message}` : String(err);
}

// dump(errorHandle) can be null (built-in failure without a JS exception —
// matrix row 10). Never read .name/.message unguarded.
function stringifyDump(d: unknown): string {
  if (d === null || d === undefined) return "internal sandbox failure";
  if (typeof d === "string") return d;
  if (typeof d === "object" && "name" in d && "message" in d) {
    const named = d as { name: unknown; message: unknown };
    return `${String(named.name)}: ${String(named.message)}`;
  }
  try {
    return JSON.stringify(d) ?? String(d);
  } catch {
    return String(d);
  }
}

function isInterrupt(d: unknown): boolean {
  return (
    typeof d === "object" &&
    d !== null &&
    (d as { name?: unknown }).name === "InternalError" &&
    String((d as { message?: unknown }).message ?? "").includes("interrupted")
  );
}

// Injected guest surface (SAND-03): ONLY console. No fetch, no VFS, no host
// state — guests get their injected globals + JS builtins, nothing else.
// Each level is a host function: dump() each arg handle (Errors -> {name,
// message, stack}, circulars -> "[object Object]"), dispose EVERY handle
// (VmFunctionImplementation's `this` param is type-level only — the FFI
// applies the guest args directly; no leading this-handle arrives).
// v5 H9: every dumped arg passes jsonSafe() BEFORE byte accounting, so a
// guest BigInt can no longer throw inside stringify accounting and a
// cyclic structure cannot poison later serializations. v5 H10: retention
// is bounded per entry — oversized args are truncated/marked at capture
// time, so a 1 MB log never retains 1 MB.
function injectConsole(context: QuickJSContext, logs: SandboxLog[]): void {
  let logBytes = 0;
  const consoleObj = context.newObject();
  for (const level of LOG_LEVELS) {
    const fn = context.newFunction(level, (...argHandles: QuickJSHandle[]) => {
      const remaining = MAX_LOG_BYTES - logBytes;
      if (remaining <= 0) {
        for (const arg of argHandles) arg.dispose();
        return;
      }
      const dumped = argHandles.map((handle): unknown => {
        const value = context.dump(handle);
        handle.dispose();
        const safe = jsonSafe(value);
        return safe === undefined ? null : safe; // pre-existing null-mapping
      });
      const fitted = fitLogArgs(dumped, remaining);
      logBytes += fitted.bytes;
      logs.push({ level, args: fitted.args });
    });
    context.setProp(consoleObj, level, fn);
    fn.dispose();
  }
  context.setProp(context.global, "console", consoleObj);
  consoleObj.dispose();
}

export class QuickJsSandboxRuntime implements SandboxRuntime {
  constructor(private readonly limits: SandboxLimits) {}

  async execute(code: string): Promise<ExecuteResult> {
    const t0 = performance.now();
    const logs: SandboxLog[] = [];
    const tests: SandboxTestResult[] = [];

    if (code.length > MAX_GUEST_SOURCE_BYTES) {
      return {
        status: "error",
        logs,
        tests,
        error: `guest source is ${code.length} bytes; limit is ${MAX_GUEST_SOURCE_BYTES}`,
        ms: performance.now() - t0,
      };
    }
    const asyncToken = findAsyncUsage(code);
    if (asyncToken !== null) {
      return {
        status: "error",
        logs,
        tests,
        error:
          `async execution is not supported in the sandbox (documented limitation): ` +
          `source contains "${asyncToken}"`,
        ms: performance.now() - t0,
      };
    }

    const module = await getModule();
    markSandboxLoaded();
    const context = module.newContext(); // fresh runtime + context, single use (SAND-04)
    let status: ExecuteResult["status"] = "ok";
    let error: string | undefined;
    let result: unknown;
    // H5: retained host-side handle to the harness's PRIVATE records array.
    let testsHandle: QuickJSHandle | undefined;
    try {
      // Guardrails (SAND-02) BEFORE any guest code runs.
      context.runtime.setMemoryLimit(this.limits.memoryLimitBytes);
      context.runtime.setMaxStackSize(this.limits.stackBytes);
      const deadline = performance.now() + this.limits.timeoutMs;
      let ops = 0;
      // Wall-clock primary: performance.now() ADVANCES inside the interrupt
      // handler during sync guest execution (03-RESEARCH correction — the
      // counter-primary premise was stale). The op counter stays as a cheap
      // secondary backstop in case a future workerd freezes the handler clock.
      context.runtime.setInterruptHandler(
        () => performance.now() > deadline || ++ops > OPS_BACKSTOP_MULTIPLIER * this.limits.timeoutMs,
      );

      injectConsole(context, logs);
      // H5: the harness's completion value IS its private records array.
      // Retain the handle — the only reference outside the harness closures —
      // instead of reading a guest-writable global afterwards.
      const harnessResult = context.evalCode(HARNESS_SOURCE, "harness.js");
      if (harnessResult.error) {
        error = `harness setup failed: ${stringifyDump(context.dump(harnessResult.error))}`;
        harnessResult.error.dispose();
        status = "error";
      } else {
        testsHandle = harnessResult.value;
      }

      if (status === "ok") {
        const evalResult = context.evalCode(code, "guest.js", { type: "global", strict: true });
        if (evalResult.error) {
          const dumped = context.dump(evalResult.error); // may be null — matrix row 10
          evalResult.error.dispose();
          if (isInterrupt(dumped)) {
            status = "timeout";
            error = `execution exceeded the ${this.limits.timeoutMs}ms sandbox deadline`;
          } else {
            status = "error";
            error = stringifyDump(dumped);
          }
        } else {
          // v5 H9: a guest BigInt (1n) used to survive dump() as a host
          // BigInt and 500 the HTTP layer at JSON.stringify; one jsonSafe
          // policy covers the result the same way it covers console args.
          result = jsonSafe(context.dump(evalResult.value));
          evalResult.value.dispose();
        }
      }

      // Tests registered BEFORE a guest throw still landed in the private
      // records (test() runs fns at registration) — read them for feedback.
      // Skipped on timeout: the guest aborted mid-flight; its state is not
      // trustworthy.
      if (status !== "timeout" && testsHandle !== undefined) {
        const dumpedTests: unknown = context.dump(testsHandle);
        if (Array.isArray(dumpedTests)) {
          for (const t of dumpedTests) {
            const entry = t as { name?: unknown; passed?: unknown; error?: unknown };
            tests.push({
              name: String(entry.name ?? "(unnamed)"),
              passed: entry.passed === true,
              ...(entry.error === undefined ? {} : { error: String(entry.error) }),
            });
          }
        }
      }
    } catch (err) {
      // Host-level failure: evalCode RangeError (native stack >128KiB class)
      // or any QuickJS abort — a catchable host exception, status "crash".
      status = "crash";
      error = describeError(err);
      abortCount += 1;
    } finally {
      // H5: release the retained records handle on every path (dump already
      // released it on the read path; the timeout path disposes here).
      if (testsHandle !== undefined) {
        try {
          testsHandle.dispose();
        } catch {
          // context may already be dying — nothing to salvage
        }
        testsHandle = undefined;
      }
      // dispose() can abort EVEN WHEN eval did not (matrix row 8) — guard it
      // separately so the dispose exception cannot mask the real cause.
      try {
        context.dispose();
      } catch (disposeErr) {
        status = "crash";
        error ??= `dispose: ${describeError(disposeErr)}`;
        abortCount += 1;
      }
    }
    if (abortCount >= ABORT_REBUILD_THRESHOLD) {
      modulePromise = undefined; // rebuild singleton on next execute (~3ms)
      abortCount = 0;
    }
    return {
      status,
      logs,
      tests,
      ...(result !== undefined ? { result } : {}),
      ...(error !== undefined ? { error } : {}),
      ms: performance.now() - t0,
    };
  }
}

// One runtime instance per request (limits come from per-request env); the
// WASM module singleton is shared module-level state. Env is structurally
// satisfied by the worker's Env interface.
export function getSandboxRuntime(env: SandboxEnv): SandboxRuntime {
  return new QuickJsSandboxRuntime(parseLimits(env));
}
