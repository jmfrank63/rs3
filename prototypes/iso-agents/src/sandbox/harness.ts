// Guest-side assertion harness (03-RESEARCH): injected as SOURCE in a first
// evalCode — NOT as host functions — because context.throw() from a host
// function does not propagate into the guest call stack (a host-function
// harness silently recorded a failing assertion as PASSED).
//
// v5 H5: the test records are a CLOSURE-PRIVATE array, never a guest global.
// The old implementation kept them in a writable `globalThis.__tests`, so a
// guest could clear the evidence or push fabricated passes; the loop trusted
// whatever it read back. Now the ONLY host-side reference to the records is
// the eval completion value — a retained QuickJS handle the guest cannot
// reach — and the harness functions are locked in place
// (writable/configurable: false), so strict-mode guests that try to replace
// test/expect/assert throw instead of quietly feeding the loop fake results.
//
// makeHarness is deliberately SELF-CONTAINED (no references outside its own
// body) so HARNESS_SOURCE can be its exact stringified form: the runtime
// evals `(makeHarness)(globalThis)` and the completion value IS the private
// records array. Specs exercise makeHarness directly (no dynamic codegen —
// workerd forbids it), and the live sandbox harness proves the SOURCE path
// end-to-end in real QuickJS.
//
// STRINGIFICATION CONTRACT (live-proven against wrangler's esbuild
// keepNames): the bundled makeHarness body comes back carrying `__name(...)`
// statements — the bundle's minify-name helper — so HARNESS_SOURCE wraps the
// stringified function in an IIFE providing an identity __name shim (see
// HARNESS_SOURCE below for the live-repro rationale).
//
// SHAPE CONTRACT: makeHarness must stay SELF-CONTAINED (no references
// outside its own body) so its stringified form is safe to eval. Helpers are
// function DECLARATIONS and object methods use SHORTHAND syntax — the two
// shapes esbuild emits verbatim in the body (expression forms are rewritten
// inline, which the shim below absorbs anyway).
export interface HarnessRecord {
  name: string;
  passed: boolean;
  error?: string;
}

export function makeHarness(globalObject: object): HarnessRecord[] {
  const records: HarnessRecord[] = [];
  function isNaNValue(value: unknown): boolean {
    return typeof value === "number" && value !== value;
  }
  // H6: structural equality without JSON-stringify's pitfalls — NaN never
  // equals anything (not even NaN), undefined !== a function, arrays and
  // plain objects compare recursively, functions only by reference.
  function deepEqual(a: unknown, b: unknown): boolean {
    if (isNaNValue(a) || isNaNValue(b)) {
      return false;
    }
    if (a === b) {
      return true;
    }
    if (a === null || b === null) {
      return false;
    }
    if (typeof a !== typeof b) {
      return false;
    }
    if (typeof a !== "object") {
      return false;
    }
    const objA = a as Record<string, unknown>;
    const objB = b as Record<string, unknown>;
    const aIsArray = Array.isArray(objA);
    if (aIsArray !== Array.isArray(objB)) {
      return false;
    }
    if (aIsArray) {
      const arrA = a as unknown[];
      const arrB = b as unknown[];
      if (arrA.length !== arrB.length) {
        return false;
      }
      for (let i = 0; i < arrA.length; i++) {
        if (!deepEqual(arrA[i], arrB[i])) {
          return false;
        }
      }
      return true;
    }
    const keysA = Object.keys(objA);
    const keysB = Object.keys(objB);
    if (keysA.length !== keysB.length) {
      return false;
    }
    for (let j = 0; j < keysA.length; j++) {
      const key = keysA[j];
      if (!Object.prototype.hasOwnProperty.call(objB, key)) {
        return false;
      }
      if (!deepEqual(objA[key], objB[key])) {
        return false;
      }
    }
    return true;
  }
  // JSON renders NaN/Infinity as null — show the truth instead (H6).
  function shown(value: unknown): string {
    if (typeof value === "number" && !isFinite(value)) {
      return String(value);
    }
    try {
      const text = JSON.stringify(value);
      return text === undefined ? String(value) : text;
    } catch (e) {
      return String(value);
    }
  }
  function testFn(name: unknown, fn: () => void): void {
    try {
      fn();
      records.push({ name: String(name), passed: true });
    } catch (e) {
      const err = e as { name?: unknown; message?: unknown };
      records.push({
        name: String(name),
        passed: false,
        error:
          err && err.name !== undefined
            ? String(err.name) + ": " + String(err.message)
            : String(e),
      });
    }
  }
  function expectFn(actual: unknown) {
    return {
      toEqual(expected: unknown): void {
        if (!deepEqual(actual, expected)) {
          throw new Error("expected " + shown(expected) + ", got " + shown(actual));
        }
      },
      toBeTrue(msg?: string): void {
        if (actual !== true) {
          throw new Error(msg === undefined ? "expected true, got " + shown(actual) : msg);
        }
      },
      // H6: validates finite number operands and a non-negative finite
      // tolerance — the old `NaN > tolerance` comparison silently PASSED
      // for NaN actuals.
      toBeCloseTo(expected: number, eps?: number): void {
        const tolerance = eps === undefined ? 1e-6 : eps;
        if (
          typeof actual !== "number" ||
          !isFinite(actual) ||
          typeof expected !== "number" ||
          !isFinite(expected)
        ) {
          throw new Error(
            "toBeCloseTo needs finite numbers, got " + shown(actual) + " vs " + shown(expected),
          );
        }
        if (typeof tolerance !== "number" || !isFinite(tolerance) || tolerance < 0) {
          throw new Error(
            "toBeCloseTo tolerance must be a non-negative finite number, got " + shown(tolerance),
          );
        }
        if (Math.abs(actual - expected) > tolerance) {
          throw new Error("expected ~" + expected + ", got " + actual);
        }
      },
    };
  }
  function assertFn(cond: unknown, msg?: string): void {
    if (!cond) {
      throw new Error(msg === undefined ? "assertion failed" : msg);
    }
  }
  function lock(name: string, fn: unknown): void {
    Object.defineProperty(globalObject, name, {
      value: fn,
      writable: false,
      enumerable: true,
      configurable: false,
    });
  }
  lock("test", testFn);
  lock("expect", expectFn);
  lock("assert", assertFn);
  return records;
}

// STRINGIFICATION CONTRACT (live-proven against wrangler's esbuild
// keepNames): the bundled makeHarness source comes back decorated with
// `__name(...)` statements — the bundle's minify-name helper, which does NOT
// exist inside QuickJS (live repro: "ReferenceError: '__name' is not
// defined" on every harness setup). The injected source therefore wraps the
// stringified function in an IIFE that defines an identity __name shim
// first. The shim lives inside the template string, which esbuild cannot
// decorate back; the IIFE scope also keeps it off the guest global.
export const HARNESS_SOURCE = [
  "(function () {",
  "  var __name = function (target, value) { return target; };",
  `  return (${makeHarness})(globalThis);`,
  "})();",
].join("\n");
